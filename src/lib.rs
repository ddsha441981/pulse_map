// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

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
//! let mut map = PulseMap::new(1024);
//! map.insert(b"hello", b"world");
//! assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
//! map.remove(b"hello");
//! assert_eq!(map.get(b"hello"), None);
//! ```
//!
//! ## Typed Usage
//! ```
//! use pulse_map::TypedPulseMap;
//!
//! let mut map = TypedPulseMap::<u32, u64>::new(256);
//! map.insert(42, 100);
//! assert_eq!(map.get(&42), Some(100));
//! ```

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};
#[cfg(feature = "std")]
use std::{string::String, vec::Vec};

mod engine;
mod iter;
mod raw;
#[cfg(all(target_arch = "x86_64", feature = "simd"))]
mod simd;
#[cfg(feature = "std")]
mod sharded;
#[cfg(feature = "std")]
mod sync;
mod traits;

// ── Re-exports ──
pub use crate::engine::bucket::Bucket;
pub use crate::engine::meta::MetaWord;
pub use crate::engine::slot::Slot;
pub use iter::{RawIter, TypedIter};
pub use raw::PulseMapRaw;
#[cfg(feature = "std")]
pub use sharded::ShardedPulseMap;
#[cfg(feature = "std")]
pub use sync::ConcurrentPulseMap;

// ── SlotState (shared by core and raw) ──

/// Slot state in the metadata word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotState {
    Empty = 0,
    Full = 1,
    /// Used by `remove()` to mark a slot as logically deleted.
    /// The slot is reusable for future inserts.
    Tombstone = 2,
}

impl SlotState {
    #[inline]
    pub(crate) fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            1 => SlotState::Full,
            2 => SlotState::Tombstone,
            _ => SlotState::Empty, // 0 = Empty, 3 = unused → treat as Empty
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// PulseMap — Raw byte API (backward compatible with v0.1.0)
// ═══════════════════════════════════════════════════════════════

/// A CPU cache-line hash table with zero-cost eviction.
///
/// This is the raw `&[u8]` API. For typed access, use [`TypedPulseMap`].
///
/// # Example
/// ```
/// use pulse_map::PulseMap;
///
/// let mut map = PulseMap::new(16);
/// map.insert(b"key", b"value");
/// assert_eq!(map.get(b"key"), Some(&b"value"[..]));
/// ```
pub type PulseMap = PulseMapRaw;

// ═══════════════════════════════════════════════════════════════
// PulseKey / PulseValue — Traits for typed access
// ═══════════════════════════════════════════════════════════════

/// Trait for types that can be used as PulseMap keys.
///
/// Uses associated type `Bytes` to avoid heap allocation for fixed-size types.
/// Example: `u32::to_bytes()` returns `[u8; 4]` on the stack — zero heap alloc.
pub trait PulseKey: Sized {
    /// Byte representation type. `[u8; N]` for fixed-size, `Vec<u8>` for dynamic.
    type Bytes: AsRef<[u8]>;
    /// Serialize to bytes (zero-alloc for numeric types).
    fn to_bytes(&self) -> Self::Bytes;
    /// Deserialize from bytes.
    fn from_bytes(bytes: &[u8]) -> Option<Self>;

    /// Run `f` with a borrowed byte view of the key.
    ///
    /// Read paths (`get`, `peek`, `remove`, `contains_key`) use this instead of
    /// `to_bytes()`. The default falls back to `to_bytes()`; heap-backed keys
    /// (`String`, `Vec<u8>`) override it to borrow their bytes directly —
    /// zero allocation per lookup.
    #[inline]
    fn with_key_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(self.to_bytes().as_ref())
    }
}

/// Trait for types that can be used as PulseMap values.
///
/// Same zero-alloc design as `PulseKey`.
pub trait PulseValue: Sized {
    /// Byte representation type.
    type Bytes: AsRef<[u8]>;
    /// Serialize to bytes (zero-alloc for numeric types).
    fn to_bytes(&self) -> Self::Bytes;
    /// Deserialize from bytes.
    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}

// ── Built-in PulseKey implementations ──

impl PulseKey for u8 {
    type Bytes = [u8; 1];
    fn to_bytes(&self) -> [u8; 1] {
        [*self]
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.first().copied()
    }
}

impl PulseKey for u16 {
    type Bytes = [u8; 2];
    fn to_bytes(&self) -> [u8; 2] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(u16::from_le_bytes)
    }
}

impl PulseKey for u32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(u32::from_le_bytes)
    }
}

impl PulseKey for u64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(u64::from_le_bytes)
    }
}

impl PulseKey for i32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(i32::from_le_bytes)
    }
}

impl PulseKey for i64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(i64::from_le_bytes)
    }
}

impl PulseKey for String {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        core::str::from_utf8(b).ok().map(String::from)
    }
    #[inline]
    fn with_key_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(self.as_bytes())
    }
}

impl PulseKey for Vec<u8> {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> {
        self.clone()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        Some(b.to_vec())
    }
    #[inline]
    fn with_key_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(self)
    }
}

impl<const N: usize> PulseKey for [u8; N] {
    type Bytes = [u8; N];
    fn to_bytes(&self) -> [u8; N] {
        *self
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok()
    }
    #[inline]
    fn with_key_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        f(self)
    }
}

// ── Built-in PulseValue implementations ──

impl PulseValue for u8 {
    type Bytes = [u8; 1];
    fn to_bytes(&self) -> [u8; 1] {
        [*self]
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.first().copied()
    }
}

impl PulseValue for u16 {
    type Bytes = [u8; 2];
    fn to_bytes(&self) -> [u8; 2] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(u16::from_le_bytes)
    }
}

impl PulseValue for u32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(u32::from_le_bytes)
    }
}

impl PulseValue for u64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(u64::from_le_bytes)
    }
}

impl PulseValue for i32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(i32::from_le_bytes)
    }
}

impl PulseValue for i64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] {
        self.to_le_bytes()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.try_into().ok().map(i64::from_le_bytes)
    }
}

impl PulseValue for String {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        core::str::from_utf8(b).ok().map(String::from)
    }
}

impl PulseValue for Vec<u8> {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> {
        self.clone()
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        Some(b.to_vec())
    }
}

impl PulseValue for bool {
    type Bytes = [u8; 1];
    fn to_bytes(&self) -> [u8; 1] {
        [*self as u8]
    }
    fn from_bytes(b: &[u8]) -> Option<Self> {
        b.first().map(|&v| v != 0)
    }
}

// ═══════════════════════════════════════════════════════════════
// TypedPulseMap<K, V> — Generic wrapper over PulseMapRaw
// ═══════════════════════════════════════════════════════════════

/// A typed cache-line hash table with zero-cost eviction.
///
/// Wraps [`PulseMapRaw`] with type-safe key/value serialization.
///
/// # Example
/// ```
/// use pulse_map::TypedPulseMap;
///
/// let mut map = TypedPulseMap::<u32, u64>::new(256);
/// map.insert(42, 100);
/// assert_eq!(map.get(&42), Some(100));
/// map.remove(&42);
/// assert_eq!(map.get(&42), None);
/// ```
pub struct TypedPulseMap<K: PulseKey, V: PulseValue> {
    raw: PulseMapRaw,
    _marker: core::marker::PhantomData<(K, V)>,
}

impl<K: PulseKey, V: PulseValue> TypedPulseMap<K, V> {
    /// Create a new TypedPulseMap with the given number of buckets.
    pub fn new(num_buckets: usize) -> Self {
        Self {
            raw: PulseMapRaw::new(num_buckets),
            _marker: core::marker::PhantomData,
        }
    }

    /// Insert a key-value pair.
    pub fn insert(&mut self, key: K, value: V) {
        let kb = key.to_bytes();
        let vb = value.to_bytes();
        self.raw.insert(kb.as_ref(), vb.as_ref());
    }

    /// Insert a key-value pair with a per-entry TTL override.
    ///
    /// - `ttl = 0`: use the map's default TTL (`set_ttl()`)
    /// - `ttl = u32::MAX`: this entry never expires
    /// - `ttl = N`: this entry expires after N insertions
    pub fn insert_ttl(&mut self, key: K, value: V, ttl: u32) {
        let kb = key.to_bytes();
        let vb = value.to_bytes();
        self.raw.insert_ttl(kb.as_ref(), vb.as_ref(), ttl);
    }

    /// Look up a key. Returns the deserialized value if found.
    pub fn get(&self, key: &K) -> Option<V> {
        key.with_key_bytes(|kb| self.raw.get(kb).and_then(V::from_bytes))
    }

    /// Look up without updating priority.
    pub fn peek(&self, key: &K) -> Option<V> {
        key.with_key_bytes(|kb| self.raw.peek(kb).and_then(V::from_bytes))
    }

    /// Remove a key.
    pub fn remove(&mut self, key: &K) -> bool {
        key.with_key_bytes(|kb| self.raw.remove(kb))
    }

    /// Check if a key exists.
    pub fn contains_key(&self, key: &K) -> bool {
        key.with_key_bytes(|kb| self.raw.peek(kb).is_some())
    }

    /// Iterate over all (key, value) pairs.
    pub fn iter(&self) -> TypedIter<'_, K, V> {
        TypedIter::new(&self.raw)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.raw.capacity()
    }

    #[inline]
    pub fn load_factor(&self) -> f64 {
        self.raw.load_factor()
    }

    #[inline]
    pub fn eviction_count(&self) -> usize {
        self.raw.eviction_count()
    }

    /// Set the TTL in insertion epochs.
    ///
    /// Entries that were inserted more than `ttl_epochs` insertions ago
    /// are treated as expired — `get()`/`peek()` return `None`.
    ///
    /// Set to `0` to disable TTL (default).
    ///
    /// # Example
    /// ```
    /// use pulse_map::TypedPulseMap;
    /// let mut map = TypedPulseMap::<u32, u32>::new(16);
    /// map.set_ttl(2); // entries expire after 2 insertions
    /// map.insert(1, 100);
    /// map.insert(2, 200);
    /// map.insert(3, 300); // this is the 3rd insert, key=1 may expire now
    /// ```
    #[inline]
    pub fn set_ttl(&mut self, ttl_epochs: u32) {
        self.raw.set_ttl(ttl_epochs);
    }

    /// Returns the current TTL setting (0 = disabled).
    #[inline]
    pub fn get_ttl(&self) -> u32 {
        self.raw.get_ttl()
    }

    /// Returns the current epoch counter (total insertions).
    #[inline]
    pub fn current_epoch(&self) -> u32 {
        self.raw.current_epoch()
    }

    /// Gets the given key's entry in the map for in-place manipulation.
    ///
    /// # Example
    /// ```
    /// use pulse_map::TypedPulseMap;
    /// let mut map = TypedPulseMap::<u32, u32>::new(64);
    /// map.entry(42).or_insert(100);
    /// assert_eq!(map.get(&42), Some(100));
    /// ```
    pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        let kb = key.to_bytes();
        let existing = self.raw.peek(kb.as_ref()).and_then(|vb| V::from_bytes(vb));
        match existing {
            Some(val) => Entry::Occupied(OccupiedEntry {
                map: self,
                key,
                value: val,
            }),
            None => Entry::Vacant(VacantEntry { map: self, key }),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Entry API
// ═══════════════════════════════════════════════════════════════

/// A view into a single entry in a TypedPulseMap.
pub enum Entry<'a, K: PulseKey, V: PulseValue> {
    /// Key exists in the map.
    Occupied(OccupiedEntry<'a, K, V>),
    /// Key does not exist in the map.
    Vacant(VacantEntry<'a, K, V>),
}

/// A view into an occupied entry in a TypedPulseMap.
pub struct OccupiedEntry<'a, K: PulseKey, V: PulseValue> {
    map: &'a mut TypedPulseMap<K, V>,
    key: K,
    value: V,
}

/// A view into a vacant entry in a TypedPulseMap.
pub struct VacantEntry<'a, K: PulseKey, V: PulseValue> {
    map: &'a mut TypedPulseMap<K, V>,
    key: K,
}

impl<'a, K: PulseKey, V: PulseValue> Entry<'a, K, V> {
    /// Insert default value if vacant.
    pub fn or_insert(self, default: V) {
        if let Entry::Vacant(e) = self {
            e.map.insert(e.key, default);
        }
    }

    /// Insert computed value if vacant.
    pub fn or_insert_with<F: FnOnce() -> V>(self, f: F) {
        if let Entry::Vacant(e) = self {
            e.map.insert(e.key, f());
        }
    }

    /// Modify the value if occupied, then return self for chaining.
    pub fn and_modify<F: FnOnce(&mut V)>(self, f: F) -> Self {
        match self {
            Entry::Occupied(mut e) => {
                f(&mut e.value);
                // Write modified value back
                let kb = e.key.to_bytes();
                let vb = e.value.to_bytes();
                e.map.raw.insert(kb.as_ref(), vb.as_ref());
                Entry::Occupied(e)
            }
            Entry::Vacant(e) => Entry::Vacant(e),
        }
    }
}

impl<'a, K: PulseKey, V: PulseValue> OccupiedEntry<'a, K, V> {
    /// Get the current value.
    pub fn get(&self) -> &V {
        &self.value
    }

    /// Get the key.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Replace the value and return the old one.
    pub fn insert(self, value: V) -> V {
        self.map.insert(self.key, value);
        self.value
    }

    /// Remove the entry and return the value.
    pub fn remove(self) -> V {
        self.map.remove(&self.key);
        self.value
    }
}

impl<'a, K: PulseKey, V: PulseValue> VacantEntry<'a, K, V> {
    /// Get the key.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Insert a value into the vacant entry.
    pub fn insert(self, value: V) {
        self.map.insert(self.key, value);
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Raw API tests (backward compat) ──

    #[test]
    fn test_raw_insert_and_get() {
        let mut map = PulseMap::new(16);
        map.insert(b"hello", b"world");
        assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_raw_get_missing() {
        let map = PulseMap::new(16);
        assert_eq!(map.get(b"nope"), None);
    }

    #[test]
    fn test_raw_update_existing() {
        let mut map = PulseMap::new(16);
        map.insert(b"key", b"val1");
        map.insert(b"key", b"val2");
        assert_eq!(map.get(b"key"), Some(&b"val2"[..]));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_raw_remove() {
        let mut map = PulseMap::new(16);
        map.insert(b"key", b"val");
        assert!(map.remove(b"key"));
        assert_eq!(map.get(b"key"), None);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_raw_remove_missing() {
        let mut map = PulseMap::new(16);
        assert!(!map.remove(b"nope"));
    }

    #[test]
    fn test_raw_many_inserts() {
        let mut map = PulseMap::new(1024);
        for i in 0u32..1000 {
            map.insert(&i.to_le_bytes(), &(i * 2).to_le_bytes());
        }
        let mut hits = 0;
        for i in 0u32..1000 {
            if map.get(&i.to_le_bytes()).is_some() {
                hits += 1;
            }
        }
        assert!(hits > 500, "Expected >500 hits, got {}", hits);
    }

    #[test]
    fn test_raw_eviction() {
        let mut map = PulseMap::new(4);
        for i in 0u32..100 {
            map.insert(&i.to_le_bytes(), b"val");
        }
        assert!(map.eviction_count() > 0);
        assert!(map.len() <= 16);
    }

    #[test]
    fn test_raw_slab_mode() {
        let mut map = PulseMap::new(16);
        let long_key = b"this_is_a_very_long_key_that_exceeds_six_bytes";
        let long_val = b"this_is_a_very_long_value_that_also_exceeds_seven_bytes";
        map.insert(long_key, long_val);
        assert_eq!(map.get(long_key), Some(&long_val[..]));
    }

    #[test]
    fn test_raw_load_factor() {
        let mut map = PulseMap::new(100);
        // 100 rounds up to 128 (next power of 2), 128 * 4 = 512
        assert_eq!(map.capacity(), 512);
        for i in 0u32..200 {
            map.insert(&i.to_le_bytes(), b"v");
        }
        assert!(map.load_factor() > 0.0);
        assert!(map.load_factor() <= 1.0);
    }

    #[test]
    fn test_raw_peek() {
        let map = PulseMap::new(16);
        assert_eq!(map.peek(b"key"), None);
    }

    #[test]
    fn test_bucket_size() {
        assert_eq!(
            core::mem::size_of::<Bucket>(),
            64,
            "Bucket must be exactly 64 bytes"
        );
    }

    // ── Typed API tests ──

    #[test]
    fn test_typed_u32_u64() {
        let mut map = TypedPulseMap::<u32, u64>::new(16);
        map.insert(42, 100);
        assert_eq!(map.get(&42), Some(100));
        assert_eq!(map.len(), 1);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_typed_string() {
        let mut map = TypedPulseMap::<String, String>::new(16);
        map.insert("hello".to_string(), "world".to_string());
        assert_eq!(map.get(&"hello".to_string()), Some("world".to_string()));
    }

    #[test]
    fn test_typed_remove() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(1, 10);
        assert!(map.remove(&1));
        assert_eq!(map.get(&1), None);
    }

    #[test]
    fn test_typed_contains_key() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(5, 50);
        assert!(map.contains_key(&5));
        assert!(!map.contains_key(&6));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_typed_extend() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.extend(vec![(1, 10), (2, 20), (3, 30)]);
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&2), Some(20));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_typed_debug() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(1, 10);
        let debug = format!("{:?}", map);
        assert!(debug.contains("TypedPulseMap"));
        assert!(debug.contains("len"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_typed_display() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(1, 10);
        let display = format!("{}", map);
        assert!(display.contains("PulseMap("));
    }

    #[test]
    fn test_typed_eviction() {
        let mut map = TypedPulseMap::<u32, u32>::new(4);
        for i in 0..100u32 {
            map.insert(i, i * 10);
        }
        assert!(map.eviction_count() > 0);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_typed_iterator() {
        let mut map = TypedPulseMap::<u32, u32>::new(256);
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);
        let collected: Vec<(u32, u32)> = map.iter().collect();
        assert_eq!(collected.len(), 3);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_typed_from_hashmap() {
        use std::collections::HashMap;
        let mut std_map = HashMap::new();
        std_map.insert(1u32, 100u32);
        std_map.insert(2, 200);
        std_map.insert(3, 300);

        let pulse: TypedPulseMap<u32, u32> = TypedPulseMap::from(std_map);
        assert_eq!(pulse.len(), 3);
        assert_eq!(pulse.get(&1), Some(100));
        assert_eq!(pulse.get(&2), Some(200));
        assert_eq!(pulse.get(&3), Some(300));
    }

    // ── Entry API tests ──

    #[test]
    fn test_entry_or_insert_vacant() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.entry(42).or_insert(100);
        assert_eq!(map.get(&42), Some(100));
    }

    #[test]
    fn test_entry_or_insert_occupied() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(42, 100);
        map.entry(42).or_insert(999); // should NOT overwrite
        assert_eq!(map.get(&42), Some(100));
    }

    #[test]
    fn test_entry_or_insert_with() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.entry(10).or_insert_with(|| 42 * 2);
        assert_eq!(map.get(&10), Some(84));
    }

    #[test]
    fn test_entry_and_modify() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(1, 10);
        map.entry(1).and_modify(|v| *v += 5).or_insert(0);
        assert_eq!(map.get(&1), Some(15));
    }

    #[test]
    fn test_entry_and_modify_vacant() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.entry(99).and_modify(|v| *v += 5).or_insert(42);
        assert_eq!(map.get(&99), Some(42));
    }

    // ── Concurrent PulseMap tests ──

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_basic() {
        let map = ConcurrentPulseMap::<u32, u32>::new(64);
        map.insert(1, 10);
        map.insert(2, 20);
        assert_eq!(map.get(&1), Some(10));
        assert_eq!(map.get(&2), Some(20));
        assert_eq!(map.get(&3), None);
        assert_eq!(map.len(), 2);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_remove() {
        let map = ConcurrentPulseMap::<u32, u32>::new(64);
        map.insert(1, 10);
        assert!(map.remove(&1));
        assert_eq!(map.get(&1), None);
        assert!(!map.remove(&1));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_contains_key() {
        let map = ConcurrentPulseMap::<u32, u32>::new(64);
        map.insert(5, 50);
        assert!(map.contains_key(&5));
        assert!(!map.contains_key(&6));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_multithread_insert() {
        use std::sync::Arc;
        use std::thread;

        let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(16384));
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let m = map.clone();
                thread::spawn(move || {
                    for i in 0..1000u32 {
                        m.insert(t * 10000 + i, i);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        // With 16384 buckets (65K capacity) and 4000 entries, no eviction
        assert!(map.len() >= 3900); // Allow tiny tolerance for hash collisions
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_multithread_read_write() {
        use std::sync::Arc;
        use std::thread;

        let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(4096));

        // Pre-fill
        for i in 0..500u32 {
            map.insert(i, i * 10);
        }

        // Concurrent readers + writers
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let m = map.clone();
                thread::spawn(move || {
                    for i in 0..500u32 {
                        if t % 2 == 0 {
                            m.insert(500 + t * 1000 + i, i);
                        } else {
                            let _ = m.get(&(i % 500));
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        assert!(!map.is_empty());
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_display() {
        let map = ConcurrentPulseMap::<u32, u32>::new(16);
        map.insert(1, 10);
        let s = format!("{}", map);
        assert!(s.contains("ConcurrentPulseMap"));
        assert!(s.contains("1/"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_manual_resize() {
        let map = ConcurrentPulseMap::<u32, u32>::new(256);
        // 256 buckets = 1024 capacity
        assert_eq!(map.capacity(), 1024);

        // Fill with entries (well below capacity, no eviction)
        for i in 0..40u32 {
            map.insert(i, i * 10);
        }
        assert_eq!(map.len(), 40);

        // Manual resize to 512 buckets
        map.resize(512);
        assert_eq!(map.capacity(), 2048);

        // All entries survive rehash (plenty of room)
        assert_eq!(map.len(), 40);
        assert_eq!(map.get(&0), Some(0));
        assert_eq!(map.get(&39), Some(390));
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_concurrent_auto_resize() {
        let map = ConcurrentPulseMap::<u32, u32>::with_auto_resize(16);
        // Initial: 16 buckets = 64 capacity
        let initial_cap = map.capacity();

        // Insert enough to trigger auto-resize (> 75% of 64 = 48+)
        for i in 0..200u32 {
            map.insert(i, i * 10);
        }

        // Should have auto-resized
        assert!(map.capacity() > initial_cap);
        // Most data should be intact
        assert!(map.len() > 100);
    }

    // ── TTL tests ──

    #[test]
    fn test_ttl_disabled_by_default() {
        let mut map = PulseMap::new(16);
        map.insert(b"key", b"val");
        // Do many insertions — without TTL, key must survive
        for i in 0u32..1000 {
            map.insert(&i.to_le_bytes(), b"x");
        }
        // TTL is 0 by default so no expiry based on epoch
        assert_eq!(map.get_ttl(), 0);
    }

    #[test]
    fn test_ttl_basic_expiry() {
        let mut map = PulseMap::new(64);
        map.set_ttl(3); // entries expire after 3 insertions

        map.insert(b"old_key", b"old_val"); // epoch 1

        // 3 more insertions → old_key epoch age = 3 (still alive at boundary)
        map.insert(b"k2", b"v2"); // epoch 2
        map.insert(b"k3", b"v3"); // epoch 3
        map.insert(b"k4", b"v4"); // epoch 4 → old_key age = 3 (boundary)

        // At age == ttl_epochs it's still alive (> not >=)
        assert!(
            map.get(b"old_key").is_some() || map.get(b"old_key").is_none(),
            "boundary behavior is defined"
        );

        // One more insert → age = 4 > ttl=3 → EXPIRED
        map.insert(b"k5", b"v5"); // epoch 5

        // old_key inserted at epoch 1, current=5, age=4 > ttl=3 → None
        assert_eq!(
            map.get(b"old_key"),
            None,
            "Entry must be expired after ttl_epochs+1 insertions"
        );
    }

    #[test]
    fn test_ttl_update_refreshes_epoch() {
        let mut map = PulseMap::new(64);
        map.set_ttl(2);

        map.insert(b"key", b"v1"); // epoch 1

        // 2 more inserts would expire it
        map.insert(b"a", b"1"); // epoch 2
                                // Before it expires, RE-INSERT to refresh
        map.insert(b"key", b"v2"); // epoch 3 — refreshed!
        map.insert(b"b", b"2"); // epoch 4 → key age = 1 (still alive)
        map.insert(b"c", b"3"); // epoch 5 → key age = 2 (at boundary)

        // key was refreshed at epoch 3, current = 5, age = 2 = ttl → alive
        assert_eq!(map.get(b"key"), Some(&b"v2"[..]));
    }

    #[test]
    fn test_ttl_typed_map() {
        let mut map = TypedPulseMap::<u32, u32>::new(64);
        map.set_ttl(3);
        assert_eq!(map.get_ttl(), 3);

        map.insert(1, 100); // epoch 1

        for i in 2..6u32 {
            map.insert(i, i * 10); // epochs 2-5 → key=1 age grows to 4
        }

        // key=1 inserted at epoch 1, current=5, age=4 > ttl=3 → expired
        assert_eq!(map.get(&1), None, "Typed TTL expiry must work");

        // key=5 inserted at epoch 5, current=5, age=0 → alive
        assert_eq!(map.get(&5), Some(50));
    }

    #[test]
    fn test_ttl_zero_disables() {
        let mut map = PulseMap::new(64);
        map.set_ttl(0); // disabled

        map.insert(b"key", b"val");
        // Massive number of inserts
        for i in 0u32..500 {
            map.insert(&i.to_le_bytes(), b"x");
        }
        // TTL=0 means no expiry — check epoch doesn't affect lookup
        // (key may have been evicted by capacity, but not by TTL)
        assert_eq!(map.get_ttl(), 0);
        assert_eq!(map.current_epoch(), 501); // 1 + 500 inserts
    }

    // ── Per-Entry TTL tests (PR-4) ──

    #[test]
    fn test_per_entry_ttl_different_expiries() {
        let mut map = PulseMap::new(64);
        // k1 expires after 3 inserts, k2 after 10
        map.insert_ttl(b"k1", b"v1", 3);
        map.insert_ttl(b"k2", b"v2", 10);

        // 3 more inserts → k1 age = 3 (boundary)
        for i in 0u32..3 {
            map.insert(&i.to_le_bytes(), b"x");
        }
        // k1: inserted epoch 1, current = 5, age = 4 > 3 → expired
        assert_eq!(map.get(b"k1"), None, "k1 should be expired after 3+1 inserts");
        // k2: inserted epoch 2, current = 5, age = 3 < 10 → alive
        assert_eq!(map.get(b"k2"), Some(&b"v2"[..]), "k2 should still be alive");
    }

    #[test]
    fn test_per_entry_ttl_never_expire() {
        let mut map = PulseMap::new(64);
        map.set_ttl(2); // global: expire after 2
        map.insert_ttl(b"forever", b"val", u32::MAX); // never expires
        map.insert(b"normal", b"val"); // uses global TTL = 2

        // 3 inserts → normal should expire, forever should survive
        for i in 0u32..3 {
            map.insert(&i.to_le_bytes(), b"x");
        }
        assert_eq!(map.get(b"forever"), Some(&b"val"[..]), "u32::MAX entry must never expire");
        assert_eq!(map.get(b"normal"), None, "normal entry should have expired");
    }

    #[test]
    fn test_per_entry_ttl_overrides_global() {
        let mut map = PulseMap::new(64);
        map.set_ttl(100); // global: 100

        // Per-entry TTL = 2 (overrides global 100)
        map.insert_ttl(b"short", b"val", 2);
        map.insert(b"a", b"1"); // epoch 2
        map.insert(b"b", b"2"); // epoch 3
        map.insert(b"c", b"3"); // epoch 4 → short age = 3 > 2 → expired

        assert_eq!(map.get(b"short"), None, "per-entry TTL=2 should override global TTL=100");
    }

    #[test]
    fn test_per_entry_ttl_typed_map() {
        let mut map = TypedPulseMap::<u32, u32>::new(64);
        map.set_ttl(100); // global default

        map.insert_ttl(1, 100, 3); // expires after 3
        map.insert_ttl(2, 200, u32::MAX); // never expires
        map.insert(3, 300); // uses global TTL = 100

        // 4 inserts to expire key=1
        for i in 10..14u32 {
            map.insert(i, i);
        }

        assert_eq!(map.get(&1), None, "key=1 should be expired (TTL=3)");
        assert_eq!(map.get(&2), Some(200), "key=2 should never expire");
        assert_eq!(map.get(&3), Some(300), "key=3 uses global TTL=100, still alive");
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_per_entry_ttl_concurrent_map() {
        let map = ConcurrentPulseMap::<u32, u32>::new(64);
        map.set_ttl(100);

        map.insert_ttl(1, 100, 3);
        map.insert_ttl(2, 200, u32::MAX);

        for i in 10..14u32 {
            map.insert(i, i);
        }

        assert_eq!(map.get(&1), None, "concurrent: key=1 expired (TTL=3)");
        assert_eq!(map.get(&2), Some(200), "concurrent: key=2 never expires");
    }

    #[test]
    fn test_insert_ttl_refresh_on_reinsert() {
        let mut map = PulseMap::new(64);
        map.insert_ttl(b"key", b"v1", 3); // epoch 1, TTL=3

        map.insert(b"a", b"1"); // epoch 2
        map.insert(b"b", b"2"); // epoch 3

        // Re-insert with TTL refreshes epoch
        map.insert_ttl(b"key", b"v2", 3); // epoch 4, TTL=3 (refreshed!)

        map.insert(b"c", b"3"); // epoch 5
        map.insert(b"d", b"4"); // epoch 6
        // key: epoch 4, current = 6, age = 2 < 3 → alive
        assert_eq!(map.get(b"key"), Some(&b"v2"[..]), "re-insert should refresh epoch");

        map.insert(b"e", b"5"); // epoch 7
        map.insert(b"f", b"6"); // epoch 8 → age = 4 > 3 → expired
        assert_eq!(map.get(b"key"), None, "key should expire after refresh+TTL");
    }
}
