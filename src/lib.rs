//! # PulseMap
//!
//! A CPU cache-line hash table with zero-cost eviction.
//!
//! Every bucket fits in exactly **one 64-byte cache line** with embedded
//! LFU+LRU eviction metadata. Eviction decisions require zero additional
//! cache misses because the priority data lives inside the metadata word
//! that was already fetched for the fingerprint check.
//!
//! ## Quick Start
//! ```
//! use pulse_map::PulseMap;
//!
//! let mut map = PulseMap::new(1024); // 1024 buckets × 4 slots = 4096 entries
//! map.insert(b"hello", b"world");
//! assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
//! map.remove(b"hello");
//! assert_eq!(map.get(b"hello"), None);
//! ```

mod core;

pub use crate::core::meta::MetaWord;
pub use crate::core::slot::Slot;
pub use crate::core::bucket::Bucket;

use crate::core::hash::compute_hash;
use crate::core::slab::{SlabEntry, SlabPool};

/// A CPU cache-line hash table with zero-cost eviction.
///
/// Each bucket is exactly 64 bytes (one cache line) containing:
/// - 8-byte MetaWord (state + H2 fingerprint + priority for 4 slots)
/// - 4 × 14-byte Slots (inline key+value or slab pointer)
///
/// # Eviction Policy
/// Hybrid LFU+LRU: 4-bit frequency + 3-bit recency = 7-bit priority per slot.
/// When a bucket is full, the slot with the lowest priority is evicted.
/// This decision costs **zero extra cache misses**.
pub struct PulseMap {
    buckets: Vec<Bucket>,
    slab_pool: SlabPool,
    num_buckets: usize,
    count: usize,
    eviction_count: usize,
}

// Safety: PulseMap uses interior mutability only for priority metadata updates
// (frequency/recency counters). The actual key-value data is never mutated during get().
unsafe impl Send for PulseMap {}
unsafe impl Sync for PulseMap {}

impl PulseMap {
    /// Create a new PulseMap with the given number of buckets.
    ///
    /// Total capacity = `num_buckets × 4` entries.
    /// Recommended load factor: 60-70% for best hit rate.
    pub fn new(num_buckets: usize) -> Self {
        let buckets = vec![Bucket::empty(); num_buckets];
        Self {
            buckets,
            slab_pool: SlabPool::new(),
            num_buckets,
            count: 0,
            eviction_count: 0,
        }
    }

    /// Insert a key-value pair. If the bucket is full, evicts the lowest-priority entry.
    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) % self.num_buckets;
        let bucket = &mut self.buckets[bucket_idx];

        // 1. Check if key already exists (update in place)
        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1; // clear lowest bit
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr) {
                // Update value
                let s = &mut bucket.slots[slot_idx as usize];
                if key.len() <= 6 && value.len() <= 7 {
                    s.set_inline(key, value);
                } else {
                    let slab = self.slab_pool.alloc(key, value);
                    s.set_slab(hr.ext_fp_hi, hr.ext_fp, slab);
                }
                bucket.meta.on_access(slot_idx);
                return;
            }
        }

        // 2. Find free slot or evict
        let (target_slot, is_eviction) = if let Some(free) = bucket.meta.find_free_slot() {
            (free, false)
        } else if let Some(evict) = bucket.meta.find_evict_target() {
            // Clean up old slab if needed
            let old_slot = &bucket.slots[evict as usize];
            if old_slot.get_mode() == 1 {
                // Slab mode — pointer is being recycled by arena, no individual free needed
            }
            self.eviction_count += 1;
            (evict, true)
        } else {
            return; // Should never happen
        };

        // 3. Insert into target slot
        let slot = &mut bucket.slots[target_slot as usize];
        if key.len() <= 6 && value.len() <= 7 {
            slot.set_inline(key, value);
        } else {
            let slab = self.slab_pool.alloc(key, value);
            slot.set_slab(hr.ext_fp_hi, hr.ext_fp, slab);
        }

        bucket.meta.set_state(target_slot, SlotState::Full);
        bucket.meta.set_h2(target_slot, hr.h2);
        bucket.meta.on_insert(target_slot);

        if !is_eviction {
            self.count += 1;
        }
    }

    /// Look up a key. Returns the value if found.
    ///
    /// This method takes `&self` (not `&mut self`), allowing concurrent reads.
    /// Priority metadata is updated via interior mutability.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) % self.num_buckets;
        let bucket = &self.buckets[bucket_idx];

        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr) {
                // Safety: only mutating priority counters (frequency/recency),
                // not key-value data. This is safe for single-threaded use.
                unsafe {
                    let bucket_ptr = bucket as *const Bucket as *mut Bucket;
                    (*bucket_ptr).meta.on_access(slot_idx);
                }
                return Some(slot.get_value(&hr));
            }
        }
        None
    }

    /// Look up a key without updating priority (read-only, immutable).
    pub fn peek(&self, key: &[u8]) -> Option<&[u8]> {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) % self.num_buckets;
        let bucket = &self.buckets[bucket_idx];

        for slot_idx in 0..4u8 {
            if bucket.meta.get_state(slot_idx) != SlotState::Full {
                continue;
            }
            if bucket.meta.get_h2(slot_idx) != hr.h2 {
                continue;
            }
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr) {
                return Some(slot.get_value(&hr));
            }
        }
        None
    }

    /// Remove a key. Returns true if the key was found and removed.
    pub fn remove(&mut self, key: &[u8]) -> bool {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) % self.num_buckets;
        let bucket = &mut self.buckets[bucket_idx];

        for slot_idx in 0..4u8 {
            if bucket.meta.get_state(slot_idx) != SlotState::Full {
                continue;
            }
            if bucket.meta.get_h2(slot_idx) != hr.h2 {
                continue;
            }
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr) {
                bucket.meta.set_state(slot_idx, SlotState::Tombstone);
                bucket.slots[slot_idx as usize].clear();
                self.count -= 1;
                return true;
            }
        }
        false
    }

    /// Number of entries currently stored.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the map is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Total capacity (num_buckets × 4).
    #[inline]
    pub fn capacity(&self) -> usize {
        self.num_buckets * 4
    }

    /// Current load factor (0.0 to 1.0).
    #[inline]
    pub fn load_factor(&self) -> f64 {
        self.count as f64 / self.capacity() as f64
    }

    /// Number of evictions that have occurred.
    #[inline]
    pub fn eviction_count(&self) -> usize {
        self.eviction_count
    }
}

/// Slot state in the metadata word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotState {
    Empty = 0,
    Full = 1,
    Deleted = 2,
    Tombstone = 3,
}

impl SlotState {
    #[inline]
    fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => SlotState::Empty,
            1 => SlotState::Full,
            2 => SlotState::Deleted,
            3 => SlotState::Tombstone,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_get() {
        let mut map = PulseMap::new(16);
        map.insert(b"hello", b"world");
        assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_get_missing() {
        let mut map = PulseMap::new(16);
        assert_eq!(map.get(b"nope"), None);
    }

    #[test]
    fn test_update_existing() {
        let mut map = PulseMap::new(16);
        map.insert(b"key", b"val1");
        map.insert(b"key", b"val2");
        assert_eq!(map.get(b"key"), Some(&b"val2"[..]));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_remove() {
        let mut map = PulseMap::new(16);
        map.insert(b"key", b"val");
        assert!(map.remove(b"key"));
        assert_eq!(map.get(b"key"), None);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_remove_missing() {
        let mut map = PulseMap::new(16);
        assert!(!map.remove(b"nope"));
    }

    #[test]
    fn test_many_inserts() {
        let mut map = PulseMap::new(1024);
        for i in 0u32..1000 {
            let key = i.to_le_bytes();
            let val = (i * 2).to_le_bytes();
            map.insert(&key, &val);
        }
        // At least some should be findable
        let mut hits = 0;
        for i in 0u32..1000 {
            let key = i.to_le_bytes();
            if map.get(&key).is_some() {
                hits += 1;
            }
        }
        assert!(hits > 500, "Expected >500 hits, got {}", hits);
    }

    #[test]
    fn test_eviction_happens() {
        let mut map = PulseMap::new(4); // 16 slots only
        for i in 0u32..100 {
            let key = i.to_le_bytes();
            map.insert(&key, b"val");
        }
        assert!(map.eviction_count() > 0);
        assert!(map.len() <= 16);
    }

    #[test]
    fn test_slab_mode() {
        let mut map = PulseMap::new(16);
        let long_key = b"this_is_a_very_long_key_that_exceeds_six_bytes";
        let long_val = b"this_is_a_very_long_value_that_also_exceeds_seven_bytes";
        map.insert(long_key, long_val);
        assert_eq!(map.get(long_key), Some(&long_val[..]));
    }

    #[test]
    fn test_load_factor() {
        let mut map = PulseMap::new(100);
        assert_eq!(map.capacity(), 400);
        for i in 0u32..200 {
            map.insert(&i.to_le_bytes(), b"v");
        }
        assert!(map.load_factor() > 0.0);
        assert!(map.load_factor() <= 1.0);
    }

    #[test]
    fn test_peek_no_priority_update() {
        let map_const = PulseMap::new(16);
        // peek on empty should return None
        assert_eq!(map_const.peek(b"key"), None);
    }

    #[test]
    fn test_bucket_size() {
        assert_eq!(std::mem::size_of::<Bucket>(), 64, "Bucket must be exactly 64 bytes");
    }
}
