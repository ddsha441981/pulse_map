// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Iterators for PulseMap.
//!
//! - `RawIter` — iterates over `(&[u8], &[u8])` raw key-value pairs
//! - `TypedIter` — iterates over `(K, V)` deserialized pairs

use crate::engine::hash::HashResult;
use crate::raw::PulseMapRaw;
use crate::{PulseKey, PulseValue, SlotState};

/// Iterator over raw `(&[u8], &[u8])` key-value pairs in a PulseMapRaw.
pub struct RawIter<'a> {
    map: &'a PulseMapRaw,
    bucket_idx: usize,
    slot_idx: u8,
}

impl<'a> RawIter<'a> {
    pub fn new(map: &'a PulseMapRaw) -> Self {
        Self {
            map,
            bucket_idx: 0,
            slot_idx: 0,
        }
    }
}

impl<'a> Iterator for RawIter<'a> {
    type Item = (&'a [u8], &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let num_buckets = self.map.num_buckets();

        while self.bucket_idx < num_buckets {
            let bucket = &self.map.buckets[self.bucket_idx];

            while self.slot_idx < 4 {
                let idx = self.slot_idx;
                self.slot_idx += 1;

                if bucket.meta.get_state(idx) == SlotState::Full {
                    let slot = &bucket.slots[idx as usize];
                    // Create a dummy HashResult for get_value — only mode check matters
                    let hr = HashResult {
                        h1: 0,
                        h2: 0,
                        ext_fp_hi: 0,
                        ext_fp: 0,
                    };

                    let key = if slot.get_mode() == 0 {
                        slot.inline_key()
                    } else {
                        slot.slab_key()
                    };

                    let value = slot.get_value(&hr);
                    return Some((key, value));
                }
            }

            self.bucket_idx += 1;
            self.slot_idx = 0;
        }

        None
    }
}

/// Iterator over typed `(K, V)` pairs in a TypedPulseMap.
pub struct TypedIter<'a, K: PulseKey, V: PulseValue> {
    raw_iter: RawIter<'a>,
    _marker: core::marker::PhantomData<(K, V)>,
}

impl<'a, K: PulseKey, V: PulseValue> TypedIter<'a, K, V> {
    pub fn new(map: &'a PulseMapRaw) -> Self {
        Self {
            raw_iter: RawIter::new(map),
            _marker: core::marker::PhantomData,
        }
    }
}

impl<'a, K: PulseKey, V: PulseValue> Iterator for TypedIter<'a, K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (kb, vb) = self.raw_iter.next()?;
            if let (Some(k), Some(v)) = (K::from_bytes(kb), V::from_bytes(vb)) {
                return Some((k, v));
            }
            // Skip entries that fail deserialization
        }
    }
}
