use std::io::{Seek, Write};

use zip::{result::ZipResult, ZipWriter};

use crate::{classfile::{descriptor::{DescriptorEntry, FieldDescriptor, MethodDescriptor}, LVTi}, FunctionTranslationContext};

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

}

/// Memory allocation strategy based on [java.lang.foreign.MemorySegment](https://docs.oracle.com/en/java/javase/22/docs/api/java.base/java/lang/foreign/package-summary.html),
/// Which has been a preview api since java 19, and was stabilized in java 22.
pub struct MemorySegmentStrategy;
impl MemoryStrategy for MemorySegmentStrategy {
    type MemoryInstructionEmitter = MemorySegmentEmitter;

    fn emitter_for_function(&self) -> Self::MemoryInstructionEmitter {
        MemorySegmentEmitter {
            stack_pointer: None,
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
    stack_pointer: Option<StackPointerLocalVariables>
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
            ctx.java_method.emit_invokestatic(java_support_lib::MEMORYSEGMENTSTACK.name, "getBase", MethodDescriptor(vec![], Some(DescriptorEntry::Class("java/lang/ThreadLocal".to_owned()))));
            ctx.java_method.emit_store(crate::classfile::JavaType::Reference, base);
            ctx.java_method.emit_invokestatic(java_support_lib::MEMORYSEGMENTSTACK.name, "getOffset", MethodDescriptor(vec![], Some(DescriptorEntry::Int)));
            ctx.java_method.emit_store(crate::classfile::JavaType::Int, offset);
            return vars;
        }
    }
}

impl MemoryInstructionEmitter for MemorySegmentEmitter {
    fn const_stack_alloc<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>, size: u64) {
        let stack_pointer = Self::get_stack_pointer_local(ctx);
        ctx.java_method.emit_iinc(stack_pointer.offset, -(size as i16));
    }
}