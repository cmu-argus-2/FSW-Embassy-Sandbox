//! RAM usage: static RAM, and the stack's high-water mark by painting.
//!
//! [`paint`] fills the unused part of the stack with a known word. As the program runs, calls and
//! interrupts overwrite it, so the lowest address that no longer holds the paint is the deepest
//! the stack has reached: [`peak`]. Both executors run on this one stack (the interrupt executor
//! runs from an interrupt, on the main stack), so the peak covers every task.
//!
//! Total RAM in use is [`static_ram`] + [`peak`]; there is no heap.
//!
//! cortex-m-rt's `paint-stack` feature cannot be used instead: `memory.x` places sections after
//! `.bss` in flash, which moves its `_stack_end` bound into flash, so it would paint flash and
//! peripherals.

use core::ptr::{addr_of, read_volatile, write_volatile};

/// Word written over the unused stack.
const PAINT: u32 = 0xCCCC_CCCC;

/// Left unpainted just below the stack pointer, for [`paint`]'s own calls (larger in debug builds).
const MARGIN: usize = 1024;

unsafe extern "C" {
    /// Start of static RAM (`.data`), from the cortex-m-rt linker script.
    static __sdata: u32;
    /// End of static RAM (after `.data`, `.bss` and `.uninit`): the lowest the stack can grow to.
    static __euninit: u32;
    /// Top of the stack (end of RAM).
    static _stack_start: u32;
}

fn ram_start() -> usize {
    addr_of!(__sdata) as usize
}

/// Bottom of the stack region.
fn bottom() -> usize {
    addr_of!(__euninit) as usize
}

/// Top of the stack region.
fn top() -> usize {
    addr_of!(_stack_start) as usize
}

/// Bytes of static RAM: `.data` + `.bss` + `.uninit`, which includes every task's storage.
pub fn static_ram() -> usize {
    bottom() - ram_start()
}

/// Bytes available to the stack: from the end of static RAM to the top of RAM.
pub fn stack_size() -> usize {
    top() - bottom()
}

/// Bytes of RAM in total: static RAM plus the stack region.
pub fn ram_size() -> usize {
    top() - ram_start()
}

/// `bytes` as a percentage of [`ram_size`].
pub fn percent_of_ram(bytes: usize) -> f32 {
    bytes as f32 * 100.0 / ram_size() as f32
}

/// Paint the stack from the end of static RAM up to just below the current stack pointer. Call
/// once, first thing in `main`. Stack used before the call stays counted as used.
pub fn paint() {
    let sp = cortex_m::register::msp::read() as usize;
    let end = (sp - MARGIN) & !3;
    let mut p = bottom();
    while p < end {
        // Below the stack pointer, so nothing live is there. An interrupt taken meanwhile pushes
        // its frame below the stack pointer too, but has returned before the loop continues.
        unsafe { write_volatile(p as *mut u32, PAINT) };
        p += 4;
    }
}

/// Deepest stack use since [`paint`], in bytes. Scans up from the bottom of the stack through all
/// the unused stack (most of RAM), so call it occasionally, not every cycle. Only meaningful after
/// [`paint`].
pub fn peak() -> usize {
    let mut p = bottom();
    while p < top() && unsafe { read_volatile(p as *const u32) } == PAINT {
        p += 4;
    }
    top() - p
}
