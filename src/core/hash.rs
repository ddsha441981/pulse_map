// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Hash computation — wyhash split into H1, H2, ext_fp_hi, ext_fp.

use std::hash::Hasher;

/// Result of hashing a key, split into components.
pub struct HashResult {
    pub h1: u64,        // Bucket index
    pub h2: u8,         // 7-bit fingerprint (for MetaWord)
    pub ext_fp_hi: u8,  // 7-bit extended fingerprint high (for slab slot header)
    pub ext_fp: u32,    // 32-bit extended fingerprint (for slab slot)
}

/// Compute hash of a key and split into H1, H2, ext_fp_hi, ext_fp.
#[inline]
pub fn compute_hash(key: &[u8]) -> HashResult {
    let mut hasher = wyhash::WyHash::with_seed(0);
    hasher.write(key);
    let full = hasher.finish();

    HashResult {
        h1: full,
        h2: ((full >> 57) & 0x7F) as u8,
        ext_fp_hi: ((full >> 50) & 0x7F) as u8,
        ext_fp: ((full >> 18) & 0xFFFFFFFF) as u32,
    }
}
