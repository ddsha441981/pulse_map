// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Sharded concurrent PulseMap — 16 independent shards, no global lock.
//!
//! `ShardedPulseMap<K, V>` splits the keyspace across 16 independent
//! [`ConcurrentPulseMap`] shards. Operations on different keys almost never
//! contend, and `resize_all()` rehashes one shard at a time instead of
//! stopping the whole map.
//!
//! ```
//! use pulse_map::ShardedPulseMap;
//! use std::sync::Arc;
//!
//! let map = Arc::new(ShardedPulseMap::<u32, u32>::new(64)); // 64 buckets/shard
//! let map2 = map.clone();
//!
//! std::thread::spawn(move || {
//!     map2.insert(42, 100);
//! }).join().unwrap();
//!
//! assert_eq!(map.get(&42), Some(100));
//! ```

use crate::engine::hash::compute_hash;
use crate::sync::ConcurrentPulseMap;
use crate::{PulseKey, PulseValue};

/// Number of independent shards. Same as dashmap's default sweet spot.
const NUM_SHARDS: usize = 16;

/// Thread-safe PulseMap sharded across 16 independent [`ConcurrentPulseMap`]s.
///
/// - Different keys hash to different shards → no cross-shard contention.
/// - `resize_all()` resizes shard-by-shard — no stop-the-world pause for the
///   whole map (each pause is 1/16th the work).
/// - All methods take `&self` — share via `Arc`.
///
/// Global stats (`len`, `eviction_count`) are sums over shards.
pub struct ShardedPulseMap<K: PulseKey, V: PulseValue> {
    shards: Box<[ConcurrentPulseMap<K, V>; NUM_SHARDS]>,
}

impl<K: PulseKey, V: PulseValue> ShardedPulseMap<K, V> {
    /// Create with `buckets_per_shard` buckets in each of the 16 shards
    /// (rounded up to a power of 2 per shard).
    ///
    /// Total capacity = `16 × buckets_per_shard × 4` entries.
    pub fn new(buckets_per_shard: usize) -> Self {
        Self {
            shards: Box::new(core::array::from_fn(|_| {
                ConcurrentPulseMap::new(buckets_per_shard)
            })),
        }
    }

    /// Create with auto-resize enabled on every shard (grows at 75% load).
    pub fn with_auto_resize(buckets_per_shard: usize) -> Self {
        Self {
            shards: Box::new(core::array::from_fn(|_| {
                ConcurrentPulseMap::with_auto_resize(buckets_per_shard)
            })),
        }
    }

    /// Pick the shard for a key.
    ///
    /// Uses bits 14-17 of the hash to avoid overlapping with:
    ///   - `h2` fingerprint (bits 57-63) — preserves full 7-bit entropy
    ///   - bucket_mask (low bits 0-N) — avoids clustering within shards
    #[inline]
    fn shard_for(key_bytes: &[u8]) -> usize {
        (compute_hash(key_bytes).h1 >> 14) as usize & (NUM_SHARDS - 1)
    }

    /// Thread-safe insert.
    pub fn insert(&self, key: K, value: V) {
        let idx = key.with_key_bytes(Self::shard_for);
        self.shards[idx].insert(key, value);
    }

    /// Thread-safe insert with a per-entry TTL override.
    ///
    /// - `ttl = 0`: use the map's default TTL
    /// - `ttl = u64::MAX`: this entry never expires
    /// - `ttl = N`: this entry expires after N insertions
    pub fn insert_ttl(&self, key: K, value: V, ttl: u64) {
        let idx = key.with_key_bytes(Self::shard_for);
        self.shards[idx].insert_ttl(key, value, ttl);
    }

    /// Thread-safe lookup (updates LFU+LRU priority).
    pub fn get(&self, key: &K) -> Option<V> {
        let idx = key.with_key_bytes(Self::shard_for);
        self.shards[idx].get(key)
    }

    /// Thread-safe lookup without priority update.
    pub fn peek(&self, key: &K) -> Option<V> {
        let idx = key.with_key_bytes(Self::shard_for);
        self.shards[idx].peek(key)
    }

    /// Thread-safe key existence check.
    #[inline]
    pub fn contains_key(&self, key: &K) -> bool {
        self.peek(key).is_some()
    }

    /// Thread-safe removal. Returns true if the key was found and removed.
    pub fn remove(&self, key: &K) -> bool {
        let idx = key.with_key_bytes(Self::shard_for);
        self.shards[idx].remove(key)
    }

    /// Resize every shard to `new_buckets_per_shard`, one shard at a time.
    ///
    /// Unlike [`ConcurrentPulseMap::resize`], this never pauses the whole
    /// map — only the shard currently being rehashed blocks.
    pub fn resize_all(&self, new_buckets_per_shard: usize) {
        for shard in self.shards.iter() {
            shard.resize(new_buckets_per_shard);
        }
    }

    /// Set TTL (in insertion epochs) on every shard. 0 = disabled.
    ///
    /// Each shard counts its own epochs, so an entry expires after `ttl`
    /// inserts landing in ITS shard (~`ttl × 16` inserts map-wide).
    pub fn set_ttl(&self, ttl: u64) {
        for shard in self.shards.iter() {
            shard.set_ttl(ttl);
        }
    }

    /// Current TTL setting (same across all shards).
    #[inline]
    pub fn get_ttl(&self) -> u64 {
        self.shards[0].get_ttl()
    }

    /// Highest epoch across shards (the most active shard's insert count).
    pub fn current_epoch(&self) -> u64 {
        self.shards
            .iter()
            .map(|s| s.current_epoch())
            .max()
            .unwrap_or(0)
    }

    /// Total live entries across all shards.
    pub fn len(&self) -> usize {
        self.shards.iter().map(|s| s.len()).sum()
    }

    /// Returns true if no shard holds any entry.
    pub fn is_empty(&self) -> bool {
        self.shards.iter().all(|s| s.is_empty())
    }

    /// Total capacity across all shards.
    pub fn capacity(&self) -> usize {
        self.shards.iter().map(|s| s.capacity()).sum()
    }

    /// Map-wide load factor.
    pub fn load_factor(&self) -> f64 {
        let cap = self.capacity();
        if cap == 0 {
            0.0
        } else {
            self.len() as f64 / cap as f64
        }
    }

    /// Total evictions across all shards.
    pub fn eviction_count(&self) -> usize {
        self.shards.iter().map(|s| s.eviction_count()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_sharded_basic_insert_get() {
        // 16384 buckets/shard → 65K slots/shard. 1000 keys / 16 shards ≈ 62/shard = 0.1% load.
        // Eliminates bucket collisions so we can assert exact retrieval.
        let map = ShardedPulseMap::<u32, u32>::new(16384);
        for i in 0u32..1000 {
            map.insert(i, i * 2);
        }
        for i in 0u32..1000 {
            assert_eq!(map.get(&i), Some(i * 2));
        }
        assert_eq!(map.len(), 1000);
        assert!(map.remove(&500));
        assert_eq!(map.get(&500), None);
        assert_eq!(map.len(), 999);
    }

    #[test]
    fn test_sharded_concurrent_4thread() {
        // 16384 buckets/shard. 40K keys / 16 shards = 2500/shard → ~0.6% load.
        let map = Arc::new(ShardedPulseMap::<u32, u32>::new(16384));
        let handles: Vec<_> = (0..4u32)
            .map(|t| {
                let m = map.clone();
                thread::spawn(move || {
                    for i in 0..10_000u32 {
                        m.insert(t * 10_000 + i, i);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(map.len(), 40_000);
        assert_eq!(map.get(&15_000), Some(5_000));
    }

    #[test]
    fn test_sharded_resize_all() {
        // Start with 1024 buckets/shard (safe for 500 keys), resize to 2048.
        let map = ShardedPulseMap::<u32, u32>::new(1024);
        for i in 0u32..500 {
            map.insert(i, i);
        }
        let before = map.capacity();
        map.resize_all(2048);
        assert!(map.capacity() > before);
        // All entries survive the rehash
        for i in 0u32..500 {
            assert_eq!(map.get(&i), Some(i));
        }
    }

    #[test]
    fn test_sharded_ttl_propagation() {
        let map = ShardedPulseMap::<u32, u32>::new(64);
        map.set_ttl(100);
        assert_eq!(map.get_ttl(), 100);
        map.insert(1, 1);
        assert!(map.current_epoch() >= 1);
        map.set_ttl(0);
        assert_eq!(map.get_ttl(), 0);
    }

    #[test]
    fn test_sharded_len_sum_of_shards() {
        let map = ShardedPulseMap::<u32, u32>::new(64);
        assert!(map.is_empty());
        for i in 0u32..100 {
            map.insert(i, i);
        }
        let shard_sum: usize = map.shards.iter().map(|s| s.len()).sum();
        assert_eq!(map.len(), shard_sum);
        assert_eq!(map.len(), 100);
        // Keys actually spread across more than one shard
        let used = map.shards.iter().filter(|s| !s.is_empty()).count();
        assert!(used > 1, "all keys landed in {used} shard(s)");
    }
}
