// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! SlabPool — Arena allocator for variable-length key+value entries.
//!
//! Used when keys > 6 bytes or values > 7 bytes (can't fit inline in a 14-byte slot).
//! Arena allocation is fast: bump pointer, no individual free.

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;
use core::alloc::Layout;

// alloc/dealloc from the global allocator
#[cfg(not(feature = "std"))]
use alloc::alloc::{alloc, dealloc};
#[cfg(feature = "std")]
use std::alloc::{alloc, dealloc};

/// A heap-allocated key+value entry for slab mode.
pub struct SlabEntry {
    key_len: u32,
    val_len: u32,
    /// Points to contiguous [key_bytes | value_bytes]
    data: *const u8,
    layout: Layout,
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
}

/// Arena-based allocator for slab entries.
#[allow(clippy::vec_box)] // Box needed: pointer stability for *const SlabEntry
pub struct SlabPool {
    entries: Vec<Box<SlabEntry>>,
}

impl SlabPool {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Allocate a new slab entry with the given key and value.
    /// Returns a raw pointer (stable because Box won't move the SlabEntry).
    pub fn alloc(&mut self, key: &[u8], value: &[u8]) -> *const SlabEntry {
        let total_len = key.len() + value.len();
        let layout = Layout::from_size_align(total_len, 1).unwrap();
        let data = unsafe { alloc(layout) };

        unsafe {
            core::ptr::copy_nonoverlapping(key.as_ptr(), data, key.len());
            core::ptr::copy_nonoverlapping(value.as_ptr(), data.add(key.len()), value.len());
        }

        let entry = Box::new(SlabEntry {
            key_len: key.len() as u32,
            val_len: value.len() as u32,
            data,
            layout,
        });

        let ptr: *const SlabEntry = &*entry;
        self.entries.push(entry);
        ptr
    }
}

impl Drop for SlabPool {
    fn drop(&mut self) {
        for entry in &self.entries {
            unsafe {
                dealloc(entry.data as *mut u8, entry.layout);
            }
        }
    }
}
