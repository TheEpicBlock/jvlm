use std::{collections::HashMap, env::args, io::Write, fs::File, io::BufWriter, path::Path};

use classfile::{descriptor::{DescriptorEntry, FunctionDescriptor}, JavaType, MethodWriter};
use inkwell::{basic_block::BasicBlock, context::Context, llvm_sys::{self, core::LLVMGetTypeKind}, types::{AnyType, AnyTypeEnum, AsTypeRef}, values::{AnyValue, AnyValueEnum, BasicValue, BasicValueEnum, FunctionValue, InstructionOpcode, InstructionValue, IntValue}, Either};
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