// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! SIMD-accelerated helpers for PulseMap.
//!
//! Enabled via `--features simd`. Currently supports x86_64 (SSE2).
//!
//! Provides `match_mask_simd()` which compares all 4 H2 fingerprints
//! in parallel using SSE2 `_mm_cmpeq_epi8` + `_mm_movemask_epi8`.

#[cfg(all(target_arch = "x86_64", feature = "simd"))]
use std::arch::x86_64::*;

/// SIMD H2 fingerprint matching for MetaWord.
///
/// Extracts 4 × 7-bit H2 values from the packed u64, compares all at once
/// using SSE2, and returns a 4-bit mask of matching + Full-state slots.
#[cfg(all(target_arch = "x86_64", feature = "simd"))]
#[target_feature(enable = "sse2")]
#[inline]
pub unsafe fn match_mask_simd(meta_raw: u64, h2: u8) -> u8 {
    let h2 = h2 & 0x7F;

    // Extract 4 H2 values from bits 28-55 (7 bits each)
    let h0 = ((meta_raw >> 28) & 0x7F) as u8;
    let h1 = ((meta_raw >> 35) & 0x7F) as u8;
    let h2_2 = ((meta_raw >> 42) & 0x7F) as u8;
    let h3 = ((meta_raw >> 49) & 0x7F) as u8;

    // SIMD compare: pack 4 H2s into XMM, compare all at once
    let h2s = _mm_set_epi8(
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, h3 as i8, h2_2 as i8, h1 as i8, h0 as i8,
    );
    let target = _mm_set1_epi8(h2 as i8);
    let cmp = _mm_cmpeq_epi8(h2s, target);
    let h2_match = (_mm_movemask_epi8(cmp) & 0x0F) as u8;

    // State == Full (0b01) mask from bits 56-63
    let s0 = ((meta_raw >> 56) & 0x03) as u8;
    let s1 = ((meta_raw >> 58) & 0x03) as u8;
    let s2 = ((meta_raw >> 60) & 0x03) as u8;
    let s3 = ((meta_raw >> 62) & 0x03) as u8;

    let full_mask = (s0 == 1) as u8
        | (((s1 == 1) as u8) << 1)
        | (((s2 == 1) as u8) << 2)
        | (((s3 == 1) as u8) << 3);

    h2_match & full_mask
}
