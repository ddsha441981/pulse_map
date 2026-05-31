// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Raw byte-level PulseMap — the foundation that generic PulseMap<K,V> wraps.
//!
//! Use `PulseMapRaw` directly when you want maximum control with `&[u8]` keys/values.
//! For ergonomic typed access, use `PulseMap<K, V>` instead.

use crate::core::bucket::Bucket;
use crate::core::hash::compute_hash;
use crate::core::slab::SlabPool;
use crate::SlotState;

/// Raw byte-level cache-line hash table with zero-cost eviction.
///
/// This is the engine that powers `PulseMap<K, V>`. It operates on raw `&[u8]` slices.
/// Most users should use `PulseMap<K, V>` instead.
pub struct PulseMapRaw {
    pub(crate) buckets: Vec<Bucket>,
    slab_pool: SlabPool,
    num_buckets: usize,
    bucket_mask: usize,
    count: usize,
    eviction_count: usize,
}

// Safety: PulseMapRaw uses interior mutability only for priority metadata updates
// (frequency/recency counters). The actual key-value data is never mutated during get().
unsafe impl Send for PulseMapRaw {}
unsafe impl Sync for PulseMapRaw {}

impl PulseMapRaw {
    /// Create a new PulseMapRaw with the given number of buckets.
    ///
    /// `num_buckets` is rounded up to the next power of 2 for fast bitwise indexing.
    /// Total capacity = `actual_buckets × 4` entries.
    pub fn new(num_buckets: usize) -> Self {
        let actual = num_buckets.max(1).next_power_of_two();
        let buckets = vec![Bucket::empty(); actual];
        Self {
            buckets,
            slab_pool: SlabPool::new(),
            num_buckets: actual,
            bucket_mask: actual - 1,
            count: 0,
            eviction_count: 0,
        }
    }

    /// Insert a raw key-value pair. Evicts lowest-priority entry on full bucket.
    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;
        let bucket = &mut self.buckets[bucket_idx];

        // 1. Check if key already exists (update in place)
        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr) {
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
            let old_slot = &bucket.slots[evict as usize];
            if old_slot.get_mode() == 1 {
                // Slab mode — arena recycles on deinit
            }
            self.eviction_count += 1;
            (evict, true)
        } else {
            return;
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

    /// Look up a key. Returns the value bytes if found.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;

        // Prefetch bucket into L1 cache before access
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let ptr = self.buckets.as_ptr().add(bucket_idx) as *const i8;
            std::arch::x86_64::_mm_prefetch(ptr, std::arch::x86_64::_MM_HINT_T0);
        }

        let bucket = &self.buckets[bucket_idx];

        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr) {
                unsafe {
                    let bucket_ptr = bucket as *const Bucket as *mut Bucket;
                    (*bucket_ptr).meta.on_access(slot_idx);
                }
                return Some(slot.get_value(&hr));
            }
        }
        None
    }

    /// Look up without updating priority.
    pub fn peek(&self, key: &[u8]) -> Option<&[u8]> {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;
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

    /// Remove a key. Returns true if found and removed.
    pub fn remove(&mut self, key: &[u8]) -> bool {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;
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

    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.num_buckets * 4
    }

    #[inline]
    pub fn num_buckets(&self) -> usize {
        self.num_buckets
    }

    #[inline]
    pub fn load_factor(&self) -> f64 {
        self.count as f64 / self.capacity() as f64
    }

    #[inline]
    pub fn eviction_count(&self) -> usize {
        self.eviction_count
    }
}
