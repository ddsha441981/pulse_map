// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Footprint probe: pulse_map half of the pulse_map-vs-lru comparison.
//!
//! Identical to `footprint_lru.rs` apart from the cache type and its
//! construction. 16 buckets x 4 slots = 64 nominal entries, matching the `lru`
//! capacity so both binaries hold the same number of live entries.
//! `sum` is printed so nothing here can be optimised away.

#![no_std]
#![no_main]

#[path = "../bump.rs"]
mod bump;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use pulse_map::TypedPulseMap;

use cortex_m as _;
use panic_semihosting as _;

#[entry]
fn main() -> ! {
    let mut cache: TypedPulseMap<u32, u32> = TypedPulseMap::new(16);
    for k in 0..256u32 {
        cache.insert(k, k.wrapping_mul(3));
    }
    let mut sum = 0u32;
    for k in 0..256u32 {
        if let Some(v) = cache.get(&k) {
            sum = sum.wrapping_add(v);
        }
    }
    hprintln!(
        "pulse_map: sum={} len={} heap={} allocs={}",
        sum,
        cache.len(),
        bump::bytes(),
        bump::allocs()
    );
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
