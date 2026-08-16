// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! AccessBuffer — Lock-free lossy ring buffer for deferred LRU/LFU updates.
//!
//! When a `get()` finds a cache hit, instead of mutating the MetaWord's priority
//! inline (which requires exclusive bucket access), it pushes an access event into
//! this buffer. The buffer is drained during `insert()` operations, piggybacking
//! eviction tracking on write operations without needing a background thread.
//!
//! The buffer is **lossy**: if it's full, new events are silently dropped.
//! This is acceptable because LRU/LFU accuracy degrades gracefully — a missed
//! access event only slightly delays priority promotion, and under high load
//! (when the buffer fills), eviction accuracy matters less than read latency.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// A single access event: which bucket and slot were accessed.
#[repr(C)]
struct AccessEvent {
    /// Packed: bucket_idx (upper 24 bits) | slot_idx (lower 8 bits)
    data: AtomicU32,
}

impl AccessEvent {
    const EMPTY: u32 = u32::MAX;

    fn new_empty() -> Self {
        Self {
            data: AtomicU32::new(Self::EMPTY),
        }
    }

    #[inline]
    fn pack(bucket_idx: usize, slot_idx: u8) -> u32 {
        ((bucket_idx as u32) << 8) | (slot_idx as u32)
    }

    #[allow(dead_code)]
    #[inline]
    fn unpack(val: u32) -> (usize, u8) {
        let bucket_idx = (val >> 8) as usize;
        let slot_idx = (val & 0xFF) as u8;
        (bucket_idx, slot_idx)
    }
}

/// Lock-free lossy ring buffer for access events.
///
/// Capacity is fixed at creation and must be a power of 2.
pub struct AccessBuffer {
    buffer: Vec<AccessEvent>,
    mask: usize,
    head: AtomicUsize, // write position
    tail: AtomicUsize, // read position
}

impl AccessBuffer {
    /// Create a new access buffer with the given capacity (rounded up to power of 2).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64).next_power_of_two();
        let buffer = (0..cap).map(|_| AccessEvent::new_empty()).collect();
        Self {
            buffer,
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Push an access event. Returns true if pushed, false if buffer is full (event dropped).
    /// This is called from the hot `get()` path and must be extremely fast.
    #[inline]
    pub fn push(&self, bucket_idx: usize, slot_idx: u8) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);

        // Buffer full — drop the event (lossy)
        if head.wrapping_sub(tail) > self.mask {
            return false;
        }

        let idx = head & self.mask;
        let packed = AccessEvent::pack(bucket_idx, slot_idx);
        self.buffer[idx].data.store(packed, Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    /// Drain up to `max_events` from the buffer. Calls `f(bucket_idx, slot_idx)` for each.
    /// This can be called from `insert()` or a background maintenance path.
    #[allow(dead_code)]
    #[inline]
    pub fn drain(&self, max_events: usize, mut f: impl FnMut(usize, u8)) {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let available = head.wrapping_sub(tail);
        let to_drain = available.min(max_events);

        for i in 0..to_drain {
            let idx = (tail.wrapping_add(i)) & self.mask;
            let val = self.buffer[idx].data.load(Ordering::Relaxed);
            if val != AccessEvent::EMPTY {
                let (bucket_idx, slot_idx) = AccessEvent::unpack(val);
                f(bucket_idx, slot_idx);
                self.buffer[idx]
                    .data
                    .store(AccessEvent::EMPTY, Ordering::Relaxed);
            }
        }

        self.tail
            .store(tail.wrapping_add(to_drain), Ordering::Release);
    }
}

// Safety: AccessBuffer uses only atomics, safe to share across threads.
unsafe impl Send for AccessBuffer {}
unsafe impl Sync for AccessBuffer {}
