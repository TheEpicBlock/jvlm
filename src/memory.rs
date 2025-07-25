use std::io::{Seek, Write};

use inkwell::types::AnyTypeEnum;
use zip::{result::ZipResult, ZipWriter};

use crate::{classfile::{descriptor::{DescriptorEntry, FieldDescriptor, MethodDescriptor}, JavaType, LVTi}, get_java_type, FunctionTranslationContext};

pub trait MemoryStrategy {
    type MemoryInstructionEmitter: MemoryInstructionEmitter;

    /// Creates a new emitter. An emitter can only be used for a single function.
    /// Trying to use an emitter for multiple functions will lead to wrong code.
    fn emitter_for_function(&self) -> Self::MemoryInstructionEmitter;

    /// Write any support classes that this memory strategy needs into the zip file
    fn append_support_classes(&self, output: &mut ZipWriter<impl Write+Seek>) -> ZipResult<()>;
}

/// Allows emitting memory-related java bytecode. An emitter is created using a [`MemoryStrategy`] and is bound to a specific function
pub trait MemoryInstructionEmitter: Sized {
    /// Do a stack allocation with a constant size. Which is equivalent to calling `alloca` in llvm. The instructions emitted should
    /// create a pointer to the newly allocated region.
    fn const_stack_alloc<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>, size: u64);
    /// Should be called whenever an external function is called
    fn pre_call<'ctx, W: Write>(ctx: &mut FunctionTranslationContext<'ctx,'_, W, Self>);
    /// Load a value from a memory location.
    /// Expects the stack to contain a pointer, will leave the stack containing the value that was loaded.
    fn load<'ctx, W: Write>(ctx: &mut FunctionTranslationContext<'ctx,'_, W, Self>, ty: AnyTypeEnum<'ctx>);
    /// Stores a value into a memory location.
    /// Expects the stack to contain a pointer. Expects the provided function to push the value that needs to be stored to the stack.
    fn store<'ctx, W: Write>(ctx: &mut FunctionTranslationContext<'ctx,'_, W, Self>, stack_pusher: impl FnOnce(&mut FunctionTranslationContext<'ctx,'_, W, Self>) -> (), ty: AnyTypeEnum<'ctx>);
}

/// Memory allocation strategy based on [java.lang.foreign.MemorySegment](https://docs.oracle.com/en/java/javase/22/docs/api/java.base/java/lang/foreign/package-summary.html),
/// Which has been a preview api since java 19, and was stabilized in java 22.
pub struct MemorySegmentStrategy;
impl MemoryStrategy for MemorySegmentStrategy {
    type MemoryInstructionEmitter = MemorySegmentEmitter;

    fn emitter_for_function(&self) -> Self::MemoryInstructionEmitter {
        MemorySegmentEmitter {
            stack_pointer: None,
            dirty: false,
        }
    }
    
    fn append_support_classes(&self, output: &mut ZipWriter<impl Write+Seek>) -> ZipResult<()> {
        java_support_lib::MEMORYSEGMENTSTACK.write_to_zip(output)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct StackPointerLocalVariables {
    /// Local variable which stores the fixed base address of the stack.
    base: LVTi,
    /// The actual stack pointer which gets incremented/decremented
    offset: LVTi,
}

pub struct MemorySegmentEmitter {
    /// Local variables which stores the values for the stack pointer
    stack_pointer: Option<StackPointerLocalVariables>,
    dirty: bool,
}

impl MemorySegmentEmitter {
    fn get_stack_pointer_local<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>) -> StackPointerLocalVariables {
        if let Some(x) = ctx.memory_allocation_info.stack_pointer {
            return x;
        } else {
            let base = ctx.get_next_slot();
            let offset = ctx.get_next_slot();
            let vars = StackPointerLocalVariables {base, offset};
            ctx.memory_allocation_info.stack_pointer.replace(vars);
            ctx.java_method.emit_invokestatic(java_support_lib::MEMORYSEGMENTSTACK.name, "getBase", MethodDescriptor(vec![], Some(DescriptorEntry::Class("java/lang/foreign/MemorySegment".to_owned()))));
            ctx.java_method.emit_store(crate::classfile::JavaType::Reference, base);
            ctx.java_method.emit_invokestatic(java_support_lib::MEMORYSEGMENTSTACK.name, "getOffset", MethodDescriptor(vec![], Some(DescriptorEntry::Int)));
            ctx.java_method.emit_store(crate::classfile::JavaType::Int, offset);
            return vars;
        }
    }
}

fn value_layout(size: u64, target_type: JavaType) -> (&'static str, DescriptorEntry, &'static str) {
    match (size, target_type) {
        (0..=8, _) => ("java/lang/foreign/ValueLayout$OfByte", DescriptorEntry::Byte, "JAVA_BYTE"),
        (9..=16, _) => ("java/lang/foreign/ValueLayout$OfShort", DescriptorEntry::Short, "JAVA_SHORT"),
        (17..=32, JavaType::Float) => ("java/lang/foreign/ValueLayout$OfFloat", DescriptorEntry::Float, "JAVA_FLOAT"),
        (17..=32, _) => ("java/lang/foreign/ValueLayout$OfInt", DescriptorEntry::Int, "JAVA_INT"),
        (33..=64, JavaType::Double) => ("java/lang/foreign/ValueLayout$OfDouble", DescriptorEntry::Double, "JAVA_DOUBLE"),
        (33..=64, _) => ("java/lang/foreign/ValueLayout$OfLong", DescriptorEntry::Long, "JAVA_LONG"),
        _ => todo!()
    }
}

impl MemoryInstructionEmitter for MemorySegmentEmitter {
    fn const_stack_alloc<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>, size: u64) {
        let stack_pointer = Self::get_stack_pointer_local(ctx);
        ctx.java_method.emit_iinc(stack_pointer.offset, -(size as i16));
        
        ctx.java_method.emit_load(crate::classfile::JavaType::Reference, stack_pointer.base);
        ctx.java_method.emit_load(crate::classfile::JavaType::Int, stack_pointer.offset);
        ctx.java_method.emit_i2l();
        ctx.java_method.emit_invokeinterface("java/lang/foreign/MemorySegment", "asSlice", MethodDescriptor(vec![DescriptorEntry::Long], Some(DescriptorEntry::Class("java/lang/foreign/MemorySegment".to_owned()))));
        ctx.memory_allocation_info.dirty = true;
    }
    
    fn pre_call<'ctx, W: Write>(ctx: &mut FunctionTranslationContext<'ctx,'_, W, Self>) {
        // Synchronize the offset pointer
        if let Some(stack_pointer) = ctx.memory_allocation_info.stack_pointer && ctx.memory_allocation_info.dirty {
            ctx.java_method.emit_load(crate::classfile::JavaType::Int, stack_pointer.offset);
            ctx.java_method.emit_invokestatic(java_support_lib::MEMORYSEGMENTSTACK.name, "setOffset", MethodDescriptor(vec![DescriptorEntry::Int], None));
        }
    }

    fn load<'ctx, W: Write>(ctx: &mut FunctionTranslationContext<'ctx,'_, W, Self>, ty: AnyTypeEnum<'ctx>) {
        let size = ctx.g.target_data.get_abi_size(&ty);
        let target_type = get_java_type(ty);
        let value_layout = value_layout(size, target_type);
        ctx.java_method.emit_getstatic("java/lang/foreign/ValueLayout", value_layout.2, FieldDescriptor::Class(value_layout.0.to_owned()));
        ctx.java_method.emit_constant_long(0);
        ctx.java_method.emit_invokeinterface("java/lang/foreign/MemorySegment", "get", MethodDescriptor(vec![DescriptorEntry::Class(value_layout.0.to_owned()), DescriptorEntry::Long], Some(value_layout.1)));
        // TODO convert to target type
    }
    
    fn store<'ctx, W: Write>(ctx: &mut FunctionTranslationContext<'ctx,'_, W, Self>, stack_pusher: impl FnOnce(&mut FunctionTranslationContext<'ctx,'_, W, Self>) -> (), ty: AnyTypeEnum<'ctx>) {
        let size = ctx.g.target_data.get_abi_size(&ty);
        let supplied_type = get_java_type(ty);
        let value_layout = value_layout(size, supplied_type);
        ctx.java_method.emit_getstatic("java/lang/foreign/ValueLayout", value_layout.2, FieldDescriptor::Class(value_layout.0.to_owned()));
        ctx.java_method.emit_constant_long(0);
        stack_pusher(ctx);
        // TODO convert from supplied type
        ctx.java_method.emit_invokeinterface("java/lang/foreign/MemorySegment", "set", MethodDescriptor(vec![DescriptorEntry::Class(value_layout.0.to_owned()), DescriptorEntry::Long, value_layout.1.into()], None));
    }
    
}