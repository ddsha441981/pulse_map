// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Bump allocator that counts what it hands out.
//!
//! The counting is the point. "pulse_map needs an allocator" is a dealbreaker
//! for firmware that deliberately has none; "pulse_map needs an allocator at
//! construction" is a line in `main`. Which one is true is measurable, so
//! `main.rs` measures it instead of asserting either.

use core::alloc::{GlobalAlloc, Layout};

pub const HEAP_SIZE: usize = 8 * 1024;

static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
// Plain `usize`, not atomics: single-threaded with interrupts never enabled, and
// Cortex-M0 has no atomic CAS to use here anyway.
static mut NEXT: usize = 0;
static mut ALLOCS: usize = 0;

/// ponytail: bump allocator, `dealloc` is a no-op. Correct for a run-once test
/// that allocates at startup and exits; real firmware wants `embedded-alloc`.
pub struct Bump;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = core::ptr::addr_of_mut!(HEAP) as usize;
        let aligned = (base + NEXT + layout.align() - 1) & !(layout.align() - 1);
        let end = aligned + layout.size();
        if end > base + HEAP_SIZE {
            return core::ptr::null_mut();
        }
        NEXT = end - base;
        ALLOCS += 1;
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: Bump = Bump;

/// Number of `alloc` calls served so far.
pub fn allocs() -> usize {
    unsafe { ALLOCS }
}

/// Bytes handed out so far. Never decreases — `dealloc` is a no-op.
pub fn bytes() -> usize {
    unsafe { NEXT }
}
