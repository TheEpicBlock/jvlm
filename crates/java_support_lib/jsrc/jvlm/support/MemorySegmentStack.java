package jvlm.support;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;

@SuppressWarnings("preview") // It's no longer preview from java 22, but if we're compiling with an older javac it might give warnings
public final class MemorySegmentStack {
    // 8 KiB is a pretty average max stack size, it should be enough.
    public final static int STACK_SIZE = 1024*8;
    // The memory segment that houses the stack needs to be aligned in order for
    // anything inside of the stack to be aligned
    public final static int STACK_ALIGNMENT = 16;

    public final static ThreadLocal<MemorySegment> STACK_BASE = ThreadLocal.withInitial(() -> {
        return Arena.global().allocate(STACK_SIZE, STACK_ALIGNMENT);
    });
    // Stack will grow downwards, so the pointer starts at `STACK_SIZE`
    public final static ThreadLocal<Integer> STACK_POINTER = ThreadLocal.withInitial(() -> STACK_SIZE);
}
