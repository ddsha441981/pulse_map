// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Bucket — 64-byte cache-line-aligned structure.
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │  MetaWord (8 bytes)  │  4 × Slot (14 bytes each) │
//! │                      │  = 56 bytes                │
//! │  Total: 8 + 56 = 64 bytes = 1 cache line         │
//! └──────────────────────────────────────────────────┘
//! ```

use crate::engine::meta::MetaWord;
use crate::engine::slot::Slot;

/// A 64-byte cache-line-aligned bucket containing metadata and 4 slots.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct Bucket {
    pub meta: MetaWord,
    pub slots: [Slot; 4],
}

impl Bucket {
    /// Empty bucket.
    #[inline]
    pub const fn empty() -> Self {
        Self {
            meta: MetaWord::empty(),
            slots: [Slot::empty(); 4],
        }
    }
}

// Compile-time assertion: Bucket must be exactly 64 bytes
const _: () = assert!(core::mem::size_of::<Bucket>() == 64);
