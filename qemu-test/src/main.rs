// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Runs PulseMap on an emulated Cortex-M — real instructions, not a `cargo check`.
//!
//! The point of this is `MetaWord`'s `AtomicU64`. On Cortex-M3 (`thumbv7m`) it
//! comes from portable-atomic's spinlock fallback; on Cortex-M0 (`thumbv6m`)
//! there is no atomic CAS in the instruction set at all, so it can only work
//! through the `critical-section` feature. `MetaWord::on_access` performs that
//! CAS on every `get` hit, so a passing `get` here is the runtime proof.
//!
//! ```text
//! cargo run --release --target thumbv7m-none-eabi                # lm3s6965evb, Cortex-M3
//! cargo run --release --target thumbv6m-none-eabi --features m0  # microbit, Cortex-M0
//! ```

#![no_std]
#![no_main]

mod bump;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use pulse_map::TypedPulseMap;

// Linked for their side effects: the panic handler, and — with `m0` — the
// critical-section impl that portable-atomic calls into.
use cortex_m as _;
use panic_semihosting as _;

// PulseMapRaw::new allocates two Vecs, not one: `buckets` at 64 B each, plus
// `slots_ttl` at 4 x sizeof(SlotTTL) = 4 x 16 B per bucket. That is **128 bytes
// per bucket**, taken upfront whether or not you ever fill it. 16 buckets is
// 2 KiB, which is what fits comfortably beside the stack in the micro:bit's
// 16 KiB of RAM.
const BUCKETS: usize = 16;
const CAPACITY: usize = BUCKETS * 4;

#[entry]
fn main() -> ! {
    let mut failed = 0u32;

    let mut map: TypedPulseMap<u32, u32> = TypedPulseMap::new(BUCKETS);
    check(&mut failed, "capacity", map.capacity() == CAPACITY);
    check(&mut failed, "starts empty", map.is_empty());

    // Each of these get hits runs the AtomicU64 CAS in MetaWord::on_access.
    map.insert(7, 700);
    check(&mut failed, "get hit", map.get(&7) == Some(700));
    check(&mut failed, "get miss", map.get(&8).is_none());
    check(&mut failed, "len after one insert", map.len() == 1);

    // 4x capacity of distinct keys, so eviction has to run. Absence is legal —
    // which of a bucket's 4 slots loses is not observable from out here — but a
    // value that was never stored under that key never is.
    let mut wrong = 0u32;
    for k in 0..(CAPACITY as u32 * 4) {
        map.insert(k, k.wrapping_mul(10));
    }
    for k in 0..(CAPACITY as u32 * 4) {
        // peek, so the check itself doesn't promote anything.
        if let Some(v) = map.peek(&k) {
            if v != k.wrapping_mul(10) {
                wrong += 1;
            }
        }
    }
    check(&mut failed, "no corrupt value after eviction", wrong == 0);
    check(
        &mut failed,
        "len stays within capacity",
        map.len() <= CAPACITY,
    );
    check(&mut failed, "eviction ran", map.eviction_count() > 0);

    // TTL counts insertions, not seconds.
    let mut ttl: TypedPulseMap<u32, u32> = TypedPulseMap::new(4);
    ttl.set_ttl(4);
    ttl.insert(1, 11);
    check(&mut failed, "fresh entry is live", ttl.get(&1) == Some(11));
    for k in 100..106 {
        ttl.insert(k, k);
    }
    check(
        &mut failed,
        "entry expired by insert count",
        ttl.get(&1).is_none(),
    );

    let mut rm: TypedPulseMap<u32, u32> = TypedPulseMap::new(4);
    rm.insert(3, 33);
    check(&mut failed, "remove reports hit", rm.remove(&3));
    check(&mut failed, "removed key is gone", rm.get(&3).is_none());

    // ── Allocator discipline ───────────────────────────────────────────────
    // "needs an allocator" and "needs an allocator at construction" are very
    // different claims to a firmware author. Count the allocations rather than
    // asserting either one.
    let before = bump::allocs();
    let mut inline: TypedPulseMap<u32, u32> = TypedPulseMap::new(8);
    let after_new = bump::allocs();
    for k in 0..256u32 {
        inline.insert(k, k);
        inline.get(&k);
        inline.remove(&(k / 2));
    }
    check(&mut failed, "construction allocates", after_new > before);
    check(
        &mut failed,
        "inline mode: zero allocations after construction",
        bump::allocs() == after_new,
    );
    hprintln!(
        "alloc: inline u32->u32 = {} at new(), {} across 256 insert+get+remove",
        after_new - before,
        bump::allocs() - after_new
    );

    // The honest counterexample: u64 exceeds the 6-byte key / 7-byte value inline
    // window, so every entry goes to the slab and heap-allocates. A static arena
    // is only sufficient for the inline case.
    let before = bump::allocs();
    let mut slab: TypedPulseMap<u64, u64> = TypedPulseMap::new(2);
    let after_new = bump::allocs();
    for k in 0..6u64 {
        slab.insert(k, k.wrapping_mul(10));
    }
    check(
        &mut failed,
        "slab mode allocates per insert",
        bump::allocs() > after_new,
    );
    hprintln!(
        "alloc: slab u64->u64   = {} at new(), {} across 6 inserts",
        after_new - before,
        bump::allocs() - after_new
    );

    hprintln!(
        "qemu-test: {} buckets, {} nominal slots, {} heap bytes used",
        BUCKETS,
        CAPACITY,
        bump::bytes()
    );

    if failed == 0 {
        hprintln!("qemu-test: all checks passed");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!("qemu-test: {} check(s) FAILED", failed);
        debug::exit(debug::EXIT_FAILURE);
    }

    // debug::exit does not return `!`; QEMU is already gone by here.
    loop {}
}

fn check(failed: &mut u32, what: &str, ok: bool) {
    if !ok {
        *failed += 1;
        hprintln!("FAIL: {}", what);
    }
}
