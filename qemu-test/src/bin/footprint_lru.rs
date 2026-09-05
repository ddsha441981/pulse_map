// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Footprint probe: `lru` half of the comparison.
//!
//! `lru` is the only one of the three caches PulseMap benchmarks against that
//! also builds without `std`, so it is the whole field on bare metal. Capacity
//! 64 matches `footprint_pulse.rs` (16 buckets x 4 slots).

#![no_std]
#![no_main]

#[path = "../bump.rs"]
mod bump;

use core::num::NonZeroUsize;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use lru::LruCache;

use cortex_m as _;
use panic_semihosting as _;

#[entry]
fn main() -> ! {
    let mut cache: LruCache<u32, u32> = LruCache::new(NonZeroUsize::new(64).unwrap());
    for k in 0..256u32 {
        cache.put(k, k.wrapping_mul(3));
    }
    let mut sum = 0u32;
    for k in 0..256u32 {
        if let Some(v) = cache.get(&k) {
            sum = sum.wrapping_add(*v);
        }
    }
    hprintln!(
        "lru:       sum={} len={} heap={} allocs={}",
        sum,
        cache.len(),
        bump::bytes(),
        bump::allocs()
    );
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
