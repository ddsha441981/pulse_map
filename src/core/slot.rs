//! Slot — 14-byte entry (inline key+value or slab pointer).
//!
//! Layout:
//! ```text
//! Inline mode (mode bit = 0):
//!   [0]     header: [0|key_len(3 bits)|val_len(3 bits)|0]
//!   [1..6]  key (6 bytes max)
//!   [7..13] value (7 bytes max)
//!
//! Slab mode (mode bit = 1):
//!   [0]     header: [1|ext_fp_hi(7 bits)]
//!   [1..4]  ext_fp (32 bits)
//!   [5]     flags
//!   [6..13] slab_ptr (u64, 8 bytes)
//! ```

use crate::core::hash::HashResult;
use crate::core::slab::SlabEntry;

/// 14-byte slot that stores either inline key+value or a slab pointer.
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct Slot {
    pub data: [u8; 14],
}

impl Slot {
    /// Empty slot.
    #[inline]
    pub const fn empty() -> Self {
        Self { data: [0u8; 14] }
    }

    /// Clear the slot.
    #[inline]
    pub fn clear(&mut self) {
        self.data = [0u8; 14];
    }

    /// Get mode: 0 = inline, 1 = slab.
    #[inline]
    pub fn get_mode(&self) -> u8 {
        (self.data[0] >> 7) & 1
    }

    // ── Inline mode ──

    /// Set inline key and value (key ≤ 6 bytes, value ≤ 7 bytes).
    #[inline]
    pub fn set_inline(&mut self, key: &[u8], value: &[u8]) {
        debug_assert!(key.len() <= 6 && value.len() <= 7);
        self.data = [0u8; 14];
        // header: mode=0, key_len in bits 4-6, val_len in bits 1-3
        self.data[0] = ((key.len() as u8) << 4) | ((value.len() as u8 & 0x07) << 1);
        // key at bytes 1..7
        self.data[1..1 + key.len()].copy_from_slice(key);
        // value at bytes 7..14
        self.data[7..7 + value.len()].copy_from_slice(value);
    }

    /// Get inline key length.
    #[inline]
    fn inline_key_len(&self) -> usize {
        ((self.data[0] >> 4) & 0x07) as usize
    }

    /// Get inline key.
    #[inline]
    pub fn inline_key(&self) -> &[u8] {
        let len = self.inline_key_len();
        &self.data[1..1 + len]
    }

    /// Get inline value length.
    #[inline]
    fn inline_val_len(&self) -> usize {
        ((self.data[0] >> 1) & 0x07) as usize
    }

    /// Get inline value.
    #[inline]
    pub fn inline_value(&self) -> &[u8] {
        let len = self.inline_val_len();
        &self.data[7..7 + len]
    }

    // ── Slab mode ──

    /// Set slab pointer with extended fingerprint.
    #[inline]
    pub fn set_slab(&mut self, ext_fp_hi: u8, ext_fp: u32, slab_ptr: *const SlabEntry) {
        self.data = [0u8; 14];
        // header: mode=1, ext_fp_hi in bits 0-6
        self.data[0] = 0x80 | (ext_fp_hi & 0x7F);
        // ext_fp at bytes 1..5
        self.data[1..5].copy_from_slice(&ext_fp.to_le_bytes());
        // flags at byte 5
        self.data[5] = 0;
        // slab_ptr at bytes 6..14
        let ptr_bytes = (slab_ptr as u64).to_le_bytes();
        self.data[6..14].copy_from_slice(&ptr_bytes);
    }

    /// Get slab pointer.
    #[inline]
    fn slab_ptr(&self) -> *const SlabEntry {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[6..14]);
        u64::from_le_bytes(bytes) as *const SlabEntry
    }

    /// Get slab entry reference.
    #[inline]
    fn slab_entry(&self) -> &SlabEntry {
        unsafe { &*self.slab_ptr() }
    }

    /// Get ext_fp from slab slot.
    #[inline]
    fn get_ext_fp(&self) -> u32 {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&self.data[1..5]);
        u32::from_le_bytes(bytes)
    }

    /// Get ext_fp_hi from slab slot.
    #[inline]
    fn get_ext_fp_hi(&self) -> u8 {
        self.data[0] & 0x7F
    }

    /// Get the key from a slab-mode slot.
    #[inline]
    pub fn slab_key(&self) -> &[u8] {
        self.slab_entry().key()
    }

    /// Check if this slot's key matches the given key.
    #[inline]
    pub fn matches_key(&self, key: &[u8], hr: &HashResult) -> bool {
        if self.get_mode() == 0 {
            // Inline: direct comparison
            self.inline_key() == key
        } else {
            // Slab: check 46-bit fingerprint first, then full key
            if self.get_ext_fp_hi() != hr.ext_fp_hi || self.get_ext_fp() != hr.ext_fp {
                return false;
            }
            let entry = self.slab_entry();
            entry.key() == key
        }
    }

    /// Get the value from this slot.
    #[inline]
    pub fn get_value(&self, _hr: &HashResult) -> &[u8] {
        if self.get_mode() == 0 {
            self.inline_value()
        } else {
            self.slab_entry().value()
        }
    }
}
