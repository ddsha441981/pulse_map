// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Raw byte-level PulseMap — the foundation that generic PulseMap<K,V> wraps.
//!
//! Use `PulseMapRaw` directly when you want maximum control with `&[u8]` keys/values.
//! For ergonomic typed access, use `PulseMap<K, V>` instead.

use crate::engine::bucket::Bucket;
use crate::engine::hash::compute_hash;
use crate::engine::slab::SlabPool;
use crate::SlotState;
#[cfg(not(feature = "std"))]
use alloc::vec;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Per-slot TTL data: insertion epoch + per-entry TTL override.
#[derive(Clone, Copy, Default)]
pub(crate) struct SlotTTL {
    /// Epoch at which this entry was inserted/last updated.
    pub epoch: u64,
    /// Per-entry TTL. 0 = use default_ttl, u64::MAX = never expire.
    pub ttl: u64,
}

/// Raw byte-level cache-line hash table with zero-cost eviction and optional TTL.
///
/// This is the engine that powers `PulseMap<K, V>`. It operates on raw `&[u8]` slices.
/// Most users should use `PulseMap<K, V>` instead.
pub struct PulseMapRaw {
    pub(crate) buckets: Vec<Bucket>,
    pub(crate) slab_pool: SlabPool,
    num_buckets: usize,
    bucket_mask: usize,
    count: usize,
    eviction_count: usize,

    // ── TTL via epoch counter ──
    /// Per-slot TTL data: insertion epoch + per-entry TTL override.
    slots_ttl: Vec<SlotTTL>,
    /// Monotonically increasing counter. Incremented on every insert.
    current_epoch: u64,
    /// Default TTL in insertion epochs. 0 = disabled. Per-entry TTL overrides this.
    default_ttl: u64,
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
        let slots_ttl = vec![SlotTTL::default(); actual * 4];
        Self {
            buckets,
            slab_pool: SlabPool::new(),
            num_buckets: actual,
            bucket_mask: actual - 1,
            count: 0,
            eviction_count: 0,
            slots_ttl,
            current_epoch: 0,
            default_ttl: 0,
        }
    }

    /// Set the TTL in insertion epochs.
    ///
    /// After `ttl_epochs` insertions, entries are considered expired and
    /// `get()`/`peek()` return `None`. Set to `0` to disable TTL.
    ///
    /// # Example
    /// ```
    /// use pulse_map::PulseMap;
    /// let mut map = PulseMap::new(16);
    /// map.set_ttl(100); // entries expire after 100 insertions
    /// map.insert(b"key", b"val");
    /// assert_eq!(map.get(b"key"), Some(&b"val"[..]));
    /// ```
    #[inline]
    pub fn set_ttl(&mut self, ttl_epochs: u64) {
        self.default_ttl = ttl_epochs;
    }

    /// Returns the current TTL setting (0 = disabled).
    #[inline]
    pub fn get_ttl(&self) -> u64 {
        self.default_ttl
    }

    /// Returns the current epoch counter (total insertions so far).
    #[inline]
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Check if a slot has expired (per-entry or global TTL).
    #[inline]
    fn is_expired(&self, bucket_idx: usize, slot_idx: u8) -> bool {
        let entry = self.slots_ttl[bucket_idx * 4 + slot_idx as usize];
        let effective_ttl = if entry.ttl == 0 {
            self.default_ttl
        } else {
            entry.ttl
        };
        if effective_ttl == 0 || effective_ttl == u64::MAX {
            return false;
        }
        self.current_epoch.wrapping_sub(entry.epoch) > effective_ttl
    }

    /// Stamp a slot's insertion epoch and per-entry TTL.
    #[inline]
    fn stamp_slot_ttl(&mut self, bucket_idx: usize, slot_idx: u8, ttl: u64) {
        self.slots_ttl[bucket_idx * 4 + slot_idx as usize] = SlotTTL {
            epoch: self.current_epoch,
            ttl,
        };
    }

    /// Insert a raw key-value pair. Uses the map's default TTL.
    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        self.insert_internal(key, value, 0);
    }

    /// Insert with a per-entry TTL override.
    ///
    /// - `ttl = 0`: use the map's default TTL (`set_ttl()`)
    /// - `ttl = u64::MAX`: this entry never expires
    /// - `ttl = N`: this entry expires after N insertions
    pub fn insert_ttl(&mut self, key: &[u8], value: &[u8], ttl: u64) {
        self.insert_internal(key, value, ttl);
    }

    /// Internal insert with TTL parameter. `ttl = 0` means use `default_ttl`.
    fn insert_internal(&mut self, key: &[u8], value: &[u8], ttl: u64) {
        self.current_epoch = self.current_epoch.wrapping_add(1);

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
            if slot.matches_key(key, &hr, &self.slab_pool) {
                if slot.get_mode() == 1 {
                    self.slab_pool.free(slot.slab_idx());
                }
                let s = &mut bucket.slots[slot_idx as usize];
                if key.len() <= 6 && value.len() <= 7 {
                    s.set_inline(key, value);
                } else {
                    let idx = self.slab_pool.alloc(key, value);
                    s.set_slab(hr.ext_fp_hi, hr.ext_fp, idx);
                }
                bucket.meta.on_access(slot_idx);
                self.stamp_slot_ttl(bucket_idx, slot_idx, ttl);
                return;
            }
        }

        // 2. Find free slot or evict
        let (target_slot, is_eviction) = if let Some(free) = self.find_free_or_expired(bucket_idx) {
            let is_ev = self.buckets[bucket_idx].meta.get_state(free) == SlotState::Full;
            if is_ev {
                let old_slot = &self.buckets[bucket_idx].slots[free as usize];
                if old_slot.get_mode() == 1 {
                    self.slab_pool.free(old_slot.slab_idx());
                }
                self.eviction_count += 1;
            }
            (free, is_ev)
        } else if let Some(evict) = self.buckets[bucket_idx].meta.find_evict_target() {
            let old_slot = &self.buckets[bucket_idx].slots[evict as usize];
            if old_slot.get_mode() == 1 {
                self.slab_pool.free(old_slot.slab_idx());
            }
            self.eviction_count += 1;
            (evict, true)
        } else {
            return;
        };

        // 3. Insert into target slot
        let slot = &mut self.buckets[bucket_idx].slots[target_slot as usize];
        if key.len() <= 6 && value.len() <= 7 {
            slot.set_inline(key, value);
        } else {
            let idx = self.slab_pool.alloc(key, value);
            slot.set_slab(hr.ext_fp_hi, hr.ext_fp, idx);
        }

        self.buckets[bucket_idx]
            .meta
            .set_state(target_slot, SlotState::Full);
        self.buckets[bucket_idx].meta.set_h2(target_slot, hr.h2);
        self.buckets[bucket_idx].meta.on_insert(target_slot);
        self.stamp_slot_ttl(bucket_idx, target_slot, ttl);

        if !is_eviction {
            self.count += 1;
        }
    }

    /// Find a free slot or an expired slot in a bucket (lazy TTL eviction).
    fn find_free_or_expired(&self, bucket_idx: usize) -> Option<u8> {
        let bucket = &self.buckets[bucket_idx];
        for i in 0..4u8 {
            let state = bucket.meta.get_state(i);
            if state != SlotState::Full {
                return Some(i); // Empty or Tombstone
            }
            // Expired Full slot → reusable (per-entry or global TTL)
            if self.is_expired(bucket_idx, i) {
                return Some(i);
            }
        }
        None
    }

    /// Look up a key. Returns the value bytes if found and not expired.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;

        // Prefetch bucket into L1 cache before access
        #[cfg(target_arch = "x86_64")]
        unsafe {
            let ptr = self.buckets.as_ptr().add(bucket_idx) as *const i8;
            core::arch::x86_64::_mm_prefetch(ptr, core::arch::x86_64::_MM_HINT_T0);
        }

        let bucket = &self.buckets[bucket_idx];

        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr, &self.slab_pool) {
                // Check TTL expiry
                if self.is_expired(bucket_idx, slot_idx) {
                    return None;
                }
                unsafe {
                    let bucket_ptr = bucket as *const Bucket as *mut Bucket;
                    (*bucket_ptr).meta.on_access(slot_idx);
                }
                return Some(slot.get_value(&self.slab_pool));
            }
        }
        None
    }

    /// Look up without updating priority. Returns None if expired.
    pub fn peek(&self, key: &[u8]) -> Option<&[u8]> {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;
        let bucket = &self.buckets[bucket_idx];

        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr, &self.slab_pool) {
                if self.is_expired(bucket_idx, slot_idx) {
                    return None;
                }
                return Some(slot.get_value(&self.slab_pool));
            }
        }
        None
    }

    /// Remove a key. Returns true if found and removed.
    pub fn remove(&mut self, key: &[u8]) -> bool {
        let hr = compute_hash(key);
        let bucket_idx = (hr.h1 as usize) & self.bucket_mask;
        let bucket = &mut self.buckets[bucket_idx];

        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key, &hr, &self.slab_pool) {
                if slot.get_mode() == 1 {
                    self.slab_pool.free(slot.slab_idx());
                }
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
