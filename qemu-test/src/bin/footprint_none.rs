// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Footprint baseline: no cache at all.
//!
//! cortex-m-rt + semihosting + panic handler + allocator dominate a binary this
//! small, so the interesting number is `text(footprint_pulse) - text(this)`.
//! Without this floor the comparison table measures mostly `hprintln!`.

#![no_std]
#![no_main]

// Shared with the other probes; this one only needs the byte counter.
#[allow(dead_code)]
#[path = "../bump.rs"]
mod bump;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};

use cortex_m as _;
use panic_semihosting as _;

#[entry]
fn main() -> ! {
    // Same arithmetic as the other two probes, with the cache removed.
    let mut sum = 0u32;
    for k in 0..256u32 {
        sum = sum.wrapping_add(k.wrapping_mul(3));
    }
    hprintln!("baseline: sum={} heap={}", sum, bump::bytes());
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
