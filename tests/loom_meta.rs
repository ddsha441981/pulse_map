// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! loom model for the `MetaWord::on_access` CAS loop.
//!
//! `on_access` takes `&self` and rewrites the whole 64-bit word: it boosts the
//! accessed slot's frequency, pins its recency to 7, and decays every other Full
//! slot's recency by one. That makes it a read-modify-write on shared state from
//! the lock-free read path, so a plain load/store would silently drop a
//! concurrent update. loom explores every interleaving and proves it does not.
//!
//! Run:
//!   RUSTFLAGS="--cfg loom" cargo test --test loom_meta
//!
//! Without `--cfg loom` the whole file compiles away to an empty test binary.

#![cfg(loom)]

use loom::sync::Arc;
use pulse_map::{MetaWord, SlotState};

/// Priority layout (`meta.rs`): `freq[6:3]` (4 bits) + `recency[2:0]` (3 bits).
fn freq(prio: u8) -> u8 {
    (prio >> 3) & 0x0F
}

fn recency(prio: u8) -> u8 {
    prio & 0x07
}

/// Two Full slots, freq 0 and recency 1 (what `on_insert` leaves behind).
fn two_full_slots() -> Arc<MetaWord> {
    let meta = Arc::new(MetaWord::empty());
    for slot in 0..2 {
        meta.set_state(slot, SlotState::Full);
        meta.on_insert(slot);
    }
    meta
}

/// Two threads touching *different* slots.
///
/// Each `on_access` commits with one CAS, so whatever the interleaving, the two
/// calls have to serialize: both slots end at freq 1, the slot whose access
/// landed last keeps recency 7, and the other one is decayed to 6. A lost update
/// shows up as a freq of 0 or two slots claiming recency 7.
#[test]
fn on_access_distinct_slots_serializes() {
    loom::model(|| {
        let meta = two_full_slots();

        let handles: Vec<_> = (0..2u8)
            .map(|slot| {
                let meta = meta.clone();
                loom::thread::spawn(move || meta.on_access(slot))
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let p0 = meta.get_priority(0);
        let p1 = meta.get_priority(1);

        for slot in 0..2 {
            assert_eq!(
                meta.get_state(slot),
                SlotState::Full,
                "slot {slot} state corrupted, priorities were {p0:#04x}/{p1:#04x}"
            );
        }
        assert_eq!(freq(p0), 1, "lost the increment on slot 0 (p1={p1:#04x})");
        assert_eq!(freq(p1), 1, "lost the increment on slot 1 (p0={p0:#04x})");

        let (r0, r1) = (recency(p0), recency(p1));
        assert!(
            (r0, r1) == (7, 6) || (r0, r1) == (6, 7),
            "recency pair ({r0}, {r1}) matches no serialization of the two accesses"
        );

        // Untouched slots stay untouched: the decay loop only walks Full slots.
        assert_eq!(meta.get_state(2), SlotState::Empty);
        assert_eq!(meta.get_state(3), SlotState::Empty);
    });
}

/// Two threads hammering the *same* slot — the sharpest no-lost-update check.
///
/// The outcome is fully determined: freq counts both hits, the slot holds the
/// maximum recency, and slot 1 is decayed twice (1 → 0, then held at 0).
#[test]
fn on_access_same_slot_counts_every_hit() {
    loom::model(|| {
        let meta = two_full_slots();

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let meta = meta.clone();
                loom::thread::spawn(move || meta.on_access(0))
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        let p0 = meta.get_priority(0);
        assert_eq!(freq(p0), 2, "two hits on slot 0 counted as {}", freq(p0));
        assert_eq!(recency(p0), 7, "accessed slot must hold max recency");

        let p1 = meta.get_priority(1);
        assert_eq!(freq(p1), 0, "slot 1 was never accessed");
        assert_eq!(
            recency(p1),
            0,
            "slot 1 should be decayed twice, floored at 0"
        );
    });
}
