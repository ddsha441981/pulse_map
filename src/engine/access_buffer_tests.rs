use super::AccessBuffer;
use std::collections::HashSet;
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

fn overlapping_wrap(capacity: usize) {
    let buf = Arc::new(AccessBuffer::new(capacity));
    for i in 0..capacity {
        assert!(buf.push(i, 0));
    }
    let (at_first, first) = mpsc::channel();
    let (refilled, refill) = mpsc::channel();
    let (at_second, second) = mpsc::channel();
    let (finished, finish) = mpsc::channel();
    let worker_buf = buf.clone();
    let worker = std::thread::spawn(move || {
        let mut delivered = Vec::new();
        worker_buf.drain(64, |bucket, slot| {
            delivered.push((bucket, slot));
            if delivered.len() == 1 {
                at_first.send(()).unwrap();
                refill.recv_timeout(Duration::from_secs(30)).unwrap();
            } else if delivered.len() == 2 {
                at_second.send(()).unwrap();
                finish.recv_timeout(Duration::from_secs(30)).unwrap();
            }
        });
        delivered
    });
    first.recv_timeout(Duration::from_secs(30)).unwrap();
    for i in capacity..capacity + 64 {
        let _ = buf.push(i, 0);
    }
    refilled.send(()).unwrap();
    second.recv_timeout(Duration::from_secs(30)).unwrap();
    let mut delivered = Vec::new();
    for _ in 0..capacity / 64 + 2 {
        buf.drain(64, |b, s| delivered.push((b, s)));
    }
    finished.send(()).unwrap();
    delivered.extend(worker.join().unwrap());
    let mut seen = HashSet::new();
    for event in delivered {
        assert!(event.0 < capacity + 64 && event.1 == 0);
        assert!(seen.insert(event), "duplicate delivery: {event:?}");
    }
}

#[test]
fn overlapping_wrap_small() {
    overlapping_wrap(64);
}

#[test]
#[cfg_attr(miri, ignore)] // same interleaving is exercised at 64 slots above
fn overlapping_wrap_production_geometry() {
    overlapping_wrap(4096);
}

#[test]
fn sequential_wrap_and_full_buffer() {
    let b = AccessBuffer::new(64);
    for round in 0..3 {
        for i in 0..64 {
            assert!(b.push(round * 64 + i, (i % 4) as u8));
        }
        assert!(!b.push(999, 0));
        let mut events = Vec::new();
        b.drain(64, |k, s| events.push((k, s)));
        assert_eq!(
            events,
            (0..64)
                .map(|i| (round * 64 + i, (i % 4) as u8))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn concurrent_producers_consumers_only_deliver_unique_real_events() {
    let b = Arc::new(AccessBuffer::new(64));
    let start = Arc::new(Barrier::new(4));
    let mut handles = Vec::new();
    for producer in 0..2 {
        let (b, start) = (b.clone(), start.clone());
        handles.push(std::thread::spawn(move || {
            start.wait();
            let mut accepted = Vec::new();
            for i in 0..128 {
                let event = producer * 128 + i;
                if b.push(event, (producer + 1) as u8) {
                    accepted.push((event, (producer + 1) as u8));
                }
            }
            (accepted, Vec::new())
        }));
    }
    for _ in 0..2 {
        let (b, start) = (b.clone(), start.clone());
        handles.push(std::thread::spawn(move || {
            start.wait();
            let mut delivered = Vec::new();
            for _ in 0..32 {
                b.drain(8, |k, s| delivered.push((k, s)));
            }
            (Vec::new(), delivered)
        }));
    }
    let (mut accepted, mut delivered) = (HashSet::new(), Vec::new());
    for h in handles {
        let (a, d) = h.join().unwrap();
        accepted.extend(a);
        delivered.extend(d);
    }
    b.drain(64, |k, s| delivered.push((k, s)));
    let mut seen = HashSet::new();
    for event in delivered {
        assert!(accepted.contains(&event));
        assert!(seen.insert(event));
    }
}

#[test]
fn counter_wrap_and_large_bucket_indices() {
    use super::Ordering;
    let b = AccessBuffer::new(64);
    // Start at an empty generation just before usize wrap.
    let start = usize::MAX - 63;
    b.head.store(start, Ordering::Relaxed);
    b.tail.store(start, Ordering::Relaxed);
    for i in 0..64 {
        b.buffer[i]
            .sequence
            .store(start.wrapping_add(i), Ordering::Relaxed);
    }
    for round in 0..2 {
        for i in 0..64 {
            assert!(b.push((1 << 24) + round * 64 + i, 3));
        }
        let mut events = Vec::new();
        b.drain(64, |k, s| events.push((k, s)));
        assert_eq!(
            events,
            (0..64)
                .map(|i| ((1 << 24) + round * 64 + i, 3))
                .collect::<Vec<_>>()
        );
    }
    assert!(!b.push(usize::MAX, 0));
    assert!(!b.push(0, 4));
}

#[test]
fn panic_in_callback_does_not_poison_or_redeliver() {
    let b = AccessBuffer::new(64);
    assert!(b.push(1, 0));
    let _ = std::panic::catch_unwind(|| b.drain(1, |_, _| panic!("callback")));
    assert!(b.push(2, 0));
    let mut events = Vec::new();
    b.drain(64, |k, s| events.push((k, s)));
    assert_eq!(events, vec![(2, 0)]);
}
