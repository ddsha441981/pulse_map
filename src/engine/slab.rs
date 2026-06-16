// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! SlabPool — Index-based allocator with free list for variable-length key+value entries.
//!
//! Used when keys > 6 bytes or values > 7 bytes (can't fit inline in a 14-byte slot).
//!
//! v0.6.0: Changed from arena (never-free) to free-list reuse.
//! Evicted slab entries are returned to the free list and reused on next alloc.
//! This eliminates memory growth on high-churn workloads.
//!
//! Slots in `PulseMapRaw` now store a `u64` index (not a raw pointer) to identify
//! which slab entry holds their key/value data.

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
use core::alloc::Layout;

#[cfg(not(feature = "std"))]
use alloc::alloc::{alloc, dealloc, realloc};
#[cfg(feature = "std")]
use std::alloc::{alloc, dealloc, realloc};

/// A heap-allocated key+value entry for slab mode.
pub struct SlabEntry {
    key_len: u32,
    val_len: u32,
    /// Allocated size of `data` buffer (>= key_len + val_len).
    cap: u32,
    /// Points to contiguous [key_bytes | value_bytes]
    data: *mut u8,
}

impl SlabEntry {
    /// Get the key.
    #[inline]
    pub fn key(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.data, self.key_len as usize) }
    }

    /// Get the value.
    #[inline]
    pub fn value(&self) -> &[u8] {
        unsafe {
            let val_ptr = self.data.add(self.key_len as usize);
            core::slice::from_raw_parts(val_ptr, self.val_len as usize)
        }
    }

    /// Rewrite key and value in-place. Reallocates only if new data doesn't fit.
    #[inline]
    pub fn rewrite(&mut self, key: &[u8], value: &[u8]) {
        let needed = key.len() + value.len();
        if needed > self.cap as usize {
            // Need more space — reallocate
            let old_layout = Layout::from_size_align(self.cap as usize, 1).unwrap();
            let new_layout = Layout::from_size_align(needed, 1).unwrap();
            self.data = unsafe { realloc(self.data, old_layout, new_layout.size()) };
            self.cap = needed as u32;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(key.as_ptr(), self.data, key.len());
            core::ptr::copy_nonoverlapping(value.as_ptr(), self.data.add(key.len()), value.len());
        }
        self.key_len = key.len() as u32;
        self.val_len = value.len() as u32;
    }

    /// Deallocate the data buffer.
    unsafe fn dealloc_data(&mut self) {
        if self.cap > 0 {
            let layout = Layout::from_size_align(self.cap as usize, 1).unwrap();
            dealloc(self.data, layout);
            self.cap = 0;
        }
    }
}

/// Index-based slab allocator with free list for O(1) reuse.
pub struct SlabPool {
    entries: Vec<Option<Box<SlabEntry>>>,
    free_list: Vec<usize>,
}

impl SlabPool {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            free_list: Vec::new(),
        }
    }

    /// Allocate a slab entry. Returns index into `entries`.
    /// Reuses a freed slot if available (O(1)), otherwise appends (amortized O(1)).
    pub fn alloc(&mut self, key: &[u8], value: &[u8]) -> usize {
        if let Some(idx) = self.free_list.pop() {
            // Reuse freed slot — rewrite in-place, zero heap alloc if fits
            self.entries[idx].as_mut().unwrap().rewrite(key, value);
            return idx;
        }

        // New allocation
        let total_len = key.len() + value.len();
        let layout = Layout::from_size_align(total_len.max(1), 1).unwrap();
        let data = unsafe { alloc(layout) };

        unsafe {
            core::ptr::copy_nonoverlapping(key.as_ptr(), data, key.len());
            core::ptr::copy_nonoverlapping(value.as_ptr(), data.add(key.len()), value.len());
        }

        let entry = Box::new(SlabEntry {
            key_len: key.len() as u32,
            val_len: value.len() as u32,
            cap: total_len as u32,
            data,
        });

        let idx = self.entries.len();
        self.entries.push(Some(entry));
        idx
    }

    /// Return a slab entry to the free list. Entry data is retained for reuse.
    #[inline]
    pub fn free(&mut self, idx: usize) {
        // Entry stays in Vec — just mark it reusable
        self.free_list.push(idx);
    }

    /// Get a slab entry by index.
    #[inline]
    pub fn get(&self, idx: usize) -> &SlabEntry {
        // Safety: idx is always valid — only freed slots are in free_list,
        // and freed slots' data is still allocated (reused on next alloc)
        self.entries[idx].as_ref().unwrap()
    }
}

impl Drop for SlabPool {
    fn drop(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            unsafe {
                entry.dealloc_data();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slab_alloc_and_get() {
        let mut pool = SlabPool::new();
        let idx = pool.alloc(b"hello", b"world");
        assert_eq!(pool.get(idx).key(), b"hello");
        assert_eq!(pool.get(idx).value(), b"world");
    }

    #[test]
    fn test_free_list_reuse_index() {
        let mut pool = SlabPool::new();
        let idx0 = pool.alloc(b"key0", b"val0");
        let idx1 = pool.alloc(b"key1", b"val1");
        assert_eq!(pool.entries.len(), 2);

        // Free idx0 → goes to free list
        pool.free(idx0);

        // Next alloc MUST reuse idx0 — pool must not grow
        let idx2 = pool.alloc(b"key2", b"val2");
        assert_eq!(idx2, idx0, "free list must reuse idx0");
        assert_eq!(pool.entries.len(), 2, "pool must not grow on reuse");
        assert_eq!(pool.get(idx2).key(), b"key2");
        assert_eq!(pool.get(idx2).value(), b"val2");
        // idx1 untouched
        assert_eq!(pool.get(idx1).key(), b"key1");
    }

    #[test]
    fn test_free_list_larger_rewrite() {
        let mut pool = SlabPool::new();
        let idx = pool.alloc(b"k", b"v");
        pool.free(idx);
        // Realloc with larger data — realloc internally, index reused
        let idx2 = pool.alloc(b"longer_key_here", b"longer_value_here");
        assert_eq!(idx2, idx);
        assert_eq!(pool.get(idx2).key(), b"longer_key_here");
        assert_eq!(pool.get(idx2).value(), b"longer_value_here");
    }

    #[test]
    fn test_free_list_bulk_reuse() {
        let mut pool = SlabPool::new();
        let i0 = pool.alloc(b"a", b"1");
        let i1 = pool.alloc(b"b", b"2");
        let i2 = pool.alloc(b"c", b"3");
        let len_before = pool.entries.len(); // = 3

        pool.free(i0);
        pool.free(i1);
        pool.free(i2);

        // 3 reallocs — must reuse all 3, no new Vec entries
        let _ = pool.alloc(b"x", b"10");
        let _ = pool.alloc(b"y", b"20");
        let _ = pool.alloc(b"z", b"30");
        assert_eq!(pool.entries.len(), len_before, "all reallocs must reuse");
        assert_eq!(pool.free_list.len(), 0, "free list must be empty");
    }
}
