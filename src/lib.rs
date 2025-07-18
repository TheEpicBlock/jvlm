use std::{collections::HashMap, io::{Seek, Write}};

use classfile::{descriptor::{DescriptorEntry, FunctionDescriptor}, ClassFileWriter, ClassMetadata, CodeLocation, InstructionTarget, JavaType, MethodMetadata, MethodWriter};
use inkwell::{basic_block::BasicBlock, llvm_sys::{self, core::LLVMGetTypeKind}, module::Module, types::{AnyType, AnyTypeEnum, AsTypeRef}, values::{AnyValue, AnyValueEnum, BasicValue, BasicValueEnum, FunctionValue, InstructionOpcode, InstructionValue, IntValue}, Either};
use options::{FunctionNameMapper, JvlmCompileOptions};
use zip::{write::SimpleFileOptions, ZipWriter};

mod classfile;
pub mod options;
pub mod linker;

pub type LlvmModule<'a> = Module<'a>;

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
                descriptor: get_descriptor(&method),
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
        translator.record_start_of_basic_block(&block);
        for instr in block.get_instructions() {
            translate(instr.as_any_value_enum(), &mut translator);
        }
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
        AnyTypeEnum::PointerType(_) => DescriptorEntry::Class("java/lang/Object".into()),
        AnyTypeEnum::StructType(_) => todo!(),
        AnyTypeEnum::VectorType(_) => todo!(),
        AnyTypeEnum::ScalableVectorType(_) => todo!(),
        AnyTypeEnum::VoidType(_) => panic!(),
    }
}

/// Allocates a slot in java's local variable table for an llvm value and emits java bytecode to store the value there.
/// If the value is of type void, only the computation is done and no slot is allocated.
fn translate<'ctx, 'class_writer, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, 'class_writer, W>) {
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

    let ty = v.get_type();
    if !matches!(ty, AnyTypeEnum::VoidType(_)) {
        let s = e.get_next_slot();
        e.java_method.emit_store(get_java_type(v.as_any_value_enum().get_type()), s);
    }
}

fn load_operand<'ctx, W: Write>(operand: Option<Either<BasicValueEnum<'ctx>, BasicBlock<'ctx>>>, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    let operand = operand.unwrap();
    match operand {
        inkwell::Either::Left(operand) => {
            load(operand.as_any_value_enum(), e);
        },
        inkwell::Either::Right(_) => todo!(),
    }
}

/// Loads a llvm ssa value onto the java stack
fn load<'ctx, 'class_writer, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, 'class_writer, W>) {    
    // It's part of the params, load it from there
    if let Some(i) = e.params.get(&v) {
        e.java_method.emit_load(get_java_type(v.get_type()), *i);
        return;
    }
    // Find out where it's stored, and load that
    if let Some(info) = e.ssa_values.get(&v) {
        e.java_method.emit_load(get_java_type(v.get_type()), info.stored_in_slot);
        return;
    }
    panic!("Trying to load uncomputed value {v}")
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

/// Translates llvm instructions. Inputs/outputs are done via the java stack
fn translate_instruction<'ctx, W: Write>(v: InstructionValue<'ctx>, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    match v.get_opcode() {
        InstructionOpcode::Add => {
            v.get_operands().for_each(|o| load_operand(o, e));
            e.java_method.emit_add(JavaType::Int); // TODO
        },
        InstructionOpcode::Mul => {
            v.get_operands().for_each(|o| load_operand(o, e));
            e.java_method.emit_mul(JavaType::Int); // TODO
        },
        InstructionOpcode::Return => {
            v.get_operands().for_each(|o| load_operand(o, e));
            e.java_method.emit_return(Some(JavaType::Int)); // TODO
        },
        InstructionOpcode::Br => {
            if v.get_num_operands() == 1 {
                // Unconditional branch
                let dest = v.get_operand(0).unwrap().right().unwrap();
                let goto_target = e.java_method.emit_goto();
                e.set_target_to_basic_block(goto_target, &dest);
            } else {
                todo!()
            }
        },
        InstructionOpcode::Select => {
            if v.get_operand(0).is_some_and(|o| o.left().is_some_and(|o| o.as_instruction_value().is_some_and(|o| o.get_opcode() == InstructionOpcode::ICmp))) {
                let icmp = v.get_operand(0).unwrap().unwrap_left().as_instruction_value().unwrap();
                icmp.get_operands().for_each(|o| load_operand(o, e));
                let predicate = icmp.get_icmp_predicate().unwrap();
                
                // TODO figure out how to deal with signed-ness
                let icmp_target = e.java_method.emit_if_icmp(match predicate {
                    inkwell::IntPredicate::EQ => classfile::ComparisonType::Equal,
                    inkwell::IntPredicate::NE => classfile::ComparisonType::NotEqual,
                    inkwell::IntPredicate::UGT => classfile::ComparisonType::GreaterThan,
                    inkwell::IntPredicate::UGE => classfile::ComparisonType::GreaterThanEqual,
                    inkwell::IntPredicate::ULT => classfile::ComparisonType::LessThan,
                    inkwell::IntPredicate::ULE => classfile::ComparisonType::LessThanEqual,
                    inkwell::IntPredicate::SGT => classfile::ComparisonType::GreaterThan,
                    inkwell::IntPredicate::SGE => classfile::ComparisonType::GreaterThanEqual,
                    inkwell::IntPredicate::SLT => classfile::ComparisonType::LessThan,
                    inkwell::IntPredicate::SLE => classfile::ComparisonType::LessThanEqual,
                });
                let pre_operand_stackmap = e.java_method.get_current_stackframe();
                // if the icmp is false, it'll execute the next instruction and compute operand one
                load_operand(v.get_operand(2), e);
                // after we've computed operand one, we skip over operand two
                let goto_target = e.java_method.emit_goto();
                // This is where we compute operand two. If out icmp is true, we should land here
                let op2 = e.java_method.current_location(); // Location where operand two is computer
                e.java_method.set_current_stackframe(pre_operand_stackmap.clone()); // We only get here via the icmp which is before any operands were pushed to the stack
                load_operand(v.get_operand(1), e);
                let post_operand_stackmap = e.java_method.get_current_stackframe();
                // This is after we compute operand two, this is where out goto should land
                let after_select = e.java_method.current_location();

                // Set the targets
                e.java_method.set_target(icmp_target, op2);
                e.java_method.set_target(goto_target, after_select);
                e.java_method.record_stackframe(op2, pre_operand_stackmap);
                e.java_method.record_stackframe(after_select, post_operand_stackmap);
            } else {
                todo!();
            }
        }
        _ => todo!("{:?}", v)
    }
}

struct FunctionTranslationContext<'ctx, 'class_writer, W: Write> {
    params: HashMap<AnyValueEnum<'ctx>, u16>,
    /// Information about the ssa values of the llvm instructions.
    /// This basically means this has information about all the results of all the instructions
    ssa_values: HashMap<AnyValueEnum<'ctx>, InstructionStatus>,
    next_slot: u16,
    java_method: MethodWriter<'class_writer, W>,
    basic_block_tracker: BasicBlockTracker<'ctx>,
}

#[derive(Default)]
struct BasicBlockTracker<'ctx> {
    /// Contains basic blocks which already have a known starting location
    already_written: HashMap<BasicBlock<'ctx>, CodeLocation>,
    /// A map of [`BasicBlock`]'s, along with [`InstructionTarget`]'s which should point to the [`BasicBlock`]'s.
    to_write: HashMap<BasicBlock<'ctx>, Vec<InstructionTarget>>,
}

impl <'ctx, W: Write>  FunctionTranslationContext<'ctx, '_, W> {
    /// Record the currect location in the [`MethodWriter`] to be the start of the given [`BasicBlock`] 
    fn record_start_of_basic_block(&mut self, block: &BasicBlock<'ctx>) {
        // We now have a resolved location for this block:
        let block_location = self.java_method.current_location();

        // This means that we can write the address for any branch instructions which still need them
        if let Some(to_write) = self.basic_block_tracker.to_write.remove(block) {
            for target in to_write {
                self.java_method.set_target(target, block_location);
            }
        }

        // Record the start of the block for future reference
        self.basic_block_tracker.already_written.insert(*block, block_location);

        // Additionally!!
        // TODO formalize the logic around local variable tables and branching
        // we explicitly record the stackframe at this point, since it's a branch target
        self.java_method.record_stackframe(block_location, self.java_method.get_current_stackframe());
    }

    /// Sets the value of a [`InstructionTarget`] to point to a [`BasicBlock`] 
    fn set_target_to_basic_block(&mut self, target_ref: InstructionTarget, target: &BasicBlock<'ctx>) {
        // We check if the basic block which we want to target has already been started. If it has
        // been started, we know its location in the bytecode and we can immediately write that
        // as the desired target.
        // If the basic block has not been started yet, we do not know which address to write here.
        // Instead, we store the InstructionTarget in a temporary list. This list will be queried later
        // inside of `start_basic_block`, where the InstructionTarget is retroactively written to with the
        // right location
        if let Some(location) = self.basic_block_tracker.already_written.get(target) {
            self.java_method.set_target(target_ref, *location);
        } else {
            self.basic_block_tracker.to_write
                .entry(*target)
                .or_insert_with(|| vec![])
                .push(target_ref);
        }
    }
}

struct InstructionStatus {
    /// Which slot in java's local variable table this is stored in
    stored_in_slot: u16
}

impl <W: Write> FunctionTranslationContext<'_, '_, W> {
    fn new<'ctx, 'class_writer>(params: Vec<BasicValueEnum<'ctx>>, method: MethodWriter<'class_writer, W>) -> FunctionTranslationContext<'ctx, 'class_writer, W> {
        let next_slot = params.len() as u16;
        return FunctionTranslationContext {
            params: params.into_iter().enumerate().map(|(a,b)| (b.as_any_value_enum(),a as u16)).collect(),
            ssa_values: HashMap::new(),
            java_method: method,
            next_slot,
            basic_block_tracker: Default::default()
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