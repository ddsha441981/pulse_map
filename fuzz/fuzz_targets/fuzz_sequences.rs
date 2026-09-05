// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.
//
// Fuzz target: fuzz_sequences
//
// Interprets arbitrary bytes as a tagged operation stream over PulseMap,
// exercising insert / get / remove / peek / insert_ttl / TTL-epoch-advance
// in random sequences and verifying key invariants after every operation.
//
// Every operation is mirrored into a shadow HashMap. The map is free to lose an
// entry (eviction, TTL expiry), but a lookup that *does* hit must return the
// value the shadow map recorded — see `check_hit`.
//
// Run:
//   cargo fuzz run fuzz_sequences
//   cargo fuzz run fuzz_sequences -- -max_total_time=60

#![no_main]

use libfuzzer_sys::fuzz_target;
use pulse_map::PulseMap;
use std::collections::HashMap;

// ── Constants ───────────────────────────────────────────────────────────────

/// Number of buckets in the fuzz map (small → lots of evictions).
const NUM_BUCKETS: usize = 16;

/// Maximum key/value byte length we'll pull from the input stream.
const MAX_KEY_LEN: usize = 32;
const MAX_VAL_LEN: usize = 32;

// ── Helper: read a length-prefixed byte slice from the stream ────────────────

/// Consume `[len_byte, ...len_byte bytes...]` from `data`.
/// Returns `(slice, remainder)`, or `None` if the stream is exhausted.
fn read_bytes(data: &[u8], max_len: usize) -> Option<(&[u8], &[u8])> {
    let (&len_byte, rest) = data.split_first()?;
    let len = (len_byte as usize) % (max_len + 1); // clamp to [0, max_len]
    if rest.len() < len {
        return None;
    }
    let (bytes, remainder) = rest.split_at(len);
    Some((bytes, remainder))
}

// ── Operation tags ───────────────────────────────────────────────────────────

/// Tagged operation encoded in the fuzz byte stream.
#[derive(Debug)]
enum Op<'a> {
    /// insert(key, value) using the map's default TTL.
    Insert { key: &'a [u8], value: &'a [u8] },
    /// get(key) — check result is consistent with internal state.
    Get { key: &'a [u8] },
    /// remove(key).
    Remove { key: &'a [u8] },
    /// peek(key) — non-mutating lookup.
    Peek { key: &'a [u8] },
    /// insert_ttl(key, value, ttl) — per-entry TTL override.
    InsertTtl {
        key: &'a [u8],
        value: &'a [u8],
        ttl: u64,
    },
    /// Advance the epoch counter by inserting a series of throwaway keys
    /// to trigger lazy TTL expiry.
    AdvanceEpoch { steps: u8 },
}

/// Parse one `Op` from the front of `data`.
/// Returns `(op, remainder)` or `None` if there are not enough bytes.
fn parse_op<'a>(data: &'a [u8]) -> Option<(Op<'a>, &'a [u8])> {
    let (&tag, rest) = data.split_first()?;

    match tag % 6 {
        // ── 0: Insert ──────────────────────────────────────────────────────
        0 => {
            let (key, rest) = read_bytes(rest, MAX_KEY_LEN)?;
            let (value, rest) = read_bytes(rest, MAX_VAL_LEN)?;
            Some((Op::Insert { key, value }, rest))
        }
        // ── 1: Get ─────────────────────────────────────────────────────────
        1 => {
            let (key, rest) = read_bytes(rest, MAX_KEY_LEN)?;
            Some((Op::Get { key }, rest))
        }
        // ── 2: Remove ──────────────────────────────────────────────────────
        2 => {
            let (key, rest) = read_bytes(rest, MAX_KEY_LEN)?;
            Some((Op::Remove { key }, rest))
        }
        // ── 3: InsertTtl ───────────────────────────────────────────────────
        3 => {
            let (key, rest) = read_bytes(rest, MAX_KEY_LEN)?;
            let (value, rest) = read_bytes(rest, MAX_VAL_LEN)?;
            // Consume 1 byte as the TTL value (0 = use default, 1-254 = N epochs)
            let (&ttl_byte, rest) = rest.split_first()?;
            Some((
                Op::InsertTtl {
                    key,
                    value,
                    ttl: ttl_byte as u64,
                },
                rest,
            ))
        }
        // ── 4: AdvanceEpoch ────────────────────────────────────────────────
        4 => {
            let (&steps, rest) = rest.split_first()?;
            // Clamp to [1, 16] to avoid unbounded work
            let steps = steps % 16 + 1;
            Some((Op::AdvanceEpoch { steps }, rest))
        }
        // ── 5: Peek ────────────────────────────────────────────────────────
        _ => {
            let (key, rest) = read_bytes(rest, MAX_KEY_LEN)?;
            Some((Op::Peek { key }, rest))
        }
    }
}

// ── Shadow-map verification ─────────────────────────────────────────────────

/// Assert the one thing eviction leaves intact: a hit must be the *right* hit.
///
/// `None` is always legal. The entry may have been evicted when its bucket
/// overflowed, or expired via TTL, and neither is observable from outside — the
/// caller cannot predict which of the 4 slots in a bucket loses. A non-`None`
/// result is a different matter: it pins down that the fingerprint matched the
/// right key, that the slab index pointed at the right entry, and that no
/// eviction corrupted a neighbouring slot.
fn check_hit(shadow: &HashMap<Vec<u8>, Vec<u8>>, key: &[u8], got: Option<&[u8]>) {
    let Some(value) = got else { return };
    match shadow.get(key) {
        Some(expected) => assert_eq!(
            value,
            expected.as_slice(),
            "wrong value for key {:?}: map returned {:?}, last insert was {:?}",
            key,
            value,
            expected
        ),
        None => panic!(
            "map returned {:?} for key {:?}, which was never inserted (or was removed)",
            value, key
        ),
    }
}

// ── Fuzz entry point ────────────────────────────────────────────────────────

fuzz_target!(|data: &[u8]| {
    let mut map = PulseMap::new(NUM_BUCKETS);

    // Shadow copy of every insert and remove. Only ever consulted to check a
    // hit, never to demand one. Built per input, so it stays as small as the
    // input is short.
    let mut shadow: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

    let mut remaining = data;

    while let Some((op, rest)) = parse_op(remaining) {
        remaining = rest;

        match op {
            // ── Insert ──────────────────────────────────────────────────────
            Op::Insert { key, value } => {
                map.insert(key, value);
                shadow.insert(key.to_vec(), value.to_vec());
            }

            // ── Get ─────────────────────────────────────────────────────────
            Op::Get { key } => {
                // Must never panic, and must never hand back a wrong value.
                check_hit(&shadow, key, map.get(key));
            }

            // ── Remove ──────────────────────────────────────────────────────
            Op::Remove { key } => {
                let was_present = map.remove(key);
                shadow.remove(key);

                // remove() is the one operation whose *absence* is checkable:
                // nothing may resurrect the key.
                assert!(
                    map.get(key).is_none(),
                    "get() returned Some after remove() for key {:?} (was_present={})",
                    key,
                    was_present
                );
            }

            // ── Peek ────────────────────────────────────────────────────────
            Op::Peek { key } => {
                let peek_result = map.peek(key);
                let get_result = map.get(key);

                // Both must agree on presence.
                assert_eq!(
                    peek_result.is_some(),
                    get_result.is_some(),
                    "peek() and get() disagree on key {:?}: peek={:?} get={:?}",
                    key,
                    peek_result.map(|b| b.len()),
                    get_result.map(|b| b.len()),
                );
                check_hit(&shadow, key, peek_result);
                check_hit(&shadow, key, get_result);
            }

            // ── InsertTtl ───────────────────────────────────────────────────
            Op::InsertTtl { key, value, ttl } => {
                map.insert_ttl(key, value, ttl);
                shadow.insert(key.to_vec(), value.to_vec());
            }

            // ── AdvanceEpoch ────────────────────────────────────────────────
            Op::AdvanceEpoch { steps } => {
                // Dummy inserts to advance the internal epoch counter, which
                // triggers lazy TTL expiry on subsequent reads. They are real
                // inserts, so they are mirrored too.
                for i in 0..steps {
                    let dummy_key = [0xAA, i, 0xFF];
                    let dummy_val = [0x00];
                    map.insert(&dummy_key, &dummy_val);
                    shadow.insert(dummy_key.to_vec(), dummy_val.to_vec());
                }
            }
        }

        // ── Global invariants (checked after every operation) ────────────────

        // len() must be consistent with capacity()
        assert!(
            map.len() <= map.capacity(),
            "len={} capacity={}",
            map.len(),
            map.capacity()
        );

        // load_factor() must be in [0.0, 1.0]
        let lf = map.load_factor();
        assert!(
            (0.0..=1.0).contains(&lf),
            "load_factor out of range: {}",
            lf
        );
    }
});
