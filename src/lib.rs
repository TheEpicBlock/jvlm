use std::{collections::HashMap, io::{Seek, Write}};

use classfile::{descriptor::{DescriptorEntry, FunctionDescriptor}, ClassFileWriter, ClassMetadata, JavaType, MethodMetadata, MethodWriter};
use inkwell::{basic_block::BasicBlock, llvm_sys::{self, core::LLVMGetTypeKind}, module::Module, types::{AnyType, AnyTypeEnum, AsTypeRef}, values::{AnyValue, AnyValueEnum, BasicValue, BasicValueEnum, FunctionValue, InstructionOpcode, InstructionValue, IntValue}, Either};
use options::{FunctionNameMapper, JvlmCompileOptions};
use zip::{write::SimpleFileOptions, ZipWriter};

mod classfile;
pub mod options;

pub fn compile<FNM>(llvm_ir: Module, out: impl Write+Seek, options: JvlmCompileOptions<FNM>) where FNM: FunctionNameMapper {
    let mut out = ZipWriter::new(out);
    
    let mut methods_per_class = HashMap::<String, Vec<(String, FunctionValue)>>::default();
    for f in llvm_ir.get_functions() {
        // Determine what java name this function will get, and which classfile this function be compiled into
        let location = options.name_mapper.get_java_location(f.get_name().to_str().unwrap());
        methods_per_class
            .entry(location.class)
            .or_insert_with(|| vec![])
            .push((location.name, f));
    }

    for (class, methods) in methods_per_class {
        out.start_file(format!("{class}.class"), SimpleFileOptions::default()).unwrap();

        let class_meta = ClassMetadata {
            is_public: true,
            is_final: true,
            is_interface: false,
            is_abstract: false,
            is_synthetic: false,
            is_annotation: false,
            is_enum: false,
            is_module: false,
            this_class: class,
            super_class: "java/lang/Object".to_owned(),
        };
        let mut class_output = ClassFileWriter::write_classfile(out, class_meta).unwrap();
        for (meth_name, method) in methods {
            let method_metadata = MethodMetadata {
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
                name: meth_name,
                descriptor: get_descriptor(&method).to_string(),
            };
            let method_output = class_output.write_method(method_metadata);
            translate_method(method, method_output)
        }
        out = class_output.finalize();
    }
    out.finish().unwrap();
}

fn translate_method<W: Write>(f: FunctionValue<'_>, method_writer: MethodWriter<'_, W>) {
    let mut translator = FunctionTranslationContext::new(f.get_params(), method_writer);
    for block in f.get_basic_blocks() {
        let terminator = block.get_terminator().unwrap();
        translate(terminator.as_any_value_enum(), &mut translator);
    }
}

fn get_descriptor(function: &FunctionValue<'_>) -> FunctionDescriptor {
    let params = function.get_params().iter().map(|p| get_descriptor_entry(p.get_type().as_any_type_enum())).collect();
    let return_ty = function.get_type().get_return_type().map(|t| get_descriptor_entry(t.as_any_type_enum()));
    return FunctionDescriptor(params, return_ty);
}

fn get_descriptor_entry(v: AnyTypeEnum<'_>) -> DescriptorEntry {
    match v {
        AnyTypeEnum::ArrayType(_) => todo!(),
        AnyTypeEnum::FloatType(float_type) => match unsafe {LLVMGetTypeKind(float_type.as_type_ref())} {
            llvm_sys::LLVMTypeKind::LLVMFloatTypeKind => DescriptorEntry::Float,
            llvm_sys::LLVMTypeKind::LLVMDoubleTypeKind => DescriptorEntry::Double,
            _ => todo!("{:?}", unsafe {LLVMGetTypeKind(float_type.as_type_ref())})
        },
        AnyTypeEnum::FunctionType(_) => todo!(),
        AnyTypeEnum::IntType(int_type) => match int_type.get_bit_width() {
            1 => DescriptorEntry::Boolean,
            1..=8 => DescriptorEntry::Byte,
            9..=16 => DescriptorEntry::Short,
            17..=32 => DescriptorEntry::Int,
            33..=64 => DescriptorEntry::Long,
            _ => todo!()
        },
        AnyTypeEnum::PointerType(_) => todo!(),
        AnyTypeEnum::StructType(_) => todo!(),
        AnyTypeEnum::VectorType(_) => todo!(),
        AnyTypeEnum::ScalableVectorType(_) => todo!(),
        AnyTypeEnum::VoidType(_) => panic!(),
    }
}

fn translate<'ctx, 'class_writer, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, 'class_writer, W>) {
    if let Some(info) = e.already_computed.get(&v) {
        // Instruction was already computed
        e.java_method.emit_load(get_java_type(v.get_type()), info.stored_in_slot);
        return;
    }

    if let Some(i) = e.params.get(&v) {
        e.java_method.emit_load(get_java_type(v.get_type()), *i);
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
fn get_java_type(v: AnyTypeEnum<'_>) -> JavaType {
    match v {
        AnyTypeEnum::ArrayType(_) => JavaType::Reference,
        AnyTypeEnum::FloatType(_) => JavaType::Float,
        AnyTypeEnum::FunctionType(_) => todo!(),
        AnyTypeEnum::IntType(_) => JavaType::Int,
        AnyTypeEnum::PointerType(_) => JavaType::Reference,
        AnyTypeEnum::StructType(_) => JavaType::Reference,
        AnyTypeEnum::VectorType(_) => todo!(),
        AnyTypeEnum::ScalableVectorType(_) => todo!(),
        AnyTypeEnum::VoidType(_) => todo!(),
    }
}

/// Should be called after any value was computed, to prevent things from being computed twice (with potential side-effects)
fn store_result<'ctx, W: Write>(v: impl AnyValue<'ctx> + HasUsageInfo, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    // We only need to store results if the value is used more than once
    if v.is_used_more_than_once() {
        let s = e.get_next_slot();
        e.java_method.emit_dup();
        e.java_method.emit_store(get_java_type(v.as_any_value_enum().get_type()), s);
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