#![allow(dead_code)] // For some reason we must add this
use std::{env::args, fs::File, io::BufWriter, path::Path};

use inkwell::context::Context;
use jvlm::{compile, options::JvlmCompileOptions};

mod classfile;

fn main() {
    let args = &args().collect::<Vec<_>>();
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);

    println!("Reading {}", input.display());

    let ctx = Context::create();
    let input_bitcode = inkwell::module::Module::parse_bitcode_from_path(input, &ctx).unwrap();
    
    let output = BufWriter::new(File::create(output).unwrap());
    compile(input_bitcode, output, JvlmCompileOptions::default());
}