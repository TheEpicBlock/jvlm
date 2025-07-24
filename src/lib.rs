#![allow(unused_variables)]
#![allow(dead_code)]

use std::{collections::HashMap, io::{Seek, Write}, mem::ManuallyDrop};

use classfile::{descriptor::{DescriptorEntry, MethodDescriptor}, ClassFileWriter, ClassMetadata, CodeLocation, InstructionTarget, JavaType, LVTi, MethodMetadata, MethodWriter};
use inkwell::{basic_block::BasicBlock, llvm_sys::{self, core::LLVMGetTypeKind}, module::Module, targets::TargetData, types::{AnyType, AnyTypeEnum, AsTypeRef}, values::{AnyValue, AnyValueEnum, AsValueRef, BasicValue, BasicValueEnum, FunctionValue, InstructionOpcode, InstructionValue, IntValue}, Either};
use llvm_sys::{core::LLVMGetTarget, target::LLVMGetModuleDataLayout};
use memory::{MemoryInstructionEmitter, MemorySegmentStrategy, MemoryStrategy};
use options::{FunctionNameMapper, JvlmCompileOptions};
use zip::{write::SimpleFileOptions, ZipWriter};

mod memory;
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

    // SAFETY: result should not be dropped
    let target_data = unsafe { ManuallyDrop::new(TargetData::new(LLVMGetModuleDataLayout(llvm_ir.as_mut_ptr()))) };
    let global_ctx = GlobalTranslationCtx {
        target_data: target_data,
        name_mapper: Box::new(options.name_mapper),
    };
    
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
            translate_method(method, &global_ctx, method_output)
        }
        out = class_output.finalize();
    }

    // Add any global support classes which are needed for memory management
    // TODO move into options
    let memory_manager = MemorySegmentStrategy;
    memory_manager.append_support_classes(&mut out).unwrap();

    out.finish().unwrap();
}

fn translate_method<W: Write>(f: FunctionValue<'_>, global_ctx: &GlobalTranslationCtx, method_writer: MethodWriter<'_, W>) {
    let mut translator = FunctionTranslationContext::new(f.get_params(), global_ctx, method_writer);
    for block in f.get_basic_blocks() {
        translator.record_start_of_basic_block(&block);
        for instr in block.get_instructions() {
            translate_and_store(instr.as_any_value_enum(), &mut translator);
        }
    }
}

fn get_descriptor(function: &FunctionValue<'_>) -> MethodDescriptor {
    let params = function.get_params().iter().map(|p| get_descriptor_entry(p.get_type().as_any_type_enum())).collect();
    let return_ty = function.get_type().get_return_type().map(|t| get_descriptor_entry(t.as_any_type_enum()));
    return MethodDescriptor(params, return_ty);
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
fn translate_and_store<'ctx, 'class_writer, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, 'class_writer, W>) {
    dbg!(&v);
    translate(v, e);
    
    let ty = v.get_type();
    if !matches!(ty, AnyTypeEnum::VoidType(_)) {
        let s = e.get_next_slot();
        e.java_method.emit_store(get_java_type(v.as_any_value_enum().get_type()), s);
        e.ssa_values.insert(v, InstructionStatus { stored_in_slot: s });
    }
}

fn translate<'ctx, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, '_, W>) {
    match v {
        AnyValueEnum::ArrayValue(array_value) => todo!(),
        AnyValueEnum::IntValue(int_value) => {
            if let Some(instr) = int_value.as_instruction() {
                translate_instruction(instr, e);
            } else if int_value.is_const() {
                e.java_method.emit_constant_int(int_value.get_sign_extended_constant().unwrap() as i32);
            } else {
                todo!("can't handle {int_value}")
            }
        },
        AnyValueEnum::FloatValue(float_value) => todo!(),
        AnyValueEnum::PhiValue(phi_value) => todo!(),
        AnyValueEnum::FunctionValue(function_value) => todo!(),
        AnyValueEnum::PointerValue(pointer_value) => {
            if let Some(instr) = pointer_value.as_instruction() {
                translate_instruction(instr, e);
            } else {
                todo!("can't handle {pointer_value}")
            }
        },
        AnyValueEnum::StructValue(struct_value) => todo!(),
        AnyValueEnum::VectorValue(vector_value) => todo!(),
        AnyValueEnum::ScalableVectorValue(scalable_vector_value) => todo!(),
        AnyValueEnum::InstructionValue(instruction_value) => translate_instruction(instruction_value, e),
        AnyValueEnum::MetadataValue(metadata_value) => todo!(),
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
fn load<'ctx, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, '_, W>) {    
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

    // Value was not computed by an instruction. Hopefully this should only happen for constants, which
    // we don't really need to store in the local variable table anyway and we can just create them whenever needed
    translate(v, e);
}

/// Compute the type with which a node is stored/retrieved in the java local variable table
fn get_java_type(v: AnyTypeEnum<'_>) -> JavaType {
    get_java_type_or_none(v).unwrap()
}
/// Compute the type with which a node is stored/retrieved in the java local variable table
fn get_java_type_or_none(v: AnyTypeEnum<'_>) -> Option<JavaType> {
    match v {
        AnyTypeEnum::ArrayType(_) => Some(JavaType::Reference),
        AnyTypeEnum::FloatType(_) => Some(JavaType::Float),
        AnyTypeEnum::FunctionType(_) => todo!(),
        AnyTypeEnum::IntType(_) => Some(JavaType::Int),
        AnyTypeEnum::PointerType(_) => Some(JavaType::Reference),
        AnyTypeEnum::StructType(_) => Some(JavaType::Reference),
        AnyTypeEnum::VectorType(_) => todo!(),
        AnyTypeEnum::ScalableVectorType(_) => todo!(),
        AnyTypeEnum::VoidType(_) => None,
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
            let return_type = v.get_operand(0).and_then(|op| get_java_type_or_none(op.unwrap_left().get_type().as_any_type_enum())); 
            e.java_method.emit_return(return_type);
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
        InstructionOpcode::ICmp => {
            v.get_operands().for_each(|o| load_operand(o, e));
            let predicate = v.get_icmp_predicate().unwrap();
            
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
            e.java_method.emit_constant_int(0);
            // after we've computed operand one, we skip over operand two
            let goto_target = e.java_method.emit_goto();
            // This is where we compute operand two. If out icmp is true, we should land here
            let op2 = e.java_method.current_location(); // Location where operand two is computer
            e.java_method.set_current_stackframe(pre_operand_stackmap.clone()); // We only get here via the icmp which is before any operands were pushed to the stack
            e.java_method.emit_constant_int(1);
            let post_operand_stackmap = e.java_method.get_current_stackframe();
            // This is after we compute operand two, this is where out goto should land
            let after_select = e.java_method.current_location();

            // Set the targets
            e.java_method.set_target(icmp_target, op2);
            e.java_method.set_target(goto_target, after_select);
            e.java_method.record_stackframe(op2, pre_operand_stackmap);
            e.java_method.record_stackframe(after_select, post_operand_stackmap);
        }
        InstructionOpcode::Select => {
            load_operand(v.get_operand(0), e);
            let if_target = e.java_method.emit_if(classfile::ComparisonType::Equal);
            let pre_operand_stackmap = e.java_method.get_current_stackframe();
            // Didn't jump: condition is true
            load_operand(v.get_operand(1), e);
            // Jump over computation of second param
            let goto_target = e.java_method.emit_goto();
            // Now compute second param
            let op2 = e.java_method.current_location();
            e.java_method.set_current_stackframe(pre_operand_stackmap.clone()); // We only get here via the if jump which is before any operands were pushed to the stack
            load_operand(v.get_operand(2), e);
            let post_operand_stackmap = e.java_method.get_current_stackframe();
            // This is after we compute operand two, this is where the goto should land

            // Set the targets
            let after_select = e.java_method.current_location();
            e.java_method.set_target(if_target, op2);
            e.java_method.set_target(goto_target, after_select);
            e.java_method.record_stackframe(op2, pre_operand_stackmap);
            e.java_method.record_stackframe(after_select, post_operand_stackmap);
        }
        InstructionOpcode::Alloca => {
            let size = e.g.target_data.get_abi_size(&v.get_type());
            let num_elements = v.get_operand(0);
            let num_elements = num_elements.map(|n| n.expect_left("Alloca does not accept basic blocks").into_int_value());
            if num_elements.is_none() || num_elements.is_some_and(|n| n.is_constant_int()) {
                let num_elements = num_elements.map(|n| n.get_zero_extended_constant().unwrap()).unwrap_or(1);
                memory::MemorySegmentEmitter::const_stack_alloc(e, size*num_elements);
            } else {
                todo!("Dynamic alloca size not supported yet");
            }
        }
        InstructionOpcode::Store => {
            // TODO
        }
        InstructionOpcode::Load => {
            // load pointer
            load_operand(v.get_operand(0), e);
            memory::MemorySegmentEmitter::load(e, v.get_type());
        }
        InstructionOpcode::Call => {
            let func = v.get_operand(0).unwrap().unwrap_left();
            let f = func.into_pointer_value().as_any_value_enum().into_function_value();
            let loc = e.g.name_mapper.get_java_location(f.get_name().to_str().unwrap());
            e.java_method.emit_invokestatic(loc.class, loc.name, get_descriptor(&f));
        }
        _ => todo!("{v:#?}")
    }
}


struct GlobalTranslationCtx<'jvlmctx> {
    target_data: ManuallyDrop<TargetData>,
    // This should be a regular generic but I don't feel like editing ten thousand function declarations
    name_mapper: Box<dyn FunctionNameMapper + 'jvlmctx>,
}

struct FunctionTranslationContext<'ctx, 'class_writer, W: Write, M: memory::MemoryInstructionEmitter = memory::MemorySegmentEmitter> {
    g: &'class_writer GlobalTranslationCtx<'class_writer>,
    params: HashMap<AnyValueEnum<'ctx>, u16>,
    /// Information about the ssa values of the llvm instructions.
    /// This basically means this has information about all the results of all the instructions
    ssa_values: HashMap<AnyValueEnum<'ctx>, InstructionStatus>,
    next_slot: LVTi,
    java_method: MethodWriter<'class_writer, W>,
    memory_allocation_info: M,
    basic_block_tracker: BasicBlockTracker<'ctx>,
}

#[derive(Default)]
struct BasicBlockTracker<'ctx> {
    /// Contains basic blocks which already have a known starting location
    already_written: HashMap<BasicBlock<'ctx>, CodeLocation>,
    /// A map of [`BasicBlock`]'s, along with [`InstructionTarget`]'s which should point to the [`BasicBlock`]'s.
    to_write: HashMap<BasicBlock<'ctx>, Vec<InstructionTarget>>,
}

impl <'ctx, W: Write> FunctionTranslationContext<'ctx, '_, W> {
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
    fn new<'ctx, 'class_writer>(params: Vec<BasicValueEnum<'ctx>>, global_ctx: &'class_writer GlobalTranslationCtx, method: MethodWriter<'class_writer, W>) -> FunctionTranslationContext<'ctx, 'class_writer, W> {
        let strat = MemorySegmentStrategy;

        let next_slot = params.len() as LVTi;
        return FunctionTranslationContext {
            g: global_ctx,
            params: params.into_iter().enumerate().map(|(a,b)| (b.as_any_value_enum(),a as u16)).collect(),
            ssa_values: HashMap::new(),
            java_method: method,
            memory_allocation_info: strat.emitter_for_function(),
            next_slot,
            basic_block_tracker: Default::default()
        };
    }
    fn get_next_slot(&mut self) -> LVTi {
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