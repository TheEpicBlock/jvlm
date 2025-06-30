#![feature(exit_status_error)]
#![feature(file_buffered)]

use std::{env, fs::{self, File}, io::{Read, Write}, path::{Path, PathBuf}, process::Command};

use toml::Table;

const RUST_GIT_SOURCE: &str = "https://github.com/rust-lang/rust.git";
const TARGET_RUST_COMMIT: &str = "60dabef95a3de3ec974dcb50926e4bfe743f078f";
const GIT_SPARSE_FILTER: &str = "/compiler/rustc_codegen_llvm/*";

fn main() {
    // Download the source
    let rust_source = download_rust_source();
    let codegen_llvm_source = rust_source.join("compiler/rustc_codegen_llvm/");
    if !fs::exists(&codegen_llvm_source).unwrap() {
        panic!("Where's rustc_codegen_llvm ??")
    }

    // Parse the dependencies
    let mut cargo_toml = String::new();
    File::open_buffered((&codegen_llvm_source).join("Cargo.toml")).unwrap().read_to_string(&mut cargo_toml).unwrap();
    let cargo_toml: Table = toml::from_str(&cargo_toml).unwrap();
    let dependencies = cargo_toml.get("dependencies").unwrap().as_table().unwrap();
    // All path dependencies are internal
    let path_dependencies: Vec<&String> = dependencies.iter()
        .filter(|(_key, v)| v.as_table().is_some_and(|table| table.contains_key("path")))
        .map(|(key, _v)| key)
        .collect();

    // Write the header
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut out = File::create(out.join("llvm_codegen_header.rs")).unwrap();
    for d in path_dependencies {
        writeln!(out, "extern crate {d};").unwrap();
    }
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