use std::{collections::HashMap, env::args, io::Write, fs::File, io::BufWriter, path::Path};

use classfile::{ClassFileWriter, ClassMetadata, JavaType, MethodMetadata, MethodWriter};
use inkwell::{basic_block::BasicBlock, context::Context, values::{AnyValue, AnyValueEnum, BasicValue, BasicValueEnum, InstructionOpcode, InstructionValue, IntValue}, Either};

mod classfile;

fn main() {
    let args = &args().collect::<Vec<_>>();
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);

    println!("Reading {}", input.display());

    let ctx = Context::create();
    let input_bitcode = inkwell::module::Module::parse_bitcode_from_path(input, &ctx).unwrap();
    
    let output = BufWriter::new(File::create(output).unwrap());
    let output_metadata = ClassMetadata {
        is_public: true,
        is_final: true,
        is_interface: false,
        is_abstract: false,
        is_synthetic: false,
        is_annotation: false,
        is_enum: false,
        is_module: false,
        this_class: "Test".to_owned(),
        super_class: "java/lang/Object".to_owned(),
    };
    let mut output_class = ClassFileWriter::write_classfile(output, output_metadata).unwrap();
    
    for f in input_bitcode.get_functions() {
        println!("Translating function named: {:?}", f.get_name());
        let method_writer = output_class.write_method(MethodMetadata {
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
            descriptor: "()V".to_owned(),
        });
        let mut translator = FunctionTranslationContext::new(f.get_params(), method_writer);
        for block in f.get_basic_blocks() {
            let terminator = block.get_terminator().unwrap();
            translate(terminator.as_any_value_enum(), &mut translator);
        }
    }
}

fn translate<'ctx, 'class_writer, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, 'class_writer, W>) {
    if let Some(info) = e.already_computed.get(&v) {
        // Instruction was already computed
        e.java_method.emit_load(get_java_type(v), info.stored_in_slot);
        return;
    }

    if let Some(i) = e.params.get(&v) {
        e.java_method.emit_load(get_java_type(v), *i);
        return;
    }
    match v {
        AnyValueEnum::ArrayValue(array_value) => todo!(),
        AnyValueEnum::IntValue(int_value) => {
            if let Some(instr) = int_value.as_instruction() {
                translate_instruction(instr, e);
            } else if int_value.is_const() {
                e.java_method.emit_constant_int(int_value.get_sign_extended_constant().unwrap() as i32);
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

/// Compute the type with which a node is stored/retrieved in the java local variable table
fn get_java_type(v: AnyValueEnum<'_>) -> JavaType {
    match v.get_type() {
        inkwell::types::AnyTypeEnum::ArrayType(_) => JavaType::Reference,
        inkwell::types::AnyTypeEnum::FloatType(_) => JavaType::Float,
        inkwell::types::AnyTypeEnum::FunctionType(_) => todo!(),
        inkwell::types::AnyTypeEnum::IntType(_) => JavaType::Int,
        inkwell::types::AnyTypeEnum::PointerType(_) => JavaType::Reference,
        inkwell::types::AnyTypeEnum::StructType(_) => JavaType::Reference,
        inkwell::types::AnyTypeEnum::VectorType(_) => todo!(),
        inkwell::types::AnyTypeEnum::ScalableVectorType(_) => todo!(),
        inkwell::types::AnyTypeEnum::VoidType(_) => todo!(),
    }
}

/// Should be called after any value was computed, to prevent things from being computed twice (with potential side-effects)
fn store_result<'ctx, W: Write>(v: impl AnyValue<'ctx> + HasUsageInfo, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    // We only need to store results if the value is used more than once
    if v.is_used_more_than_once() {
        let s = e.get_next_slot();
        e.java_method.emit_dup();
        e.java_method.emit_store(get_java_type(v.as_any_value_enum()), s);
        e.already_computed.insert(v.as_any_value_enum(), InstructionStatus { stored_in_slot: s });
    }
}

fn translate_instruction<'ctx, W: Write>(v: InstructionValue<'ctx>, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    match v.get_opcode() {
        InstructionOpcode::Add => {
            v.get_operands().for_each(|o| translate_operand(o, e));
            e.java_method.emit_add(JavaType::Int); // TODO
        },
        InstructionOpcode::Mul => {
            v.get_operands().for_each(|o| translate_operand(o, e));
            e.java_method.emit_mul(JavaType::Int); // TODO
        },
        InstructionOpcode::Return => {
            v.get_operands().for_each(|o| translate_operand(o, e));
            e.java_method.emit_return(Some(JavaType::Int)); // TODO
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

fn translate_operand<'ctx, W: Write>(operand: Option<Either<BasicValueEnum<'ctx>, BasicBlock<'ctx>>>, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    let operand = operand.unwrap();
    match operand {
        inkwell::Either::Left(operand) => {
            translate(operand.as_any_value_enum(), e);
        },
        inkwell::Either::Right(_) => todo!(),
    }
}

struct FunctionTranslationContext<'ctx, 'class_writer, W: Write> {
    params: HashMap<AnyValueEnum<'ctx>, u16>,
    already_computed: HashMap<AnyValueEnum<'ctx>, InstructionStatus>,
    next_slot: u16,
    java_method: MethodWriter<'class_writer, W>
}

struct InstructionStatus {
    stored_in_slot: u16
}

impl <W: Write> FunctionTranslationContext<'_, '_, W> {
    fn new<'ctx, 'class_writer>(params: Vec<BasicValueEnum<'ctx>>, method: MethodWriter<'class_writer, W>) -> FunctionTranslationContext<'ctx, 'class_writer, W> {
        let next_slot = params.len() as u16;
        return FunctionTranslationContext {
            params: params.into_iter().enumerate().map(|(a,b)| (b.as_any_value_enum(),a as u16)).collect(),
            already_computed: HashMap::new(),
            java_method: method,
            next_slot,
        };
    }
    fn get_next_slot(&mut self) -> u16 {
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