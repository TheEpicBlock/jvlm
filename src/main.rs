use std::{env::args, path::Path};

use inkwell::{context::Context, values::AnyValue};

fn main() {
    let arg = &args().collect::<Vec<_>>()[1];
    let arg = Path::new(&arg);
    let ctx = Context::create();

    println!("Reading {}", arg.display());
    let m = inkwell::module::Module::parse_bitcode_from_path(arg, &ctx).unwrap();
    for f in m.get_functions() {
        println!("Read: {:?}", f.get_name());
        for block in f.get_basic_blocks() {
            for instr in block.get_instructions() {
                println!("{:?}: {}", instr.get_opcode(), instr.get_num_operands());
                for op in instr.get_operands().filter_map(|e| e) {
                    match op {
                        inkwell::Either::Left(x) => {
                            println!("{:?}",x.as_any_value_enum());
                            println!("BVE {:?}, {:?}", x.get_name(), x.get_type())
                        },
                        inkwell::Either::Right(x) => {
                            println!("BB {:?}", x.get_name())
                        },
                    }
                }
            }
        }
    }
}

struct JvmBytecodeEmitter;

impl JvmBytecodeEmitter {
    
}