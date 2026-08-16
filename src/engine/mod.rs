// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Core engine — the 64-byte cache-line hash table internals.
//!
//! This module contains the fundamental building blocks:
//! - `MetaWord`: 64-bit packed metadata (state + H2 + priority)
//! - `Slot`: 14-byte entry (inline or slab pointer)
//! - `Bucket`: 64-byte cache-line-aligned container
//! - `SlabPool`: Arena allocator for variable-length entries
//! - `hash`: wyhash-based hash splitting

#[cfg(feature = "std")]
pub mod access_buffer;
pub mod bucket;
pub mod hash;
pub mod meta;
pub mod slab;
pub mod slot;
