// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! loom model for the `AccessBuffer` ring buffer.
//!
//! The buffer is documented as lossy: a full buffer drops events. What must never
//! happen is a *wrong* delivery — a torn or fabricated event, or the same event
//! handed to `drain` twice. That is what these models check.
//!
//! Includes a two-slot repeated-generation model. Payload publication and slot
//! reuse are synchronized by per-slot sequences, independently of head/tail claims.
//!
//! Run:
//!   RUSTFLAGS="--cfg loom" cargo test --test loom_access_buffer
//!
//! Without `--cfg loom` the whole file compiles away to an empty test binary.

#![cfg(loom)]

use loom::sync::{Arc, Mutex};
use pulse_map::AccessBuffer;

/// The two events the producers push. Distinct in both fields so a mix-up is visible.
const PUSHED: [(usize, u8); 2] = [(1, 0), (2, 1)];

/// Two producers racing one drain.
///
/// Racing producers can fail a reservation CAS and drop their event. At least one
/// succeeds when the buffer is empty; no successful reservation is overwritten.
#[test]
fn concurrent_push_drain_never_delivers_a_wrong_event() {
    loom::model(|| {
        let buf = Arc::new(AccessBuffer::new(64));

        let producers: Vec<_> = PUSHED
            .iter()
            .map(|&(bucket, slot)| {
                let buf = buf.clone();
                loom::thread::spawn(move || buf.push(bucket, slot))
            })
            .collect();

        // The model thread is the consumer, so it drains while the pushes are in
        // flight. Three threads keeps loom inside its four-thread budget.
        let mut delivered = Vec::new();
        buf.drain(4, |bucket, slot| delivered.push((bucket, slot)));

        for p in producers {
            p.join().unwrap();
        }

        // Whatever the racing drain missed is still queued.
        buf.drain(4, |bucket, slot| delivered.push((bucket, slot)));

        for event in &delivered {
            assert!(
                PUSHED.contains(event),
                "drained {event:?}, which was never pushed (delivered {delivered:?})"
            );
            assert_eq!(
                delivered.iter().filter(|e| *e == event).count(),
                1,
                "{event:?} delivered more than once (delivered {delivered:?})"
            );
        }
        assert!(
            delivered.len() <= PUSHED.len(),
            "more events out than in: {delivered:?}"
        );
        // Two pushes into a 64-slot buffer: the buffer is never full, so at least
        // one event has to survive. Zero would mean a published event stayed
        // invisible to a drain that had already seen the head advance.
        assert!(
            !delivered.is_empty(),
            "both events vanished from a buffer that was never full"
        );
    });
}

/// A drain that overlaps a push must not hand back the `EMPTY` sentinel as an event.
///
/// Single producer this time, so the buffer is used exactly as designed: the event
/// has to come out exactly once, whatever order the drain and the push interleave in.
#[test]
fn single_producer_event_is_never_torn() {
    loom::model(|| {
        let buf = Arc::new(AccessBuffer::new(64));

        let producer = {
            let buf = buf.clone();
            loom::thread::spawn(move || assert!(buf.push(7, 3), "empty buffer refused a push"))
        };

        let mut delivered = Vec::new();
        buf.drain(4, |bucket, slot| delivered.push((bucket, slot)));
        producer.join().unwrap();
        buf.drain(4, |bucket, slot| delivered.push((bucket, slot)));

        assert_eq!(
            delivered,
            vec![(7, 3)],
            "single push must be delivered exactly once"
        );
    });
}

/// Two consumers claim individual ready positions; a failed CAS may stop a drain.
/// Once both finish, flush remaining events and require exactly-once delivery.
///
/// Three threads total (model + two drains), inside loom's four-thread budget.
#[test]
fn concurrent_drains_deliver_every_event_exactly_once() {
    loom::model(|| {
        let buf = Arc::new(AccessBuffer::new(64));
        for &(bucket, slot) in PUSHED.iter() {
            buf.push(bucket, slot);
        }

        let delivered = Arc::new(Mutex::new(Vec::new()));
        let drains: Vec<_> = (0..2)
            .map(|_| {
                let buf = buf.clone();
                let delivered = delivered.clone();
                loom::thread::spawn(move || {
                    buf.drain(4, |bucket, slot| {
                        delivered.lock().unwrap().push((bucket, slot));
                    })
                })
            })
            .collect();
        for d in drains {
            d.join().unwrap();
        }

        buf.drain(4, |bucket, slot| {
            delivered.lock().unwrap().push((bucket, slot))
        });

        let mut events = delivered.lock().unwrap().clone();
        events.sort();
        assert_eq!(
            events,
            vec![(1, 0), (2, 1)],
            "both events must be delivered exactly once across two concurrent drains"
        );
    });
}

#[test]
fn tiny_ring_reuse_never_duplicates_or_fabricates_events() {
    let mut model = loom::model::Builder::new();
    model.preemption_bound = Some(2);
    model.check(|| {
        let buf = Arc::new(AccessBuffer::new(2));
        assert!(buf.push(10, 0));
        assert!(buf.push(11, 1));
        let producer = {
            let buf = buf.clone();
            loom::thread::spawn(move || {
                let mut accepted = vec![];
                for key in 12..14 {
                    if buf.push(key, 2) {
                        accepted.push((key, 2));
                    }
                    loom::thread::yield_now();
                }
                accepted
            })
        };
        let consumer = {
            let buf = buf.clone();
            loom::thread::spawn(move || {
                let mut events = vec![];
                for _ in 0..3 {
                    buf.drain(1, |b, s| {
                        events.push((b, s));
                        loom::thread::yield_now();
                    });
                }
                events
            })
        };
        let mut events = vec![];
        buf.drain(2, |b, s| events.push((b, s)));
        let mut accepted = producer.join().unwrap();
        accepted.extend([(10, 0), (11, 1)]);
        events.extend(consumer.join().unwrap());
        buf.drain(4, |b, s| events.push((b, s)));
        events.sort();
        accepted.sort();
        assert_eq!(events, accepted);
    });
}
