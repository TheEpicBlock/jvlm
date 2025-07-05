#![feature(exit_status_error)]
#![feature(file_buffered)]

use std::{env, ffi::OsString, fs::{self, File}, io::Write, path::PathBuf, process::Command};
use toml::Table;
use wax::Glob;

const RUST_GIT_SOURCE: &str = "https://github.com/rust-lang/rust.git";
const TARGET_RUST_COMMIT: &str = "60dabef95a3de3ec974dcb50926e4bfe743f078f";
const GIT_SPARSE_FILTER: &str = "/compiler/rustc_codegen_llvm/*\n/compiler/rustc_llvm/*";
const LLVM_VERSION: &str = "201";

/// We transform rustc_codegen_llvm into a module inside our code, this defines what that module will be called.
const LLVM_MODULE: &str = "codegen_llvm";

fn main() {
    // Download the source
    let rust_source = download_rust_source();
    let codegen_llvm_source = rust_source.join("compiler/rustc_codegen_llvm/");
    if !fs::exists(&codegen_llvm_source).unwrap() {
        panic!("Where's rustc_codegen_llvm ??")
    }

    // Build rustc_llvm
    let llvm_conf = find_llvm_config();
    let rustc_llvm = rust_source.join("compiler/rustc_llvm/");
    let mut cc = cc::Build::new();
    cc.warnings(false);
    cc.files(Glob::new("**/*.cpp").unwrap().walk(rustc_llvm).map(|e| e.unwrap().into_path()));
    cc.cpp(true);
    let cxxflags = Command::new(llvm_conf).arg("--cxxflags").output().unwrap().stdout;
    String::from_utf8(cxxflags).unwrap().split_ascii_whitespace().for_each(|f| {cc.flag(f);});
    cc.compile("llvm-wrapper");

    // Process rustc_codegen_llvm
    copy_codegen_llvm(codegen_llvm_source);

    // TODO: set this to something accurate
    println!("cargo::rustc-env=CFG_VERSION=testtest");
    
    // We want to run if we change yeah
    println!("cargo::rerun-if-changed=build.rs");
}

/// Grabs the downloaded rustc_codegen_llvm and transforms it into a module which we can then "import" into our
/// code via an `include!()`
fn copy_codegen_llvm(codegen_llvm_source: PathBuf) {
    // Parse the dependencies of the original codegen_llvm
    let cargo_toml = fs::read_to_string((&codegen_llvm_source).join("Cargo.toml")).unwrap();
    let cargo_toml: Table = toml::from_str(&cargo_toml).unwrap();
    let dependencies = cargo_toml.get("dependencies").unwrap().as_table().unwrap();
    let dependencies: Vec<&String> = dependencies.iter()
        .map(|(key, _v)| key)
        .collect();

    // Write the header
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut header_out = File::create(out.join("llvm_include.rs")).unwrap();
    for d in dependencies {
        writeln!(header_out, "extern crate {};", d.replace("-", "_")).unwrap();
    }
    // We need to reexport this because other tooling expects this to be available at the crate root
    writeln!(header_out, "pub(crate) use {LLVM_MODULE}::fluent_generated;").unwrap();
    writeln!(header_out, "#[path=\"{}\"]", out.join("src/lib.rs").display()).unwrap();
    writeln!(header_out, "#[allow(warnings)]").unwrap(); // I do not want to hear codegen_llvm's warnings
    writeln!(header_out, "mod {LLVM_MODULE};").unwrap();
    
    // Write the rest of codegen_llvm
    let messages = fs::read_to_string(codegen_llvm_source.join("messages.ftl")).unwrap();
    let messages = messages.replace("codegen_llvm_", "codegen_jvlm_codegen_llvm_");
    File::create(out.join("messages.ftl")).unwrap().write_all(messages.as_bytes()).unwrap();
    let src = codegen_llvm_source.join("src");
    for entry in Glob::new("**/*.rs").unwrap().walk(&src) {
        let entry = entry.unwrap();
        let target = out.join("src").join(entry.path().strip_prefix(&src).unwrap());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let contents = fs::read_to_string(entry.path()).unwrap();
        
        // Change some stuff so code can be imported as a module
        let contents = contents.replace("crate::", &format!("crate::{LLVM_MODULE}::"));
        let contents = contents
            .lines()
            .filter(|l| !l.starts_with("#!["))
            .collect::<Vec<_>>()
            .join("\n");
        let contents = contents.replace("#[diag(codegen_llvm", "#[diag(codegen_jvlm_codegen_llvm");
        let contents = contents.replace("#[note(codegen_llvm", "#[note(codegen_jvlm_codegen_llvm");
        let contents = contents.replace("#[help(codegen_llvm", "#[help(codegen_jvlm");
        let contents = contents.replace("fluent::codegen_llvm", "fluent::codegen_jvlm_codegen_llvm");

        // Actual modification to the meaning of the code
        let contents = contents.replace("mod errors;", "pub(crate) mod errors;");
        let contents = contents.replace("mod llvm;", "pub(crate) mod llvm;");
        let contents = contents.replace("fn write_output_file", "pub(crate) use crate::write_output_file;\nfn write_output_file_unused");

        File::create(target).unwrap().write_all(contents.as_bytes()).unwrap();
    }
}

fn find_llvm_config() -> PathBuf {
    let llvm_prefix_env = format!("LLVM_SYS_{LLVM_VERSION}_PREFIX");
    let llvm_prefix = tracked_env(&llvm_prefix_env);
    if let Some(llvm_prefix) = llvm_prefix {
        return PathBuf::from(llvm_prefix).join("bin/llvm-config");
    }

    println!("cargo::error=Could not find llvm libraries to link against. Try setting {llvm_prefix_env}");
    panic!();
}

fn tracked_env(env_name: impl AsRef<str>) -> Option<OsString> {
    println!("cargo::rerun-if-env-changed={}", env_name.as_ref());
    return env::var_os(env_name.as_ref())
}

fn download_rust_source() -> PathBuf {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    
    let mut git_target_dir = PathBuf::from(manifest_dir);
    git_target_dir.push("rust_git");


    let git = || {
        let mut c = Command::new("git");
        c.current_dir(&git_target_dir);
        return c;
    };

    if !fs::exists(&git_target_dir).unwrap() {
        println!("Initing git repo at {}", git_target_dir.display());
        Command::new("git")
            .arg("init")
            .arg(&git_target_dir)
            .status().unwrap().exit_ok().unwrap();
    }

    let commit_exist = commit_exists(git(), TARGET_RUST_COMMIT);
    if !commit_exist {
        println!("cargo::warning=cloning needed files from rust's git");

        // Let us ensure sparse checkout is fully configured
        git()
            .arg("config")
            .arg("set")
            .arg("--local")
            .arg("core.sparseCheckout").arg("true")
            .status().unwrap().exit_ok().unwrap();
        git()
            .arg("config")
            .arg("set")
            .arg("--local")
            .arg("index.sparse").arg("true")
            .status().unwrap().exit_ok().unwrap();
        File::create(git_target_dir.join(".git/info/sparse-checkout"))
            .unwrap()
            .write_all(GIT_SPARSE_FILTER.as_bytes())
            .unwrap();

        git()
            .arg("fetch")
            .arg("--filter=blob:none")
            .arg("--depth=1")
            .arg(RUST_GIT_SOURCE)
            .arg(TARGET_RUST_COMMIT)
            .status().unwrap().exit_ok().unwrap();
        if !commit_exists(git(), TARGET_RUST_COMMIT) {
            panic!("The commit we want doesn't exist. Even after running git fetch");
        }
    }

    git()
        .arg("checkout")
        .arg("--detach")
        .arg(TARGET_RUST_COMMIT)
        .status().unwrap().exit_ok().unwrap();

    return git_target_dir;
}

fn commit_exists(mut git: Command, commit: &str) -> bool {
    return git
        .arg("rev-parse")
        .arg(format!("{commit}^{{commit}}"))
        .status().unwrap().success();
}