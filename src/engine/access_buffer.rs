// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Bounded, lossy MPMC access tracking for deferred eviction updates.
//!
//! Each slot has a sequence number: a producer reserves its head position, writes
//! the payload, then publishes it with Release. A consumer acquires that sequence,
//! claims the tail position, reads the payload and releases the slot for its next
//! generation BEFORE invoking the callback. Advancing tail alone never permits reuse.
//!
//! Push and pop each attempt one CAS; contention/full/unpublished head means skip,
//! not spin. A stalled producer can delay consumption, so this is not a formal
//! lock-free FIFO. Eviction tracking tolerates loss; key/value storage is separate.

#[cfg(not(loom))]
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicUsize, Ordering};

struct AccessEvent {
    sequence: AtomicUsize,
    // Bucket index in upper bits, slot index (0..4) in low two bits. Using usize
    // avoids the old 24-bit bucket-index truncation on large 64-bit maps.
    data: AtomicUsize,
}

/// A bounded sequence-tagged queue. Operations never wait for another producer.
pub struct AccessBuffer {
    buffer: Vec<AccessEvent>,
    mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl AccessBuffer {
    pub fn new(capacity: usize) -> Self {
        let min = if cfg!(loom) { 2 } else { 64 };
        let capacity = capacity.max(min).next_power_of_two();
        let buffer = (0..capacity)
            .map(|i| AccessEvent {
                sequence: AtomicUsize::new(i),
                data: AtomicUsize::new(0),
            })
            .collect();
        Self {
            buffer,
            mask: capacity - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Try to record an event. False means full/contention or an invalid event.
    #[inline]
    pub fn push(&self, bucket_idx: usize, slot_idx: u8) -> bool {
        if slot_idx >= 4 || bucket_idx > usize::MAX >> 2 {
            return false;
        }
        let head = self.head.load(Ordering::Relaxed);
        let slot = &self.buffer[head & self.mask];
        if slot.sequence.load(Ordering::Acquire) != head {
            return false;
        }
        if self
            .head
            .compare_exchange(
                head,
                head.wrapping_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }
        slot.data
            .store((bucket_idx << 2) | slot_idx as usize, Ordering::Relaxed);
        slot.sequence.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    #[inline]
    fn pop(&self) -> Option<(usize, u8)> {
        let tail = self.tail.load(Ordering::Relaxed);
        let slot = &self.buffer[tail & self.mask];
        if slot.sequence.load(Ordering::Acquire) != tail.wrapping_add(1) {
            return None;
        }
        if self
            .tail
            .compare_exchange(
                tail,
                tail.wrapping_add(1),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return None;
        }
        let data = slot.data.load(Ordering::Relaxed);
        slot.sequence
            .store(tail.wrapping_add(self.buffer.len()), Ordering::Release);
        Some((data >> 2, (data & 3) as u8))
    }

    /// Consume at most max_events, stopping on contention or unpublished/empty head.
    /// A slot is released before f: slow callbacks cannot race its next generation.
    #[inline]
    pub fn drain(&self, max_events: usize, mut f: impl FnMut(usize, u8)) {
        for _ in 0..max_events {
            let Some((bucket, slot)) = self.pop() else {
                break;
            };
            f(bucket, slot);
        }
    }

    /// Discard queued indices before rehash. Caller holds the map's exclusive
    /// resize lock, so no map operation can concurrently push or drain events.
    pub(crate) fn clear(&self) {
        self.drain(self.buffer.len(), |_, _| {});
    }
}

#[cfg(all(test, not(loom)))]
#[path = "access_buffer_tests.rs"]
mod tests;
