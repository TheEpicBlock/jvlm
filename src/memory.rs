use std::io::Write;

use crate::{classfile::LVTi, FunctionTranslationContext};

pub trait MemoryStrategy {
    type MemoryInstructionEmitter: MemoryInstructionEmitter;

    /// Creates a new emitter. An emitter can only be used for a single function.
    /// Trying to use an emitter for multiple functions will lead to wrong code.
    fn emitter_for_function(&self) -> Self::MemoryInstructionEmitter;
}

/// Allows emitting memory-related java bytecode. An emitter is created using a [`MemoryStrategy`] and is bound to a specific function
pub trait MemoryInstructionEmitter: Sized {
    /// Do a stack allocation with a constant size. Which is equivalent to calling `alloca` in llvm. The instructions emitted should
    /// create a pointer to the newly allocated region.
    fn const_stack_alloc<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>, size: u64);

}

pub struct UnsafeMemorySegmentStrategy;
impl MemoryStrategy for UnsafeMemorySegmentStrategy {
    type MemoryInstructionEmitter = UnsafeMemorySegmentEmitter;

    fn emitter_for_function(&self) -> Self::MemoryInstructionEmitter {
        todo!()
    }
}

#[derive(Clone, Copy)]
struct StackPointerLocalVariables {
    /// Local variable which stores the fixed base address of the stack.
    base: LVTi,
    /// The actual stack pointer which gets incremented/decremented
    offset: LVTi,
}

pub struct UnsafeMemorySegmentEmitter {
    /// Local variables which stores the values for the stack pointer
    stack_pointer: Option<StackPointerLocalVariables>
}

impl UnsafeMemorySegmentEmitter {
    fn get_stack_pointer_local<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>) -> StackPointerLocalVariables {
        if let Some(x) = ctx.memory_allocation_info.stack_pointer {
            return x;
        } else {
            let base = ctx.get_next_slot();
            let offset = ctx.get_next_slot();
            let vars = StackPointerLocalVariables {base, offset};
            ctx.memory_allocation_info.stack_pointer.replace(vars);
            // TODO: load properly!
            ctx.java_method.emit_constant_int(0);
            ctx.java_method.emit_store(crate::classfile::JavaType::Reference, base);
            ctx.java_method.emit_constant_int(0);
            ctx.java_method.emit_store(crate::classfile::JavaType::Int, offset);
            return vars;
        }
    }
}

impl MemoryInstructionEmitter for UnsafeMemorySegmentEmitter {
    fn const_stack_alloc<W: Write>(ctx: &mut FunctionTranslationContext<'_,'_, W, Self>, size: u64) {
        let stack_pointer = Self::get_stack_pointer_local(ctx);
        ctx.java_method.emit_iinc(stack_pointer.offset, size as i16);
    }
}