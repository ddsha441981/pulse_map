//! Core engine — the 64-byte cache-line hash table internals.
//!
//! This module contains the fundamental building blocks:
//! - `MetaWord`: 64-bit packed metadata (state + H2 + priority)
//! - `Slot`: 14-byte entry (inline or slab pointer)
//! - `Bucket`: 64-byte cache-line-aligned container
//! - `SlabPool`: Arena allocator for variable-length entries
//! - `hash`: wyhash-based hash splitting

pub mod meta;
pub mod slot;
pub mod bucket;
pub mod slab;
pub mod hash;
