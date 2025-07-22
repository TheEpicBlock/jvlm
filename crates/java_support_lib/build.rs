#![feature(exit_status_error)]

use std::{env, ffi::OsStr, fs::{self, File}, hash::Hasher, io::{self, BufReader, BufWriter, Write}, path::PathBuf, process::Command};

use num::{BigUint, ToPrimitive};
use sha2::Digest;
use wax::Glob;

struct JavaFile {
    /// The name of the constant in the rust code
    name: String,
    /// The name of the class (stored as a binary internal name, eg `java/lang/Thread`)
    classname: String,
    /// The pathbuf where the .class file is
    classfile_location: PathBuf,
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let jsrc = manifest_dir.join("jsrc");
    println!("cargo::rerun-if-changed={}", jsrc.display());

    let mut java_files = vec![];

    for entry in Glob::new("**/*.java").unwrap().walk(&jsrc) {
        let entry = entry.unwrap();
        let java_file = entry.path();
        let classname = java_file.with_extension("").file_name().unwrap().to_owned();
        let mut new_classname = classname.clone();

        let hash = {
            let mut hasher = sha2::Sha512::new();
            let mut input = BufReader::new(File::open(java_file).unwrap());
            io::copy(&mut input, &mut hasher).unwrap();
            hasher.finalize()
        };

        new_classname.push("_");
        new_classname.push(base62::<8>(hash));
        let java_out = out.join(&new_classname).with_extension("java");

        let classname = classname.into_string().unwrap();
        let new_classname = new_classname.into_string().unwrap();

        let rewritten_class = fs::read_to_string(java_file).unwrap()
            .replace(&format!("class {classname} {{"), &format!("class {new_classname} {{"));
        fs::write(&java_out, rewritten_class).unwrap();

        Command::new("javac")
            .arg(&java_out)
            .status().unwrap().exit_ok().unwrap();

        java_files.push(JavaFile {
            name: classname.to_uppercase(),
            classname: java_file.parent().unwrap().join(new_classname).strip_prefix(&jsrc).unwrap().to_str().unwrap().to_owned(),
            classfile_location: java_out.with_extension("class")
        });
    }

    let main_out = out.join("java_compiled.rs");
    let mut main_out = BufWriter::new(File::create(main_out).unwrap());

    for j in java_files {
        let JavaFile { name, classname, classfile_location } = j;
        writeln!(main_out, "pub const {name}: BuiltinJavaCode = BuiltinJavaCode {{ name: \"{classname}\", class: include_bytes!(\"{}\")}};", classfile_location.display()).unwrap();
    }
}

/// Creates a "base62" string of constant length. Depending on the length of the input, some information will be truncated.
/// This does not use the standard base62 alphabet. Instead it uses `a-zA-Z0-9`
fn base62<const LEN: usize>(input: impl AsRef<[u8]>) -> String {
    let mut str = String::with_capacity(LEN);
    let mut input = BigUint::from_bytes_be(input.as_ref());
    for _ in 0..LEN {
        let n = (&input % 57u8).to_u8().unwrap();
        str.push(char::from_u32((if n < 52 { (if n < 26 { b'a' } else { b'A' }) + (n % 26) } else { b'0' + (n - 52)}) as u32).unwrap());
        input /= 62u8;
    }
    return str;
}