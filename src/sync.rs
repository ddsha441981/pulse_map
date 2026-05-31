// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Thread-safe concurrent PulseMap with per-bucket spinlocks.
//!
//! `ConcurrentPulseMap<K, V>` allows multiple threads to read and write
//! concurrently. Each bucket has its own spinlock, so operations on
//! different buckets run fully in parallel.
//!
//! ```
//! use pulse_map::ConcurrentPulseMap;
//! use std::sync::Arc;
//!
//! let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(256));
//! let map2 = map.clone();
//!
//! std::thread::spawn(move || {
//!     map2.insert(42, 100);
//! }).join().unwrap();
//!
//! assert_eq!(map.get(&42), Some(100));
//! ```

use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, RwLock};

use crate::core::bucket::Bucket;
use crate::core::hash::compute_hash;
use crate::core::slab::SlabPool;
use crate::{PulseKey, PulseValue, SlotState};

// ═══════════════════════════════════════════════════════════════
// Per-Bucket Spinlock
// ═══════════════════════════════════════════════════════════════

/// Per-bucket spinlock array. Each bucket gets an independent AtomicU8 lock.
struct BucketLocks {
    locks: Vec<AtomicU8>,
}

impl BucketLocks {
    fn new(num_buckets: usize) -> Self {
        let locks = (0..num_buckets).map(|_| AtomicU8::new(0)).collect();
        Self { locks }
    }

    #[inline]
    fn lock(&self, bucket_idx: usize) {
        while self.locks[bucket_idx]
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            std::hint::spin_loop();
        }
    }

    #[inline]
    fn unlock(&self, bucket_idx: usize) {
        self.locks[bucket_idx].store(0, Ordering::Release);
    }
}

/// RAII guard that automatically unlocks a bucket when dropped.
struct BucketGuard<'a> {
    locks: &'a BucketLocks,
    idx: usize,
}

impl<'a> BucketGuard<'a> {
    #[inline]
    fn new(locks: &'a BucketLocks, idx: usize) -> Self {
        locks.lock(idx);
        Self { locks, idx }
    }
}

impl Drop for BucketGuard<'_> {
    #[inline]
    fn drop(&mut self) {
        self.locks.unlock(self.idx);
    }
}

// ═══════════════════════════════════════════════════════════════
// Inner State (behind RwLock for resize support)
// ═══════════════════════════════════════════════════════════════

/// Resizable inner state. Protected by RwLock:
/// - Normal ops: read lock (concurrent, cheap)
/// - Resize: write lock (exclusive, blocks everything)
struct MapInner {
    buckets: Vec<UnsafeCell<Bucket>>,
    locks: BucketLocks,
    slab_pool: Mutex<SlabPool>,
    num_buckets: usize,
    bucket_mask: usize,
}

// Safety: bucket access is protected by per-bucket spinlocks + RwLock.
unsafe impl Send for MapInner {}
unsafe impl Sync for MapInner {}

impl MapInner {
    fn new(num_buckets: usize) -> Self {
        let actual = num_buckets.max(1).next_power_of_two();
        let buckets = (0..actual)
            .map(|_| UnsafeCell::new(Bucket::empty()))
            .collect();
        Self {
            buckets,
            locks: BucketLocks::new(actual),
            slab_pool: Mutex::new(SlabPool::new()),
            num_buckets: actual,
            bucket_mask: actual - 1,
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// ConcurrentPulseMap
// ═══════════════════════════════════════════════════════════════

/// Thread-safe PulseMap with per-bucket locking and optional dynamic resize.
///
/// - All methods take `&self` (not `&mut self`) — safe to share via `Arc`.
/// - Different buckets are accessed fully in parallel.
/// - Same bucket: serialized via spinlock (fast for short critical sections).
/// - Resize: stop-the-world (write lock blocks all ops during rehash).
///
/// # Example
/// ```
/// use pulse_map::ConcurrentPulseMap;
/// use std::sync::Arc;
/// use std::thread;
///
/// let map = Arc::new(ConcurrentPulseMap::<u32, u64>::new(1024));
///
/// let handles: Vec<_> = (0..4).map(|t| {
///     let m = map.clone();
///     thread::spawn(move || {
///         for i in 0..1000u32 {
///             m.insert(t * 1000 + i, i as u64);
///         }
///     })
/// }).collect();
///
/// for h in handles { h.join().unwrap(); }
/// assert!(map.len() > 0);
/// ```
pub struct ConcurrentPulseMap<K: PulseKey, V: PulseValue> {
    inner: RwLock<MapInner>,
    count: AtomicUsize,
    eviction_count: AtomicUsize,
    auto_resize: bool,
    resize_threshold: f64,
    _marker: PhantomData<(K, V)>,
}

// Safety: RwLock + per-bucket spinlocks protect all access.
unsafe impl<K: PulseKey, V: PulseValue> Send for ConcurrentPulseMap<K, V> {}
unsafe impl<K: PulseKey, V: PulseValue> Sync for ConcurrentPulseMap<K, V> {}

impl<K: PulseKey, V: PulseValue> ConcurrentPulseMap<K, V> {
    /// Create a new fixed-size concurrent PulseMap.
    ///
    /// `num_buckets` is rounded up to the next power of 2.
    /// Total capacity = `actual_buckets × 4` entries.
    pub fn new(num_buckets: usize) -> Self {
        Self {
            inner: RwLock::new(MapInner::new(num_buckets)),
            count: AtomicUsize::new(0),
            eviction_count: AtomicUsize::new(0),
            auto_resize: false,
            resize_threshold: 0.75,
            _marker: PhantomData,
        }
    }

    /// Create a concurrent PulseMap that auto-resizes when load exceeds threshold.
    ///
    /// Default threshold: 75% load factor. When exceeded, the map doubles in size.
    ///
    /// ```
    /// use pulse_map::ConcurrentPulseMap;
    ///
    /// let map = ConcurrentPulseMap::<u32, u32>::with_auto_resize(64);
    /// for i in 0..1000u32 {
    ///     map.insert(i, i * 10);
    /// }
    /// // Map auto-grew from 64 to 1024+ buckets
    /// assert!(map.capacity() > 256);
    /// ```
    pub fn with_auto_resize(num_buckets: usize) -> Self {
        Self {
            inner: RwLock::new(MapInner::new(num_buckets)),
            count: AtomicUsize::new(0),
            eviction_count: AtomicUsize::new(0),
            auto_resize: true,
            resize_threshold: 0.75,
            _marker: PhantomData,
        }
    }

    /// Thread-safe insert. Takes `&self` — no `&mut` required.
    ///
    /// If auto-resize is enabled, triggers resize at > 75% load factor.
    pub fn insert(&self, key: K, value: V) {
        // Auto-resize check (before acquiring read lock)
        if self.auto_resize {
            let state = self.inner.read().unwrap();
            let num_bkts = state.num_buckets;
            let cap = num_bkts * 4;
            let len = self.count.load(Ordering::Relaxed);
            let load = len as f64 / cap as f64;
            drop(state);
            if load > self.resize_threshold {
                self.resize(num_bkts * 2);
            }
        }

        let kb = key.to_bytes();
        let vb = value.to_bytes();
        let key_bytes = kb.as_ref();
        let val_bytes = vb.as_ref();
        let hr = compute_hash(key_bytes);

        let state = self.inner.read().unwrap();
        let idx = (hr.h1 as usize) & state.bucket_mask;

        let _guard = BucketGuard::new(&state.locks, idx);

        // Safety: We hold the bucket lock + RwLock read, exclusive bucket access.
        let bucket = unsafe { &mut *state.buckets[idx].get() };

        // 1. Check if key already exists (update in place)
        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key_bytes, &hr) {
                let s = &mut bucket.slots[slot_idx as usize];
                if key_bytes.len() <= 6 && val_bytes.len() <= 7 {
                    s.set_inline(key_bytes, val_bytes);
                } else {
                    let mut slab = state.slab_pool.lock().unwrap();
                    let slab_ref = slab.alloc(key_bytes, val_bytes);
                    s.set_slab(hr.ext_fp_hi, hr.ext_fp, slab_ref);
                }
                bucket.meta.on_access(slot_idx);
                return;
            }
        }

        // 2. Find free slot or evict
        let (target_slot, is_eviction) = if let Some(free) = bucket.meta.find_free_slot() {
            (free, false)
        } else if let Some(evict) = bucket.meta.find_evict_target() {
            self.eviction_count.fetch_add(1, Ordering::Relaxed);
            (evict, true)
        } else {
            return;
        };

        // 3. Insert into target slot
        let slot = &mut bucket.slots[target_slot as usize];
        if key_bytes.len() <= 6 && val_bytes.len() <= 7 {
            slot.set_inline(key_bytes, val_bytes);
        } else {
            let mut slab = state.slab_pool.lock().unwrap();
            let slab_ref = slab.alloc(key_bytes, val_bytes);
            slot.set_slab(hr.ext_fp_hi, hr.ext_fp, slab_ref);
        }

        bucket.meta.set_state(target_slot, SlotState::Full);
        bucket.meta.set_h2(target_slot, hr.h2);
        bucket.meta.on_insert(target_slot);

        if !is_eviction {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Thread-safe lookup. Returns owned `Option<V>`.
    pub fn get(&self, key: &K) -> Option<V> {
        let kb = key.to_bytes();
        let key_bytes = kb.as_ref();
        let hr = compute_hash(key_bytes);

        let state = self.inner.read().unwrap();
        let idx = (hr.h1 as usize) & state.bucket_mask;

        let _guard = BucketGuard::new(&state.locks, idx);

        let bucket = unsafe { &mut *state.buckets[idx].get() };

        let mask = bucket.meta.match_mask(hr.h2);
        let mut m = mask;
        while m != 0 {
            let slot_idx = m.trailing_zeros() as u8;
            m &= m - 1;
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key_bytes, &hr) {
                bucket.meta.on_access(slot_idx);
                let val_bytes = slot.get_value(&hr);
                return V::from_bytes(val_bytes);
            }
        }
        None
    }

    /// Thread-safe lookup without priority update.
    pub fn peek(&self, key: &K) -> Option<V> {
        let kb = key.to_bytes();
        let key_bytes = kb.as_ref();
        let hr = compute_hash(key_bytes);

        let state = self.inner.read().unwrap();
        let idx = (hr.h1 as usize) & state.bucket_mask;

        let _guard = BucketGuard::new(&state.locks, idx);

        let bucket = unsafe { &*state.buckets[idx].get() };

        for slot_idx in 0..4u8 {
            if bucket.meta.get_state(slot_idx) != SlotState::Full {
                continue;
            }
            if bucket.meta.get_h2(slot_idx) != hr.h2 {
                continue;
            }
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key_bytes, &hr) {
                let val_bytes = slot.get_value(&hr);
                return V::from_bytes(val_bytes);
            }
        }
        None
    }

    /// Thread-safe key existence check.
    #[inline]
    pub fn contains_key(&self, key: &K) -> bool {
        self.peek(key).is_some()
    }

    /// Thread-safe removal. Returns true if key was found and removed.
    pub fn remove(&self, key: &K) -> bool {
        let kb = key.to_bytes();
        let key_bytes = kb.as_ref();
        let hr = compute_hash(key_bytes);

        let state = self.inner.read().unwrap();
        let idx = (hr.h1 as usize) & state.bucket_mask;

        let _guard = BucketGuard::new(&state.locks, idx);

        let bucket = unsafe { &mut *state.buckets[idx].get() };

        for slot_idx in 0..4u8 {
            if bucket.meta.get_state(slot_idx) != SlotState::Full {
                continue;
            }
            if bucket.meta.get_h2(slot_idx) != hr.h2 {
                continue;
            }
            let slot = &bucket.slots[slot_idx as usize];
            if slot.matches_key(key_bytes, &hr) {
                bucket.meta.set_state(slot_idx, SlotState::Tombstone);
                bucket.slots[slot_idx as usize].clear();
                self.count.fetch_sub(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    // ═══════════════════════════════════════════════════════════
    // Dynamic Resize
    // ═══════════════════════════════════════════════════════════

    /// Resize the map to a new number of buckets.
    ///
    /// Stop-the-world: acquires exclusive write lock, blocking all concurrent
    /// operations until rehashing completes. This is safe but causes a brief pause.
    ///
    /// `new_num_buckets` is rounded up to the next power of 2.
    pub fn resize(&self, new_num_buckets: usize) {
        let new_actual = new_num_buckets.max(1).next_power_of_two();

        // Acquire write lock — blocks ALL reads and writes
        let mut state = self.inner.write().unwrap();

        // Skip if already at target size (another thread may have resized)
        if state.num_buckets >= new_actual {
            return;
        }

        // Create new bucket array
        let new_buckets: Vec<UnsafeCell<Bucket>> = (0..new_actual)
            .map(|_| UnsafeCell::new(Bucket::empty()))
            .collect();
        let new_locks = BucketLocks::new(new_actual);
        let new_mask = new_actual - 1;
        let new_slab = Mutex::new(SlabPool::new());

        // Rehash all entries from old buckets into new buckets
        let mut new_count = 0usize;
        for bucket_cell in state.buckets.iter() {
            let bucket = unsafe { &*bucket_cell.get() };
            for slot_idx in 0..4u8 {
                if bucket.meta.get_state(slot_idx) != SlotState::Full {
                    continue;
                }
                let slot = &bucket.slots[slot_idx as usize];

                // Extract key and value bytes from the slot
                let _hr_from_h2 = bucket.meta.get_h2(slot_idx);
                let key_bytes = slot.get_key_bytes();
                let val_bytes = slot.get_value_bytes();

                if key_bytes.is_empty() {
                    continue;
                }

                // Rehash into new bucket
                let hr = compute_hash(key_bytes);
                let new_idx = (hr.h1 as usize) & new_mask;
                let new_bucket = unsafe { &mut *new_buckets[new_idx].get() };

                if let Some(free) = new_bucket.meta.find_free_slot() {
                    let new_slot = &mut new_bucket.slots[free as usize];
                    if key_bytes.len() <= 6 && val_bytes.len() <= 7 {
                        new_slot.set_inline(key_bytes, val_bytes);
                    } else {
                        let mut slab = new_slab.lock().unwrap();
                        let slab_ref = slab.alloc(key_bytes, val_bytes);
                        new_slot.set_slab(hr.ext_fp_hi, hr.ext_fp, slab_ref);
                    }
                    new_bucket.meta.set_state(free, SlotState::Full);
                    new_bucket.meta.set_h2(free, hr.h2);
                    new_bucket.meta.on_insert(free);
                    new_count += 1;
                }
                // If new bucket is full, entry is lost (eviction during resize)
            }
        }

        // Swap state
        state.buckets = new_buckets;
        state.locks = new_locks;
        state.slab_pool = new_slab;
        state.num_buckets = new_actual;
        state.bucket_mask = new_mask;

        // Update count to actual rehashed entries
        self.count.store(new_count, Ordering::Relaxed);
    }

    // ── Stats ──

    #[inline]
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        let state = self.inner.read().unwrap();
        state.num_buckets * 4
    }

    #[inline]
    pub fn load_factor(&self) -> f64 {
        let cap = self.capacity();
        self.len() as f64 / cap as f64
    }

    #[inline]
    pub fn eviction_count(&self) -> usize {
        self.eviction_count.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn num_buckets(&self) -> usize {
        let state = self.inner.read().unwrap();
        state.num_buckets
    }
}

// ═══════════════════════════════════════════════════════════════
// Display + Debug
// ═══════════════════════════════════════════════════════════════

impl<K: PulseKey, V: PulseValue> std::fmt::Debug for ConcurrentPulseMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentPulseMap")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .field("load_factor", &format!("{:.1}%", self.load_factor() * 100.0))
            .field("evictions", &self.eviction_count())
            .finish()
    }
}

impl<K: PulseKey, V: PulseValue> std::fmt::Display for ConcurrentPulseMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ConcurrentPulseMap({}/{} entries, {:.1}% load, {} evictions)",
            self.len(),
            self.capacity(),
            self.load_factor() * 100.0,
            self.eviction_count()
        )
    }
}
