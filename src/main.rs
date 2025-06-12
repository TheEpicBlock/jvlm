use std::{collections::HashMap, env::args, path::Path};

use inkwell::{context::Context, values::{AnyValue, AnyValueEnum, BasicValue, BasicValueEnum, InstructionOpcode, InstructionValue}};

fn main() {
    let arg = &args().collect::<Vec<_>>()[1];
    let arg = Path::new(&arg);
    let ctx = Context::create();

    println!("Reading {}", arg.display());
    let m = inkwell::module::Module::parse_bitcode_from_path(arg, &ctx).unwrap();
    for f in m.get_functions() {
        println!("Read: {:?}", f.get_name());
        for block in f.get_basic_blocks() {
            let mut translator = FunctionTranslationContext::from_params(f.get_params());
            let terminator = block.get_terminator().unwrap();
            translate_recursive(terminator.as_any_value_enum(), &mut translator);
        }
    }
}

fn translate_recursive<'ctx>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx>) {
    match v {
        AnyValueEnum::ArrayValue(array_value) => todo!(),
        AnyValueEnum::IntValue(int_value) => {
            if let Some(instr) = int_value.as_instruction() {
                translate_instruction_recursive(instr, e);
            } else {
                translate_immediately(v, e);
            }
        },
        AnyValueEnum::FloatValue(float_value) => todo!(),
        AnyValueEnum::PhiValue(phi_value) => todo!(),
        AnyValueEnum::FunctionValue(function_value) => todo!(),
        AnyValueEnum::PointerValue(pointer_value) => todo!(),
        AnyValueEnum::StructValue(struct_value) => todo!(),
        AnyValueEnum::VectorValue(vector_value) => todo!(),
        AnyValueEnum::ScalableVectorValue(scalable_vector_value) => todo!(),
        AnyValueEnum::InstructionValue(instruction_value) => translate_instruction_recursive(instruction_value, e),
        AnyValueEnum::MetadataValue(metadata_value) => todo!(),
    }
}

fn translate_instruction_recursive<'ctx>(v: InstructionValue<'ctx>, e: &mut FunctionTranslationContext<'ctx>) {
    if let Some(info) = e.instruction_status.get(&v) {
        // Instruction was already computed
        e.emit_load(info.stored_in_slot);
        return;
    }

    for operand in v.get_operands() {
        let operand = operand.unwrap();
        match operand {
            inkwell::Either::Left(operand) => {
                translate_recursive(operand.as_any_value_enum(), e);
            },
            inkwell::Either::Right(_) => todo!(),
        }
    }

    translate_immediately(v.as_any_value_enum(), e);
    // This line checks if the instruction has more than one usage
    if v.get_first_use().is_some_and(|u| u.get_next_use().is_some()) {
        let s = e.get_next_slot();
        e.emit_dup();
        e.emit_store(s);
        e.instruction_status.insert(v.clone(), InstructionStatus { stored_in_slot: s });
    }
}

fn translate_immediately(v: AnyValueEnum<'_>, e: &mut FunctionTranslationContext<'_>) {
    if let Some(i) = e.params.get(&v) {
        e.emit_load(*i);
        return;
    }
    match v {
        AnyValueEnum::ArrayValue(array_value) => todo!(),
        AnyValueEnum::IntValue(int_value) => {
            if let Some(instr) = int_value.as_instruction() {
                translate_instruction(instr, e);
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
fn translate_instruction(v: InstructionValue<'_>, e: &mut FunctionTranslationContext<'_>) {
    match v.get_opcode() {
        InstructionOpcode::Add => {
            e.emit_add();
        },
        InstructionOpcode::Mul => {
            e.emit_add();
        },
        InstructionOpcode::Return => {
            e.emit_ret();
        }
        _ => todo!()
    }
}

struct FunctionTranslationContext<'ctx> {
    params: HashMap<AnyValueEnum<'ctx>, u32>,
    instruction_status: HashMap<InstructionValue<'ctx>, InstructionStatus>,
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
            instruction_status: HashMap::new(),
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