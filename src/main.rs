use std::{collections::HashMap, env::args, fs::File, io::BufWriter, path::Path};

use classfile::{ClassFileWriter, MethodMetadata};
use inkwell::{basic_block::BasicBlock, context::Context, llvm_sys::core::LLVMConstInt, values::{AnyValue, AnyValueEnum, AsValueRef, BasicValue, BasicValueEnum, InstructionOpcode, InstructionValue, IntValue}, Either};

mod classfile;

fn main() {
    let args = &args().collect::<Vec<_>>();
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);

    println!("Reading {}", input.display());

    let ctx = Context::create();
    let input_bitcode = inkwell::module::Module::parse_bitcode_from_path(input, &ctx).unwrap();
    
    let output = BufWriter::new(File::create(output).unwrap());
    let mut output_class = ClassFileWriter::write_classfile(output).unwrap();
    
    for f in input_bitcode.get_functions() {
        println!("Translating function named: {:?}", f.get_name());
        output_class.write_method(MethodMetadata {
            visibility: classfile::Visibility::PUBLIC,
            is_static: true,
            is_final: true,
            is_synchronized: false,
            is_bridge: false,
            is_varargs: false,
            is_native: false,
            is_abstract: false,
            is_strictfp: true,
            is_synthetic: false,
            name: f.get_name().to_str().unwrap().to_owned(),
            descriptor: "".to_owned(),
        });
        for block in f.get_basic_blocks() {
            let mut translator = FunctionTranslationContext::from_params(f.get_params());
            let terminator = block.get_terminator().unwrap();
            translate(terminator.as_any_value_enum(), &mut translator);
        }
    }
}

fn translate<'ctx>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx>) {
    if let Some(info) = e.already_computed.get(&v) {
        // Instruction was already computed
        e.emit_load(info.stored_in_slot);
        return;
    }

    if let Some(i) = e.params.get(&v) {
        e.emit_load(*i);
        return;
    }
    match v {
        AnyValueEnum::ArrayValue(array_value) => todo!(),
        AnyValueEnum::IntValue(int_value) => {
            if let Some(instr) = int_value.as_instruction() {
                translate_instruction(instr, e);
            } else if int_value.is_const() {
                e.emit_const(int_value.get_sign_extended_constant().unwrap());
            } else {
                dbg!(int_value);
                todo!()
            }
        },
        AnyValueEnum::FloatValue(float_value) => todo!(),
        AnyValueEnum::PhiValue(phi_value) => todo!(),
        AnyValueEnum::FunctionValue(function_value) => todo!(),
        AnyValueEnum::PointerValue(pointer_value) => todo!(),
        AnyValueEnum::StructValue(struct_value) => todo!(),
        AnyValueEnum::VectorValue(vector_value) => todo!(),
        AnyValueEnum::ScalableVectorValue(scalable_vector_value) => todo!(),
        AnyValueEnum::InstructionValue(instruction_value) => translate_instruction(instruction_value, e),
        AnyValueEnum::MetadataValue(metadata_value) => todo!(),
    }
}

/// Should be called after any value was computed, to prevent things from being computed twice (with potential side-effects)
fn store_result<'ctx>(v: impl AnyValue<'ctx> + HasUsageInfo, e: &mut FunctionTranslationContext<'ctx>) {
    // We only need to store results if the value is used more than once
    if v.is_used_more_than_once() {
        let s = e.get_next_slot();
        e.emit_dup();
        e.emit_store(s);
        e.already_computed.insert(v.as_any_value_enum(), InstructionStatus { stored_in_slot: s });
    }
}

fn translate_instruction<'ctx>(v: InstructionValue<'ctx>, e: &mut FunctionTranslationContext<'ctx>) {
    match v.get_opcode() {
        InstructionOpcode::Add => {
            v.get_operands().for_each(|o| translate_operand(o, e));
            e.emit_add();
        },
        InstructionOpcode::Mul => {
            v.get_operands().for_each(|o| translate_operand(o, e));
            e.emit_mul();
        },
        InstructionOpcode::Return => {
            v.get_operands().for_each(|o| translate_operand(o, e));
            e.emit_ret();
        },
        InstructionOpcode::Select => {
            if v.get_operand(0).is_some_and(|o| o.left().is_some_and(|o| o.as_instruction_value().is_some_and(|o| o.get_opcode() == InstructionOpcode::ICmp))) {
                let icmp = v.get_operand(0).unwrap().unwrap_left().as_instruction_value().unwrap();
                icmp.get_operands().for_each(|o| translate_operand(o, e));
                let predicate = icmp.get_icmp_predicate().unwrap();
                println!("IF_ICMP {predicate:?}");
                translate_operand(v.get_operand(1), e);
                println!("GOTO");
                translate_operand(v.get_operand(2), e);
            } else {
                todo!();
            }
        }
        _ => todo!("{:?}", v)
    }
    store_result(v, e);
}

fn translate_operand<'ctx>(operand: Option<Either<BasicValueEnum<'ctx>, BasicBlock<'ctx>>>, e: &mut FunctionTranslationContext<'ctx>) {
    let operand = operand.unwrap();
    match operand {
        inkwell::Either::Left(operand) => {
            translate(operand.as_any_value_enum(), e);
        },
        inkwell::Either::Right(_) => todo!(),
    }
}

struct FunctionTranslationContext<'ctx> {
    params: HashMap<AnyValueEnum<'ctx>, u32>,
    already_computed: HashMap<AnyValueEnum<'ctx>, InstructionStatus>,
    next_slot: u32
}

struct InstructionStatus {
    stored_in_slot: u32
}

impl FunctionTranslationContext<'_> {
    fn from_params<'ctx>(params: Vec<BasicValueEnum<'ctx>>) -> FunctionTranslationContext<'ctx> {
        let next_slot = params.len() as u32;
        return FunctionTranslationContext {
            params: params.into_iter().enumerate().map(|(a,b)| (b.as_any_value_enum(),a as u32)).collect(),
            already_computed: HashMap::new(),
            next_slot,
        };
    }
    fn emit_dup(&mut self) {
        println!("DUP");
    }
    fn emit_store(&mut self, slot: u32) {
        println!("STORE{slot}");
    }
    fn emit_load(&mut self, slot: u32) {
        println!("LOAD{slot}");
    }
    fn emit_const(&mut self, value: i64) {
        println!("LDC{value}");
    }
    fn emit_add(&mut self) {
        println!("ADD");
    }
    fn emit_mul(&mut self) {
        println!("MUL");
    }
    fn emit_ret(&mut self) {
        println!("RET");
    }
    fn get_next_slot(&mut self) -> u32 {
        let n = self.next_slot;
        self.next_slot += 1;
        return n;
    }
}


trait HasUsageInfo {
    fn is_used_more_than_once(&self) -> bool;
}

macro_rules! impl_usage {
    ($t:ty) => {
impl HasUsageInfo for $t {
    fn is_used_more_than_once(&self) -> bool {
        self.get_first_use().is_some_and(|n| n.get_next_use().is_some())
    }
}
    };
}

impl_usage!(InstructionValue<'_>);
impl_usage!(IntValue<'_>);