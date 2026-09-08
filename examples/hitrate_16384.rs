// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Hit-rate at equal real capacity (16,384 entries) for the read-path issue:
//! TypedPulseMap's get() promotes priority inline; ConcurrentPulseMap's get()
//! defers via AccessBuffer (drained in insert, after the fix on this branch).
//! The peek() control is the same map with reads deliberately not weighted —
//! the measured "reads carry no weight" baseline.
//!
//! Both constructors take bucket counts; capacity = buckets × 4, so new(4096)
//! gives each map 16,384 entries.

use pulse_map::{ConcurrentPulseMap, TypedPulseMap};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Zipf};

const BUCKETS: usize = 4096; // 16,384 entries
const KEY_SPACE: usize = 163_840; // 10× capacity, same ratio as readratio_isolated
const ZIPF_EXP: f64 = 1.3;
const TOTAL_OPS: usize = 2_000_000;
const TRIALS: usize = 5;

fn mean_std(vals: &[f64]) -> (f64, f64) {
    let n = vals.len() as f64;
    let mean = vals.iter().sum::<f64>() / n;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}

fn trial(seed: u64) -> (f64, f64, f64) {
    let mut rng = StdRng::seed_from_u64(seed);
    let zipf = Zipf::new(KEY_SPACE as u64, ZIPF_EXP).unwrap();

    let mut typed = TypedPulseMap::<u32, u32>::new(BUCKETS);
    let sync = ConcurrentPulseMap::<u32, u32>::new(BUCKETS);
    let ctrl = ConcurrentPulseMap::<u32, u32>::new(BUCKETS); // peek() control

    let mut t_hits = 0u64;
    let mut s_hits = 0u64;
    let mut c_hits = 0u64;

    for _ in 0..TOTAL_OPS {
        let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
        let read = rng.gen_bool(0.99);

        if read {
            if typed.get(&key).is_some() {
                t_hits += 1;
            } else {
                typed.insert(key, key);
            }
            if sync.get(&key).is_some() {
                s_hits += 1;
            } else {
                sync.insert(key, key);
            }
            if ctrl.peek(&key).is_some() {
                c_hits += 1;
            } else {
                ctrl.insert(key, key);
            }
        } else {
            typed.insert(key, key);
            sync.insert(key, key);
            ctrl.insert(key, key);
        }
    }

    let ops = TOTAL_OPS as f64;
    (
        100.0 * t_hits as f64 / ops,
        100.0 * s_hits as f64 / ops,
        100.0 * c_hits as f64 / ops,
    )
}

fn main() {
    let mut typed = Vec::new();
    let mut sync = Vec::new();
    let mut ctrl = Vec::new();
    for t in 0..TRIALS {
        let (a, b, c) = trial(t as u64);
        typed.push(a);
        sync.push(b);
        ctrl.push(c);
    }

    let (tm, ts) = mean_std(&typed);
    let (sm, ss) = mean_std(&sync);
    let (cm, cs) = mean_std(&ctrl);
    println!(
        "capacity=16384 key_space={} zipf={} ops={} trials={} read_ratio=0.99",
        KEY_SPACE, ZIPF_EXP, TOTAL_OPS, TRIALS
    );
    println!(
        "{:<44} {:>8.3}% ± {:.3}%",
        "TypedPulseMap get (inline on_access)", tm, ts
    );
    println!(
        "{:<44} {:>8.3}% ± {:.3}%",
        "ConcurrentPulseMap get (drained)", sm, ss
    );
    println!(
        "{:<44} {:>8.3}% ± {:.3}%",
        "ConcurrentPulseMap peek (no read weight)", cm, cs
    );
}
