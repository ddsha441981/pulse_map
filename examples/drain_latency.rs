// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

//! Insert-path cost of the drain wiring: get() pushes access events, and every
//! insert() drains up to DRAIN_BATCH of them before its eviction decision.
//!
//! Three configs, 4 threads on one shared map, same keys and op mix:
//!  - mixed-get : 99% get()    → inserts drain events (drain cost included)
//!  - mixed-peek: 99% peek()   → buffer stays empty (drain fast-path only)
//!  - pure-write: 100% insert  → no reads at all
//!
//! mixed-get vs mixed-peek isolates what the drain adds per op; pure-write
//! checks the empty-buffer fast path is free.

use pulse_map::ConcurrentPulseMap;
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_distr::{Distribution, Zipf};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

const BUCKETS: usize = 4096; // 16,384 entries
const KEY_SPACE: u64 = 163_840;
const ZIPF_EXP: f64 = 1.3;
const THREADS: usize = 4;
const OPS_PER_THREAD: usize = 500_000;
const TRIALS: usize = 3;

#[derive(Clone)]
enum ReadMode {
    Get,
    Peek,
    None,
}

fn run(read_mode: ReadMode, seed: u64) -> f64 {
    let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(BUCKETS));
    let barrier = Arc::new(std::sync::Barrier::new(THREADS));
    let start = Arc::new(std::sync::Mutex::new(None::<Instant>));

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            let start = Arc::clone(&start);
            let read_mode = read_mode.clone();
            thread::spawn(move || {
                let mut rng = StdRng::seed_from_u64(seed * 1000 + t as u64);
                let zipf = Zipf::new(KEY_SPACE, ZIPF_EXP).unwrap();
                barrier.wait();
                if t == 0 {
                    *start.lock().unwrap() = Some(Instant::now());
                }
                for _ in 0..OPS_PER_THREAD {
                    let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
                    let do_read = match read_mode {
                        ReadMode::None => false,
                        _ => rng.gen_bool(0.99),
                    };
                    if do_read {
                        let hit = match read_mode {
                            ReadMode::Get => map.get(&key).is_some(),
                            ReadMode::Peek => map.peek(&key).is_some(),
                            ReadMode::None => unreachable!(),
                        };
                        if !hit {
                            map.insert(key, key);
                        }
                    } else {
                        map.insert(key, key);
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
    let elapsed = start.lock().unwrap().take().unwrap().elapsed();
    (THREADS * OPS_PER_THREAD) as f64 / elapsed.as_secs_f64()
}

fn main() {
    let configs: [(&str, ReadMode); 3] = [
        ("mixed 99% get    (drain active)", ReadMode::Get),
        ("mixed 99% peek   (buffer empty)", ReadMode::Peek),
        ("pure insert      (no reads)    ", ReadMode::None),
    ];

    println!(
        "threads={} ops/thread={} key_space={} zipf={} trials={}",
        THREADS, OPS_PER_THREAD, KEY_SPACE, ZIPF_EXP, TRIALS
    );

    for (name, mode) in configs {
        let mut rates = Vec::new();
        for t in 0..TRIALS {
            rates.push(run(mode.clone(), t as u64));
        }
        let mean = rates.iter().sum::<f64>() / rates.len() as f64;
        let best = rates.iter().cloned().fold(f64::MIN, f64::max);
        println!("{name}  {:>8.2} Mops/s (best {best:.2})", mean / 1e6);
    }
}
