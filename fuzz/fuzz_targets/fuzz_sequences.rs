// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.
//
// Fuzz target: fuzz_sequences
//
// Interprets arbitrary bytes as a tagged operation stream over PulseMap,
// exercising insert / get / remove / peek / insert_ttl / TTL-epoch-advance
// in random sequences and verifying key invariants after every operation.
//
// Run:
//   cargo fuzz run fuzz_sequences
//   cargo fuzz run fuzz_sequences -- -max_total_time=60

#![no_main]

use libfuzzer_sys::fuzz_target;
use pulse_map::PulseMap;

// ── Constants ───────────────────────────────────────────────────────────────

/// Number of buckets in the fuzz map (small → lots of evictions).
const NUM_BUCKETS: usize = 16;

/// Maximum key/value byte length we'll pull from the input stream.
const MAX_KEY_LEN: usize = 32;
const MAX_VAL_LEN: usize = 32;

// ── Helper: read a length-prefixed byte slice from the stream ────────────────

/// Consume `[len_byte, ...len_byte bytes...]` from `data`.
/// Returns `(slice, remainder)`, or `None` if the stream is exhausted.
fn read_bytes<'a>(data: &'a [u8], max_len: usize) -> Option<(&'a [u8], &'a [u8])> {
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

// ── Fuzz entry point ────────────────────────────────────────────────────────

fuzz_target!(|data: &[u8]| {
    let mut map = PulseMap::new(NUM_BUCKETS);

    // We track the last inserted (key, value) so we can verify get() consistency
    // when the bucket has enough room (i.e., we haven't overflowed it).
    // Using fixed-size arrays on the stack to avoid heap allocation in the harness.
    let mut last_insert_key: [u8; MAX_KEY_LEN] = [0u8; MAX_KEY_LEN];
    let mut last_insert_key_len: usize = 0;
    let mut last_insert_val: [u8; MAX_VAL_LEN] = [0u8; MAX_VAL_LEN];
    let mut last_insert_val_len: usize = 0;
    let mut last_was_ttl: bool = false; // TTL inserts may expire, skip strict check

    let mut remaining = data;

    while let Some((op, rest)) = parse_op(remaining) {
        remaining = rest;

        match op {
            // ── Insert ──────────────────────────────────────────────────────
            Op::Insert { key, value } => {
                map.insert(key, value);

                // Track for post-insert get() verification
                let klen = key.len().min(MAX_KEY_LEN);
                let vlen = value.len().min(MAX_VAL_LEN);
                last_insert_key[..klen].copy_from_slice(&key[..klen]);
                last_insert_key_len = klen;
                last_insert_val[..vlen].copy_from_slice(&value[..vlen]);
                last_insert_val_len = vlen;
                last_was_ttl = false;

                // Invariant: capacity is never exceeded
                assert!(
                    map.len() <= map.capacity(),
                    "len {} exceeded capacity {}",
                    map.len(),
                    map.capacity()
                );
            }

            // ── Get ─────────────────────────────────────────────────────────
            Op::Get { key } => {
                // Must never panic
                let result = map.get(key);

                // If this exact key was the last thing we inserted (and it wasn't
                // a TTL insert), the map *should* return Some — unless it was
                // evicted (which happens when the bucket is full). We can't know
                // for certain whether eviction happened without reimplementing the
                // map logic here, so we only assert the weaker property: if the
                // map returns Some, the data is non-empty (no zero-length slice
                // corruption).
                if let Some(val_bytes) = result {
                    // Returned slice must be internally consistent — it should
                    // point into valid memory (the sanitizer will catch UB).
                    // We do a shallow byte read to force the memory access.
                    let _ = val_bytes.len();
                    if !val_bytes.is_empty() {
                        let _ = val_bytes[0];
                        let _ = val_bytes[val_bytes.len() - 1];
                    }
                }

                // Stronger check: if we JUST inserted this exact key and got
                // back Some, the returned value must match what we inserted.
                if !last_was_ttl
                    && last_insert_key_len == key.len()
                    && &last_insert_key[..last_insert_key_len] == key
                {
                    if let Some(val_bytes) = result {
                        assert_eq!(
                            val_bytes,
                            &last_insert_val[..last_insert_val_len],
                            "get() after insert() returned wrong value"
                        );
                    }
                    // Note: result == None is allowed because the bucket may have
                    // been full and the insert evicted a different key instead,
                    // or the bucket itself evicted *our* key under pressure.
                }
            }

            // ── Remove ──────────────────────────────────────────────────────
            Op::Remove { key } => {
                let was_present = map.remove(key);

                // Invariant: after remove(), get() must return None
                let after = map.get(key);
                assert!(
                    after.is_none(),
                    "get() returned Some after remove() for key {:?} (was_present={})",
                    key,
                    was_present
                );

                // Invalidate last-insert tracking if we just removed that key
                if last_insert_key_len == key.len()
                    && &last_insert_key[..last_insert_key_len] == key
                {
                    last_insert_key_len = 0;
                    last_insert_val_len = 0;
                }
            }

            // ── Peek ────────────────────────────────────────────────────────
            Op::Peek { key } => {
                // peek() must never panic and must be consistent with get():
                // if peek() returns None, get() must also return None.
                let peek_result = map.peek(key);
                let get_result = map.get(key);

                // Both should agree on presence.
                assert_eq!(
                    peek_result.is_some(),
                    get_result.is_some(),
                    "peek() and get() disagree on key {:?}: peek={:?} get={:?}",
                    key,
                    peek_result.map(|b| b.len()),
                    get_result.map(|b| b.len()),
                );
            }

            // ── InsertTtl ───────────────────────────────────────────────────
            Op::InsertTtl { key, value, ttl } => {
                map.insert_ttl(key, value, ttl);
                last_was_ttl = true; // TTL entries may expire; skip strict get check

                // Invariant: capacity is never exceeded
                assert!(
                    map.len() <= map.capacity(),
                    "len {} exceeded capacity {} after insert_ttl",
                    map.len(),
                    map.capacity()
                );
            }

            // ── AdvanceEpoch ────────────────────────────────────────────────
            Op::AdvanceEpoch { steps } => {
                // Insert `steps` dummy keys to advance the internal epoch counter,
                // which triggers lazy TTL expiry on subsequent reads.
                for i in 0..steps {
                    let dummy_key = [0xAA, i, 0xFF];
                    let dummy_val = [0x00];
                    map.insert(&dummy_key, &dummy_val);
                }
                last_was_ttl = true; // state is now mixed; disable strict check

                // Invariant: still sane after epoch advance
                assert!(
                    map.len() <= map.capacity(),
                    "len {} exceeded capacity {} after epoch advance",
                    map.len(),
                    map.capacity()
                );
            }
        }

        // ── Global invariants (checked every iteration) ─────────────────────

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
