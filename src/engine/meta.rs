// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! MetaWord — 64-bit packed metadata for 4 slots.
//!
//! Layout (64 bits):
//! ```text
//! [63..56] States:    4 × 2-bit (empty/full/deleted/tombstone)
//! [55..28] H2:        4 × 7-bit fingerprints
//! [27..0]  Priorities: 4 × 7-bit (freq[4] + recency[3])
//! ```

use portable_atomic::{AtomicU64, Ordering};

use crate::SlotState;

/// 64-bit metadata word containing state, fingerprints, and priority for 4 slots.
///
/// Uses `AtomicU64` internally to support lock-free reads.
/// All getter methods use `Relaxed` atomic loads.
/// Mutating methods (`set_*`, `on_access`, `on_insert`) use atomic stores or CAS.
#[repr(transparent)]
pub struct MetaWord(AtomicU64);

impl MetaWord {
    /// Empty metadata word (all slots empty, no fingerprints, no priority).
    #[inline]
    pub const fn empty() -> Self {
        Self(AtomicU64::new(0))
    }

    /// Load the raw u64 value atomically.
    #[inline]
    fn load(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Store a raw u64 value atomically.
    #[inline]
    fn store(&self, val: u64) {
        self.0.store(val, Ordering::Relaxed);
    }

    // ── State (2 bits per slot, bits 56-63) ──

    #[inline]
    pub fn get_state(&self, slot: u8) -> SlotState {
        let shift = 56 + (slot as u32) * 2;
        let bits = ((self.load() >> shift) & 0x03) as u8;
        SlotState::from_bits(bits)
    }

    #[inline]
    pub fn set_state(&self, slot: u8, state: SlotState) {
        let shift = 56 + (slot as u32) * 2;
        let mut v = self.load();
        v &= !(0x03u64 << shift);
        v |= (state as u64) << shift;
        self.store(v);
    }

    // ── H2 fingerprint (7 bits per slot, bits 28-55) ──

    #[inline]
    pub fn get_h2(&self, slot: u8) -> u8 {
        let shift = 28 + (slot as u32) * 7;
        ((self.load() >> shift) & 0x7F) as u8
    }

    #[inline]
    pub fn set_h2(&self, slot: u8, h2: u8) {
        let shift = 28 + (slot as u32) * 7;
        let mut v = self.load();
        v &= !(0x7Fu64 << shift);
        v |= ((h2 & 0x7F) as u64) << shift;
        self.store(v);
    }

    // ── Priority (7 bits per slot, bits 0-27) ──
    // Layout: freq[6:3] (4 bits) + recency[2:0] (3 bits)

    #[inline]
    pub fn get_priority(&self, slot: u8) -> u8 {
        let shift = (slot as u32) * 7;
        ((self.load() >> shift) & 0x7F) as u8
    }

    #[inline]
    fn set_priority(&self, slot: u8, prio: u8) {
        let shift = (slot as u32) * 7;
        let mut v = self.load();
        v &= !(0x7Fu64 << shift);
        v |= ((prio & 0x7F) as u64) << shift;
        self.store(v);
    }

    // ── Branchless scanning ──

    /// Returns a bitmask of slots where H2 matches AND state == Full.
    /// Bit i set = slot i is a potential match.
    ///
    /// Dispatches to SIMD (simd.rs) when `simd` feature is enabled on x86_64.
    /// Benchmarks confirm the SSE2 path is faster than scalar in release builds.
    #[inline]
    pub fn match_mask(&self, h2: u8) -> u8 {
        let raw = self.load();
        #[cfg(all(target_arch = "x86_64", feature = "simd"))]
        {
            // Safety: SSE2 is guaranteed on all x86_64 CPUs
            unsafe { crate::simd::match_mask_simd(raw, h2) }
        }
        #[cfg(not(all(target_arch = "x86_64", feature = "simd")))]
        {
            Self::match_mask_branchless_raw(raw, h2)
        }
    }

    /// Branchless scalar match — no if-statements, pure bit arithmetic.
    /// Works on a raw u64 snapshot (no additional atomic load needed).
    #[inline]
    #[allow(dead_code)]
    fn match_mask_branchless_raw(v: u64, h2: u8) -> u8 {
        let h2 = h2 & 0x7F;
        let mut mask: u8 = 0;

        let s0 = ((v >> 56) & 0x03) as u8;
        let s1 = ((v >> 58) & 0x03) as u8;
        let s2 = ((v >> 60) & 0x03) as u8;
        let s3 = ((v >> 62) & 0x03) as u8;

        let h0 = ((v >> 28) & 0x7F) as u8;
        let h1 = ((v >> 35) & 0x7F) as u8;
        let h2_2 = ((v >> 42) & 0x7F) as u8;
        let h3 = ((v >> 49) & 0x7F) as u8;

        mask |= (s0 == 1) as u8 & (h0 == h2) as u8;
        mask |= ((s1 == 1) as u8 & (h1 == h2) as u8) << 1;
        mask |= ((s2 == 1) as u8 & (h2_2 == h2) as u8) << 2;
        mask |= ((s3 == 1) as u8 & (h3 == h2) as u8) << 3;
        mask
    }

    /// Instance method for backward compatibility with non-SIMD path.
    #[inline]
    #[allow(dead_code)]
    fn match_mask_branchless(&self, h2: u8) -> u8 {
        Self::match_mask_branchless_raw(self.load(), h2)
    }

    /// Find a free slot (Empty or Tombstone — any non-Full state).
    #[inline]
    pub fn find_free_slot(&self) -> Option<u8> {
        let v = self.load();
        for i in 0..4u8 {
            let shift = 56 + (i as u32) * 2;
            let bits = ((v >> shift) & 0x03) as u8;
            if SlotState::from_bits(bits) != SlotState::Full {
                return Some(i);
            }
        }
        None
    }

    /// Find the slot with the lowest priority among Full slots (eviction target).
    #[inline]
    pub fn find_evict_target(&self) -> Option<u8> {
        let v = self.load();
        let mut min_prio: u8 = 0xFF;
        let mut min_slot: Option<u8> = None;
        for i in 0..4u8 {
            let state_shift = 56 + (i as u32) * 2;
            let state_bits = ((v >> state_shift) & 0x03) as u8;
            if SlotState::from_bits(state_bits) == SlotState::Full {
                let prio_shift = (i as u32) * 7;
                let p = ((v >> prio_shift) & 0x7F) as u8;
                if min_slot.is_none() || p < min_prio {
                    min_prio = p;
                    min_slot = Some(i);
                }
            }
        }
        min_slot
    }

    /// Called on access (lookup hit): boost frequency, set recency to max.
    ///
    /// Uses a CAS loop so it can be called from a shared reference (`&self`),
    /// enabling lock-free priority updates from the read path.
    #[inline]
    pub fn on_access(&self, slot: u8) {
        loop {
            let current = self.load();
            let mut new_val = current;

            // Boost target slot priority
            let prio_shift = (slot as u32) * 7;
            let prio = ((new_val >> prio_shift) & 0x7F) as u8;
            let freq = (prio >> 3) & 0x0F;
            let new_freq = if freq < 15 { freq + 1 } else { 15 };
            let new_prio = (new_freq << 3) | 0x07; // recency = max (7)
            new_val &= !(0x7Fu64 << prio_shift);
            new_val |= ((new_prio & 0x7F) as u64) << prio_shift;

            // Decay other slots' recency
            for i in 0..4u8 {
                if i != slot {
                    let state_shift = 56 + (i as u32) * 2;
                    let state_bits = ((new_val >> state_shift) & 0x03) as u8;
                    if SlotState::from_bits(state_bits) == SlotState::Full {
                        let other_shift = (i as u32) * 7;
                        let p = ((new_val >> other_shift) & 0x7F) as u8;
                        let r = p & 0x07;
                        if r > 0 {
                            let decayed = (p & 0x78) | (r - 1);
                            new_val &= !(0x7Fu64 << other_shift);
                            new_val |= ((decayed & 0x7F) as u64) << other_shift;
                        }
                    }
                }
            }

            match self.0.compare_exchange_weak(
                current,
                new_val,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(_) => continue, // CAS failed, retry with fresh value
            }
        }
    }

    /// Called on insert: set initial priority (freq=0, recency=1).
    #[inline]
    pub fn on_insert(&self, slot: u8) {
        self.set_priority(slot, 0x01); // freq=0, recency=1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_roundtrip() {
        let m = MetaWord::empty();
        m.set_state(0, SlotState::Full);
        m.set_state(2, SlotState::Tombstone);
        assert_eq!(m.get_state(0), SlotState::Full);
        assert_eq!(m.get_state(1), SlotState::Empty);
        assert_eq!(m.get_state(2), SlotState::Tombstone);
    }

    #[test]
    fn test_h2_roundtrip() {
        let m = MetaWord::empty();
        m.set_h2(0, 0x55);
        m.set_h2(3, 0x7F);
        assert_eq!(m.get_h2(0), 0x55);
        assert_eq!(m.get_h2(3), 0x7F);
    }

    #[test]
    fn test_find_free_slot() {
        let m = MetaWord::empty();
        assert_eq!(m.find_free_slot(), Some(0));
        m.set_state(0, SlotState::Full);
        m.set_state(1, SlotState::Full);
        assert_eq!(m.find_free_slot(), Some(2));
    }

    #[test]
    fn test_evict_target() {
        let m = MetaWord::empty();
        for i in 0..4u8 {
            m.set_state(i, SlotState::Full);
        }
        m.set_priority(0, 100);
        m.set_priority(1, 5); // lowest
        m.set_priority(2, 50);
        m.set_priority(3, 80);
        assert_eq!(m.find_evict_target(), Some(1));
    }
}
