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

use crate::SlotState;

/// 64-bit metadata word containing state, fingerprints, and priority for 4 slots.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct MetaWord(u64);

impl MetaWord {
    /// Empty metadata word (all slots empty, no fingerprints, no priority).
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    // ── State (2 bits per slot, bits 56-63) ──

    #[inline]
    pub fn get_state(&self, slot: u8) -> SlotState {
        let shift = 56 + (slot as u32) * 2;
        let bits = ((self.0 >> shift) & 0x03) as u8;
        SlotState::from_bits(bits)
    }

    #[inline]
    pub fn set_state(&mut self, slot: u8, state: SlotState) {
        let shift = 56 + (slot as u32) * 2;
        self.0 &= !(0x03u64 << shift);
        self.0 |= (state as u64) << shift;
    }

    // ── H2 fingerprint (7 bits per slot, bits 28-55) ──

    #[inline]
    pub fn get_h2(&self, slot: u8) -> u8 {
        let shift = 28 + (slot as u32) * 7;
        ((self.0 >> shift) & 0x7F) as u8
    }

    #[inline]
    pub fn set_h2(&mut self, slot: u8, h2: u8) {
        let shift = 28 + (slot as u32) * 7;
        self.0 &= !(0x7Fu64 << shift);
        self.0 |= ((h2 & 0x7F) as u64) << shift;
    }

    // ── Priority (7 bits per slot, bits 0-27) ──
    // Layout: freq[6:3] (4 bits) + recency[2:0] (3 bits)

    #[inline]
    pub fn get_priority(&self, slot: u8) -> u8 {
        let shift = (slot as u32) * 7;
        ((self.0 >> shift) & 0x7F) as u8
    }

    #[inline]
    fn set_priority(&mut self, slot: u8, prio: u8) {
        let shift = (slot as u32) * 7;
        self.0 &= !(0x7Fu64 << shift);
        self.0 |= ((prio & 0x7F) as u64) << shift;
    }

    // ── Branchless scanning ──

    /// Returns a bitmask of slots where H2 matches AND state == Full.
    /// Bit i set = slot i is a potential match.
    ///
    /// Dispatches to SIMD (simd.rs) when `simd` feature is enabled on x86_64.
    #[inline]
    pub fn match_mask(&self, h2: u8) -> u8 {
        #[cfg(all(target_arch = "x86_64", feature = "simd"))]
        {
            // Safety: SSE2 is guaranteed on all x86_64 CPUs
            unsafe { crate::simd::match_mask_simd(self.0, h2) }
        }
        #[cfg(not(all(target_arch = "x86_64", feature = "simd")))]
        {
            self.match_mask_branchless(h2)
        }
    }

    /// Branchless scalar match — no if-statements, pure bit arithmetic.
    #[inline]
    #[allow(dead_code)]
    fn match_mask_branchless(&self, h2: u8) -> u8 {
        let v = self.0;
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

    /// Find a free slot (Empty or Tombstone — any non-Full state).
    #[inline]
    pub fn find_free_slot(&self) -> Option<u8> {
        for i in 0..4u8 {
            if self.get_state(i) != SlotState::Full {
                return Some(i);
            }
        }
        None
    }

    /// Find the slot with the lowest priority among Full slots (eviction target).
    #[inline]
    pub fn find_evict_target(&self) -> Option<u8> {
        let mut min_prio: u8 = 0xFF;
        let mut min_slot: Option<u8> = None;
        for i in 0..4u8 {
            if self.get_state(i) == SlotState::Full {
                let p = self.get_priority(i);
                if min_slot.is_none() || p < min_prio {
                    min_prio = p;
                    min_slot = Some(i);
                }
            }
        }
        min_slot
    }

    /// Called on access (lookup hit): boost frequency, set recency to max.
    #[inline]
    pub fn on_access(&mut self, slot: u8) {
        let prio = self.get_priority(slot);
        let freq = (prio >> 3) & 0x0F;
        let new_freq = if freq < 15 { freq + 1 } else { 15 };
        let new_prio = (new_freq << 3) | 0x07; // recency = max (7)
        self.set_priority(slot, new_prio);

        // Decay other slots' recency
        for i in 0..4u8 {
            if i != slot && self.get_state(i) == SlotState::Full {
                let p = self.get_priority(i);
                let r = p & 0x07;
                if r > 0 {
                    self.set_priority(i, (p & 0x78) | (r - 1));
                }
            }
        }
    }

    /// Called on insert: set initial priority (freq=0, recency=1).
    #[inline]
    pub fn on_insert(&mut self, slot: u8) {
        self.set_priority(slot, 0x01); // freq=0, recency=1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_roundtrip() {
        let mut m = MetaWord::empty();
        m.set_state(0, SlotState::Full);
        m.set_state(2, SlotState::Tombstone);
        assert_eq!(m.get_state(0), SlotState::Full);
        assert_eq!(m.get_state(1), SlotState::Empty);
        assert_eq!(m.get_state(2), SlotState::Tombstone);
    }

    #[test]
    fn test_h2_roundtrip() {
        let mut m = MetaWord::empty();
        m.set_h2(0, 0x55);
        m.set_h2(3, 0x7F);
        assert_eq!(m.get_h2(0), 0x55);
        assert_eq!(m.get_h2(3), 0x7F);
    }

    #[test]
    fn test_find_free_slot() {
        let mut m = MetaWord::empty();
        assert_eq!(m.find_free_slot(), Some(0));
        m.set_state(0, SlotState::Full);
        m.set_state(1, SlotState::Full);
        assert_eq!(m.find_free_slot(), Some(2));
    }

    #[test]
    fn test_evict_target() {
        let mut m = MetaWord::empty();
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
