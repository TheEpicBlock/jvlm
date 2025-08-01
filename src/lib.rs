#![allow(unused_variables)]
#![allow(dead_code)]

use std::{collections::{HashMap, HashSet}, ffi::CString, io::{Seek, Write}, mem::ManuallyDrop};

use classfile::{descriptor::{DescriptorEntry, MethodDescriptor}, ClassFileWriter, ClassMetadata, CodeLocation, FieldData, InstructionTarget, JavaType, LVTi, MethodMetadata, MethodWriter};
use cstr_ops::CStrExt;
use inkwell::{basic_block::BasicBlock, llvm_sys::{self, core::LLVMGetTypeKind}, module::Module, targets::TargetData, types::{AnyType, AnyTypeEnum, AsTypeRef, IntType, PointerType}, values::{AggregateValue, AnyValue, AnyValueEnum, AsValueRef, BasicValue, BasicValueEnum, FunctionValue, GlobalValue, InstructionOpcode, InstructionValue, IntValue, StructValue}, AddressSpace, Either};
use llvm_intrinsics::get_instrinsic_handler;
use llvm_sys::{core::{LLVMGetAggregateElement, LLVMGetPointerAddressSpace, LLVMIsAConstantArray, LLVMIsAGlobalValue, LLVMTypeOf}, debuginfo::LLVMDITypeGetFlags, target::LLVMGetModuleDataLayout};
use memory::{MemoryInstructionEmitter, MemorySegmentStrategy, MemoryStrategy};
use options::{ExtraTypeInfo, FunctionNameMapper, FunctionType, JvlmCompileOptions};
use zip::{write::SimpleFileOptions, DateTime, ZipWriter};

mod memory;
mod classfile;
mod llvm_intrinsics;
pub(crate) mod java_types;
pub mod options;
pub mod linker;

pub type LlvmModule<'a> = Module<'a>;

#[allow(non_snake_case)] // It's a constant, we just can't write it because .into isn't constant compatible
pub fn JAVA_OBJECT_ADDRESS_SPACE() -> AddressSpace {
    return 1.into();
}

pub fn compile<FNM>(llvm_ir: Module, out: impl Write+Seek, options: JvlmCompileOptions<FNM>) where FNM: FunctionNameMapper {
    // Read llvm annotations
    // these are strings which can be attached to pretty much any llvm type. Handy for implementing target-specific stuff
    let mut parsed_annotations = HashMap::new();
    let annotation = llvm_ir.get_global("llvm.global.annotations");
    if let Some(annotation) = annotation {
        let annotation = annotation.get_initializer().unwrap().into_array_value();
        for i in 0..(annotation.get_type().len()) {
            let annotation_struct = unsafe { AnyValueEnum::new(LLVMGetAggregateElement(annotation.as_value_ref(), i))}.into_struct_value();
            let annotation_target = unsafe { AnyValueEnum::new(LLVMGetAggregateElement(annotation_struct.as_value_ref(), 0))}.into_pointer_value();
            let annotation_content = unsafe { GlobalValue::new(LLVMGetAggregateElement(annotation_struct.as_value_ref(), 1))};
            let annotation_content = CString::from_vec_with_nul(annotation_content.get_initializer().unwrap().into_array_value().as_const_string().unwrap().into()).unwrap().into_string().unwrap();
            parsed_annotations.insert(annotation_target, annotation_content);
        }
    }

    let mut out = ZipWriter::new(out);
    
    // First, we must determine which functions/globals are going to go into which java classes.
    // To do this, we create a plan for each class
    let mut class_plans = HashMap::<String, ClassPlan>::default();

    for f in llvm_ir.get_functions() {
        // Don't generate code for functions that don't have code (external function declarations)
        if f.get_basic_blocks().len() == 0 { continue; }
        // Determine what java name this function will get, and which classfile this function be compiled into
        let location = options.name_mapper.get_java_location(f.get_name().to_str().unwrap());
        if location.external {
            panic!("Function {} should not be defined!", f.get_name().to_str().unwrap());
        } else if location.ty != FunctionType::Static {
            panic!("Can not yet emit non-static functions");
        }
        class_plans
            .entry(location.class)
            .or_insert_with(|| ClassPlan::default())
            .methods.push((location.name, location.extra_type_info, f));
    }

    for global in llvm_ir.get_globals() {
        // Check annotations
        let mut no_field = false;
        if let Some(annotation) = parsed_annotations.get(&global.as_pointer_value()) {
            // TODO allow including as resource without specifying the location
            if let Some(target) = annotation.strip_prefix("jvlm::include_as_resource(") {
                // The user specified this global should be included as a file inside of the jar. Lets do so
                let target = target.strip_suffix(")").expect("Expected closing bracket after jvlm::include_as_resource");
                out.start_file(target, SimpleFileOptions::default().last_modified_time(DateTime::default())).unwrap();
                out.write_all(global.get_initializer().unwrap().into_array_value().as_const_string().unwrap()).unwrap();
                no_field = true; // Don't generate a field for this one
            }
        }
        if !global.is_declaration() && !global.get_section().is_some_and(|name| name.equals(b"llvm.metadata")) && !no_field {
            // We're the ones defining this global, so we need to generate a field in the class
            let location = options.name_mapper.get_static_field_location(global.get_name().to_str().unwrap());
            class_plans
                .entry(location.class)
                .or_insert_with(|| ClassPlan::default())
                .fields.push((location.name, location.extra_type_info, global));
        }
    }

    // Now we're ready to start generating these classes

    // SAFETY: result should not be dropped
    let target_data = unsafe { ManuallyDrop::new(TargetData::new(LLVMGetModuleDataLayout(llvm_ir.as_mut_ptr()))) };
    let global_ctx = GlobalTranslationCtx {
        target_data: target_data,
        name_mapper: Box::new(options.name_mapper),
    };

    for (class, plan) in class_plans {
        out.start_file(format!("{class}.class"), SimpleFileOptions::default().last_modified_time(DateTime::default())).unwrap();

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
        for (meth_name, type_info, method) in plan.methods {
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
                descriptor: get_descriptor(&method, type_info, false),
            };
            let method_output = class_output.write_method(method_metadata);
            translate_method(method, &global_ctx, method_output)
        }
        for (field_name, mut type_info, global) in plan.fields {
            if type_info.is_none() { continue; } // TODO hack due to not parsing address space correctly
            // TODO check if the global has an initializer
            class_output.write_field(FieldData {
                is_public: true,
                is_private: false,
                is_protected: false,
                is_static: true,
                is_final: false,
                is_volatile: false,
                is_transient: false,
                is_enum: false,
                is_synthetic: false,
                name: field_name,
                // TODO it seems like globals don't parse their address space correctly?
                descriptor: DescriptorEntry::Class(type_info.unwrap()),
                // descriptor: get_descriptor_entry(global.as_pointer_value().get_type().as_any_type_enum(), &mut || type_info.take().unwrap()),
            });
        }
        out = class_output.finalize();
    }

    // Add any global support classes which are needed for memory management
    // TODO move into options
    let memory_manager = MemorySegmentStrategy;
    memory_manager.append_support_classes(&mut out).unwrap();

    out.finish().unwrap();
}

/// A skeleton of a yet-to-be-generated class
#[derive(Default)]
struct ClassPlan<'a> {
    pub methods: Vec<(String, ExtraTypeInfo, FunctionValue<'a>)>,
    pub fields: Vec<(String, Option<String>, GlobalValue<'a>)>,
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

fn get_descriptor(function: &FunctionValue<'_>, extra_info: ExtraTypeInfo, nothis: bool) -> MethodDescriptor {
    let mut extra_info_iter = extra_info.map(|i| i.into_iter());
    let mut extra_info_getter = || (&mut extra_info_iter).as_mut().unwrap().next().unwrap();
    let params = function.get_params().iter().skip(if nothis {1} else {0}).map(|p| get_descriptor_entry(p.get_type().as_any_type_enum(), &mut extra_info_getter)).collect();
    let return_ty = function.get_type().get_return_type().map(|t| get_descriptor_entry(t.as_any_type_enum(), &mut extra_info_getter));
    return MethodDescriptor(params, return_ty);
}

fn get_descriptor_entry(v: AnyTypeEnum<'_>, get_extra_info: &mut impl FnMut() -> String) -> DescriptorEntry {
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
        AnyTypeEnum::PointerType(pty) => {
            if pty.get_address_space() == JAVA_OBJECT_ADDRESS_SPACE() {
                return DescriptorEntry::Class(get_extra_info());
            }
            // TODO should prolly depend on the memory allocator
            DescriptorEntry::Class("java/lang/Object".into())
        },
        AnyTypeEnum::StructType(_) => todo!(),
        AnyTypeEnum::VectorType(_) => todo!(),
        AnyTypeEnum::ScalableVectorType(_) => todo!(),
        AnyTypeEnum::VoidType(_) => panic!(),
    }
}

/// Allocates a slot in java's local variable table for an llvm value and emits java bytecode to store the value there.
/// If the value is of type void, only the computation is done and no slot is allocated.
fn translate_and_store<'ctx, 'class_writer, W: Write>(v: AnyValueEnum<'ctx>, e: &mut FunctionTranslationContext<'ctx, 'class_writer, W>) {
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
                match get_int_representation(int_value.get_type()) {
                    IntRepresentation::JavaInt => e.java_method.emit_constant_int(int_value.get_sign_extended_constant().unwrap() as i32),
                    IntRepresentation::JavaLong => e.java_method.emit_constant_long(int_value.get_sign_extended_constant().unwrap() as i64),
                    IntRepresentation::BigInteger => todo!(),
                }
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
                if pointer_value.is_null() {
                    e.java_method.emit_constant_null();
                } else {
                    todo!("can't handle {pointer_value}")
                }
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
        AnyTypeEnum::IntType(ty) => {
            match get_int_representation(ty) {
                IntRepresentation::JavaInt => Some(JavaType::Int),
                IntRepresentation::JavaLong => Some(JavaType::Long),
                IntRepresentation::BigInteger => Some(JavaType::Reference),
            }
        },
        AnyTypeEnum::PointerType(_) => Some(JavaType::Reference),
        AnyTypeEnum::StructType(_) => Some(JavaType::Reference),
        AnyTypeEnum::VectorType(_) => todo!(),
        AnyTypeEnum::ScalableVectorType(_) => todo!(),
        AnyTypeEnum::VoidType(_) => None,
    }
}

/// Gets the correct representation for an llvm integer type in java
fn get_int_representation(ty: IntType) -> IntRepresentation {
    match ty.get_bit_width() {
        0..=32 => IntRepresentation::JavaInt,
        33..=64 => IntRepresentation::JavaLong,
        _ => IntRepresentation::BigInteger,
    }
}

pub enum IntRepresentation {
    /// The number is represented using a java integer
    JavaInt,
    /// The number is represented using a java long
    JavaLong,
    /// The number is represented using a java BigInteger
    BigInteger,
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

            let return_type = v.get_operand(0).and_then(|op| get_java_type_or_none(op.unwrap_left().get_type().as_any_type_enum())); 
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
            let ptr = v.get_operand(1).unwrap().unwrap_left().into_pointer_value();
            let ty = v.get_operand(0).unwrap().unwrap_left().get_type();
            if ptr.is_const() && ty.is_pointer_type() && ty.into_pointer_type().get_address_space() == JAVA_OBJECT_ADDRESS_SPACE() {
                let name = ptr.get_name().to_str().unwrap();
                let mut loc = e.g.name_mapper.get_static_field_location(name);
                // load value
                load_operand(v.get_operand(0), e);
                e.java_method.emit_putstatic(loc.class, loc.name, get_descriptor_entry(ty.as_any_type_enum(), &mut || loc.extra_type_info.take().unwrap()));
            } else {
                // load pointer
                load_operand(v.get_operand(1), e);
                memory::MemorySegmentEmitter::store(e, |e| {
                    // load value
                    load_operand(v.get_operand(0), e);
                }, v.get_operand(0).unwrap().unwrap_left().get_type().as_any_type_enum());
            }
        }
        InstructionOpcode::Load => {
            let ptr = v.get_operand(0).unwrap().unwrap_left().into_pointer_value();
            if ptr.is_const() && v.get_type().into_pointer_type().get_address_space() == JAVA_OBJECT_ADDRESS_SPACE() {
                let name = ptr.get_name().to_str().unwrap();
                let mut loc = e.g.name_mapper.get_static_field_location(name);
                e.java_method.emit_getstatic(loc.class, loc.name, get_descriptor_entry(v.get_type(), &mut || loc.extra_type_info.take().unwrap()));
            } else {
                // load pointer
                load_operand(v.get_operand(0), e);
                memory::MemorySegmentEmitter::load(e, v.get_type());
            }
        }
        InstructionOpcode::Call => {
            // For some reason the function pointer is the last argument
            let func = v.get_operand(v.get_num_operands() - 1).unwrap().unwrap_left();
            let f = func.into_pointer_value().as_any_value_enum().into_function_value();
            if let Some(handler) = get_instrinsic_handler(f.get_name()) {
                handler();
            } else {
                let name = f.get_name().to_str().unwrap();
                // Test for special syntax to use `new`
                if let Some(target) = e.g.name_mapper.is_special_new_function(name) {
                    e.java_method.emit_new(target);
                } else {
                    let loc = e.g.name_mapper.get_java_location(name);
                    // If the java code is guaranteed never to enter back into our code we can technically omit this
                    memory::MemorySegmentEmitter::pre_call(e);
                    // Load operands
                    v.get_operands().take((v.get_num_operands()-1) as usize).for_each(|o| load_operand(o, e));
                    match loc.ty {
                        FunctionType::Special => e.java_method.emit_invokespecial(loc.class, loc.name, get_descriptor(&f, loc.extra_type_info, true)),
                        FunctionType::Virtual => e.java_method.emit_invokevirtual(loc.class, loc.name, get_descriptor(&f, loc.extra_type_info, true)),
                        FunctionType::Interface => e.java_method.emit_invokeinterface(loc.class, loc.name, get_descriptor(&f, loc.extra_type_info, true)),
                        FunctionType::Static => e.java_method.emit_invokestatic(loc.class, loc.name, get_descriptor(&f, loc.extra_type_info, false)),
                        FunctionType::StaticInterface => e.java_method.emit_invokestatic_on_interface(loc.class, loc.name, get_descriptor(&f, loc.extra_type_info, false)),
                    }
                }
            }
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