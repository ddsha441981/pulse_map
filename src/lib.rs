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

mod core;
mod raw;
mod iter;
mod traits;
#[cfg(all(target_arch = "x86_64", feature = "simd"))]
mod simd;

// ── Re-exports ──
pub use crate::core::meta::MetaWord;
pub use crate::core::slot::Slot;
pub use crate::core::bucket::Bucket;
pub use raw::PulseMapRaw;
pub use iter::{RawIter, TypedIter};

// ── SlotState (shared by core and raw) ──

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
    pub(crate) fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => SlotState::Empty,
            1 => SlotState::Full,
            2 => SlotState::Deleted,
            3 => SlotState::Tombstone,
            _ => unreachable!(),
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
    fn to_bytes(&self) -> [u8; 1] { [*self] }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.first().copied() }
}

impl PulseKey for u16 {
    type Bytes = [u8; 2];
    fn to_bytes(&self) -> [u8; 2] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(u16::from_le_bytes) }
}

impl PulseKey for u32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(u32::from_le_bytes) }
}

impl PulseKey for u64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(u64::from_le_bytes) }
}

impl PulseKey for i32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(i32::from_le_bytes) }
}

impl PulseKey for i64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(i64::from_le_bytes) }
}

impl PulseKey for String {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> { self.as_bytes().to_vec() }
    fn from_bytes(b: &[u8]) -> Option<Self> { std::str::from_utf8(b).ok().map(String::from) }
}

impl PulseKey for Vec<u8> {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> { self.clone() }
    fn from_bytes(b: &[u8]) -> Option<Self> { Some(b.to_vec()) }
}

impl<const N: usize> PulseKey for [u8; N] {
    type Bytes = [u8; N];
    fn to_bytes(&self) -> [u8; N] { *self }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok() }
}

// ── Built-in PulseValue implementations ──

impl PulseValue for u8 {
    type Bytes = [u8; 1];
    fn to_bytes(&self) -> [u8; 1] { [*self] }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.first().copied() }
}

impl PulseValue for u16 {
    type Bytes = [u8; 2];
    fn to_bytes(&self) -> [u8; 2] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(u16::from_le_bytes) }
}

impl PulseValue for u32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(u32::from_le_bytes) }
}

impl PulseValue for u64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(u64::from_le_bytes) }
}

impl PulseValue for i32 {
    type Bytes = [u8; 4];
    fn to_bytes(&self) -> [u8; 4] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(i32::from_le_bytes) }
}

impl PulseValue for i64 {
    type Bytes = [u8; 8];
    fn to_bytes(&self) -> [u8; 8] { self.to_le_bytes() }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.try_into().ok().map(i64::from_le_bytes) }
}

impl PulseValue for String {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> { self.as_bytes().to_vec() }
    fn from_bytes(b: &[u8]) -> Option<Self> { std::str::from_utf8(b).ok().map(String::from) }
}

impl PulseValue for Vec<u8> {
    type Bytes = Vec<u8>;
    fn to_bytes(&self) -> Vec<u8> { self.clone() }
    fn from_bytes(b: &[u8]) -> Option<Self> { Some(b.to_vec()) }
}

impl PulseValue for bool {
    type Bytes = [u8; 1];
    fn to_bytes(&self) -> [u8; 1] { [*self as u8] }
    fn from_bytes(b: &[u8]) -> Option<Self> { b.first().map(|&v| v != 0) }
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
    _marker: std::marker::PhantomData<(K, V)>,
}

impl<K: PulseKey, V: PulseValue> TypedPulseMap<K, V> {
    /// Create a new TypedPulseMap with the given number of buckets.
    pub fn new(num_buckets: usize) -> Self {
        Self {
            raw: PulseMapRaw::new(num_buckets),
            _marker: std::marker::PhantomData,
        }
    }

    /// Insert a key-value pair.
    pub fn insert(&mut self, key: K, value: V) {
        let kb = key.to_bytes();
        let vb = value.to_bytes();
        self.raw.insert(kb.as_ref(), vb.as_ref());
    }

    /// Look up a key. Returns the deserialized value if found.
    pub fn get(&self, key: &K) -> Option<V> {
        let kb = key.to_bytes();
        self.raw.get(kb.as_ref()).and_then(|vb| V::from_bytes(vb))
    }

    /// Look up without updating priority.
    pub fn peek(&self, key: &K) -> Option<V> {
        let kb = key.to_bytes();
        self.raw.peek(kb.as_ref()).and_then(|vb| V::from_bytes(vb))
    }

    /// Remove a key.
    pub fn remove(&mut self, key: &K) -> bool {
        let kb = key.to_bytes();
        self.raw.remove(kb.as_ref())
    }

    /// Check if a key exists.
    pub fn contains_key(&self, key: &K) -> bool {
        let kb = key.to_bytes();
        self.raw.peek(kb.as_ref()).is_some()
    }

    /// Iterate over all (key, value) pairs.
    pub fn iter(&self) -> TypedIter<'_, K, V> {
        TypedIter::new(&self.raw)
    }

    #[inline]
    pub fn len(&self) -> usize { self.raw.len() }

    #[inline]
    pub fn is_empty(&self) -> bool { self.raw.is_empty() }

    #[inline]
    pub fn capacity(&self) -> usize { self.raw.capacity() }

    #[inline]
    pub fn load_factor(&self) -> f64 { self.raw.load_factor() }

    #[inline]
    pub fn eviction_count(&self) -> usize { self.raw.eviction_count() }

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
            Some(val) => Entry::Occupied(OccupiedEntry { map: self, key, value: val }),
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
            if map.get(&i.to_le_bytes()).is_some() { hits += 1; }
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
        assert_eq!(std::mem::size_of::<Bucket>(), 64, "Bucket must be exactly 64 bytes");
    }

    // ── Typed API tests ──

    #[test]
    fn test_typed_u32_u64() {
        let mut map = TypedPulseMap::<u32, u64>::new(16);
        map.insert(42, 100);
        assert_eq!(map.get(&42), Some(100));
        assert_eq!(map.len(), 1);
    }

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

    #[test]
    fn test_typed_extend() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.extend(vec![(1, 10), (2, 20), (3, 30)]);
        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&2), Some(20));
    }

    #[test]
    fn test_typed_debug() {
        let mut map = TypedPulseMap::<u32, u32>::new(16);
        map.insert(1, 10);
        let debug = format!("{:?}", map);
        assert!(debug.contains("TypedPulseMap"));
        assert!(debug.contains("len"));
    }

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

    #[test]
    fn test_typed_iterator() {
        let mut map = TypedPulseMap::<u32, u32>::new(256);
        map.insert(1, 10);
        map.insert(2, 20);
        map.insert(3, 30);
        let collected: Vec<(u32, u32)> = map.iter().collect();
        assert_eq!(collected.len(), 3);
    }

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
}
