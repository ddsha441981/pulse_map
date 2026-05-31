// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Trait implementations for TypedPulseMap.
//!
//! - `Debug` — struct info (len, capacity, load%, evictions)
//! - `Display` — human-readable summary
//! - `Extend<(K, V)>` — bulk insertion from iterators
//! - `From<HashMap<K, V>>` — convert std::HashMap to TypedPulseMap

use crate::{PulseKey, PulseValue, TypedPulseMap};
#[cfg(not(feature = "std"))]
use alloc::format;

// ── Debug trait ──

impl<K: PulseKey + core::fmt::Debug, V: PulseValue + core::fmt::Debug> core::fmt::Debug
for TypedPulseMap<K, V>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TypedPulseMap")
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .field(
                "load_factor",
                &format!("{:.1}%", self.load_factor() * 100.0),
            )
            .field("evictions", &self.eviction_count())
            .finish()
    }
}

// ── Display trait ──

impl<K: PulseKey, V: PulseValue> core::fmt::Display for TypedPulseMap<K, V> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PulseMap({}/{} entries, {:.1}% load, {} evictions)",
            self.len(),
            self.capacity(),
            self.load_factor() * 100.0,
            self.eviction_count()
        )
    }
}

// ── Extend trait ──

impl<K: PulseKey, V: PulseValue> Extend<(K, V)> for TypedPulseMap<K, V> {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

// ── From<HashMap> trait (requires std) ──

#[cfg(feature = "std")]
impl<K: PulseKey + std::hash::Hash + Eq, V: PulseValue> From<std::collections::HashMap<K, V>>
for TypedPulseMap<K, V>
{
    /// Convert a std::HashMap into a TypedPulseMap.
    ///
    /// The number of buckets is auto-calculated for ~65% load factor.
    fn from(map: std::collections::HashMap<K, V>) -> Self {
        // Target ~65% load factor: buckets = entries / (4 slots * 0.65)
        let num_buckets = (map.len() / 3).max(16);
        let mut pulse = TypedPulseMap::new(num_buckets);
        for (k, v) in map {
            pulse.insert(k, v);
        }
        pulse
    }
}

// Note: Index<&K> trait is intentionally NOT implemented.
// PulseMap stores bytes, not typed values. `get()` returns `Option<V>` (deserialized copy),
// but Index requires `&V` (a reference). Use `.get()` instead of `map[&key]`.
