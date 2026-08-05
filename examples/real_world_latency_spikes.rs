//! Statistical version: runs many trials per cache and reports
//! mean ± stddev of p50/p99/max, instead of a single trial's numbers.
//!
//! Why this exists: v2 showed PulseMap vs QuickCache p99 within ~1.4x and
//! max where QuickCache was actually *lower* in one run. A single trial
//! can't tell you if that's real or noise. This version runs NUM_TRIALS
//! independent trials per cache (fresh cache instance each time) and
//! reports mean/stddev so you can see whether the gap is bigger than the
//! run-to-run variance — that's the difference between a real result and
//! a coin flip that happened to land your way once.
//!
//! Cargo.toml:
//!   pulse_map = { path = "../pulse_map" }
//!   moka = { version = "0.12", features = ["sync"] }
//!   quick_cache = "0.6"
//!   lru = "0.12"
//!
//! Run:
//!   cargo run --release --example real_world_latency_spikes_v3

use lru::LruCache;
use moka::sync::Cache as MokaCache;
use pulse_map::ShardedPulseMap;
use quick_cache::sync::Cache as QuickCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAX_CAPACITY: usize = 100_000;
const NUM_INSERTS: u32 = 1_000_000;
const NUM_THREADS: u32 = 8;
const NUM_TRIALS: usize = 15;

fn run_trial<F>(threads: u32, per_thread_inserts: u32, insert_fn: F) -> Vec<Duration>
where
    F: FnMut(u32) + Send + Clone + 'static,
{
    let mut handles = Vec::with_capacity(threads as usize);
    let barrier = Arc::new(Barrier::new(threads as usize));

    for t in 0..threads {
        let mut f = insert_fn.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let base = t * per_thread_inserts;
            let mut local = Vec::with_capacity(per_thread_inserts as usize);
            barrier.wait();
            for i in 0..per_thread_inserts {
                let key = base + i;
                let t0 = Instant::now();
                f(key);
                local.push(t0.elapsed());
            }
            local
        }));
    }

    let mut all = Vec::with_capacity((threads * per_thread_inserts) as usize);
    for h in handles {
        all.extend(h.join().expect("thread panicked"));
    }
    all
}

fn percentiles_ns(latencies: &[Duration]) -> (f64, f64, f64) {
    let mut ns: Vec<u128> = latencies.iter().map(|d| d.as_nanos()).collect();
    ns.sort_unstable();
    let len = ns.len();
    let p50 = ns[len * 50 / 100] as f64;
    let p99 = ns[len * 99 / 100] as f64;
    let max = ns[len - 1] as f64;
    (p50, p99, max)
}

fn mean_std(vals: &[f64]) -> (f64, f64) {
    let n = vals.len() as f64;
    let mean = vals.iter().sum::<f64>() / n;
    let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, variance.sqrt())
}

fn fmt_ns(ns: f64) -> String {
    if ns >= 1_000_000.0 {
        format!("{:.3}ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.3}µs", ns / 1_000.0)
    } else {
        format!("{:.0}ns", ns)
    }
}

struct Stats {
    name: &'static str,
    total_mean: f64,
    p50_mean: f64,
    p50_std: f64,
    p99_mean: f64,
    p99_std: f64,
    max_mean: f64,
    max_std: f64,
}

fn bench<F, G>(name: &'static str, mut make_insert_fn: F) -> Stats
where
    F: FnMut() -> G,
    G: FnMut(u32) + Send + Clone + 'static,
{
    let per_thread = NUM_INSERTS / NUM_THREADS;
    let mut totals = Vec::with_capacity(NUM_TRIALS);
    let mut p50s = Vec::with_capacity(NUM_TRIALS);
    let mut p99s = Vec::with_capacity(NUM_TRIALS);
    let mut maxs = Vec::with_capacity(NUM_TRIALS);

    for _ in 0..NUM_TRIALS {
        let insert_fn = make_insert_fn();
        let start = Instant::now();
        let latencies = run_trial(NUM_THREADS, per_thread, insert_fn);
        totals.push(start.elapsed().as_secs_f64() * 1000.0); // ms
        let (p50, p99, max) = percentiles_ns(&latencies);
        p50s.push(p50);
        p99s.push(p99);
        maxs.push(max);
    }

    let (total_mean, _) = mean_std(&totals);
    let (p50_mean, p50_std) = mean_std(&p50s);
    let (p99_mean, p99_std) = mean_std(&p99s);
    let (max_mean, max_std) = mean_std(&maxs);

    Stats {
        name,
        total_mean,
        p50_mean,
        p50_std,
        p99_mean,
        p99_std,
        max_mean,
        max_std,
    }
}

fn print_stats(s: &Stats) {
    println!(
        "{:<12} total={:>9.1}ms   p50={:>10} ± {:<9}  p99={:>10} ± {:<9}  max={:>10} ± {:<9}",
        s.name,
        s.total_mean,
        fmt_ns(s.p50_mean),
        fmt_ns(s.p50_std),
        fmt_ns(s.p99_mean),
        fmt_ns(s.p99_std),
        fmt_ns(s.max_mean),
        fmt_ns(s.max_std),
    );
}

/// Prints a verdict for a head-to-head pair: is the gap in p99 bigger than
/// the combined noise (stddev) of both? If yes, it's a real difference.
/// If the gap is smaller than ~1 combined stddev, call it a statistical tie.
fn verdict(a: &Stats, b: &Stats) {
    let gap = (a.p99_mean - b.p99_mean).abs();
    let combined_std = a.p99_std + b.p99_std;
    println!(
        "\n{} vs {} (p99): gap = {}, combined stddev = {}",
        a.name,
        b.name,
        fmt_ns(gap),
        fmt_ns(combined_std)
    );
    if combined_std == 0.0 || gap > combined_std {
        let winner = if a.p99_mean < b.p99_mean {
            a.name
        } else {
            b.name
        };
        println!(
            "  -> {winner} is reliably lower p99 across {NUM_TRIALS} trials (gap exceeds noise)."
        );
    } else {
        println!(
            "  -> statistical tie: gap is within run-to-run noise, don't claim a winner here."
        );
    }
}

fn main() {
    println!("⏱️  STATISTICAL MULTI-THREADED WRITE-PRESSURE BENCHMARK ⏱️\n");
    println!("{NUM_THREADS} threads, {NUM_INSERTS} inserts/trial, {NUM_TRIALS} independent trials/cache (fresh cache each trial).\n");
    println!("Running... this takes a while with {NUM_TRIALS} trials x 5 caches.\n");

    let pulse = bench("PulseMap", || {
        let map = Arc::new(ShardedPulseMap::<u32, u32>::new(MAX_CAPACITY / 64));
        move |k: u32| {
            map.insert(k, k);
        }
    });

    let moka = bench("Moka", || {
        let cache = Arc::new(
            MokaCache::builder()
                .max_capacity(MAX_CAPACITY as u64)
                .initial_capacity(MAX_CAPACITY)
                .build(),
        );
        move |k: u32| {
            cache.insert(k, k);
        }
    });

    let quick = bench("QuickCache", || {
        let cache = Arc::new(QuickCache::<u32, u32>::new(MAX_CAPACITY));
        move |k: u32| {
            cache.insert(k, k);
        }
    });

    let lru = bench("LRU", || {
        let cache = Arc::new(Mutex::new(LruCache::<u32, u32>::new(
            NonZeroUsize::new(MAX_CAPACITY).unwrap(),
        )));
        move |k: u32| {
            cache.lock().unwrap().put(k, k);
        }
    });

    let simple = bench("Simple", || {
        let cache = Arc::new(Mutex::new(HashMap::<u32, u32>::with_capacity(MAX_CAPACITY)));
        move |k: u32| {
            cache.lock().unwrap().insert(k, k);
        }
    });

    println!("📊 RESULTS — mean ± stddev across {NUM_TRIALS} trials:");
    println!("{}", "-".repeat(110));
    print_stats(&pulse);
    print_stats(&moka);
    print_stats(&quick);
    print_stats(&lru);
    print_stats(&simple);
    println!("{}", "-".repeat(110));

    println!("\n🔍 HEAD-TO-HEAD VERDICTS (is the difference bigger than the noise?):");
    verdict(&pulse, &moka);
    verdict(&pulse, &quick);
    verdict(&pulse, &lru);
    verdict(&pulse, &simple);
}

//============================================

// //! Multi-threaded, percentile-based latency benchmark.
// //!
// //! Fixes vs the v1 single-threaded benchmark:
// //!  - Multiple concurrent writer threads (exercises real "write pressure" /
// //!    queue backpressure, which is the actual claim being tested)
// //!  - Moka gets `initial_capacity` set so table-resize cost isn't mixed
// //!    into "eviction" cost
// //!  - Reports p50 / p95 / p99 / max instead of a single max sample
// //!  - Runs N trials per cache and reports the median trial (reduces noise
// //!    from a single unlucky scheduler hiccup)
// //!  - Includes LRU (lru crate, single-threaded, wrapped in Mutex) and a
// //!    naive Mutex<HashMap> "simple cache" baseline
// //!
// //! Add to Cargo.toml (dev-dependencies is fine if this stays an example):
// //!   pulse_map = { path = "../pulse_map" }   # or version = "..."
// //!   moka = { version = "0.12", features = ["sync"] }
// //!   quick_cache = "0.6"
// //!   lru = "0.12"
// //!
// //! Run with:
// //!   cargo run --release --example real_world_latency_spikes_v2

// use pulse_map::ShardedPulseMap;
// use moka::sync::Cache as MokaCache;
// use lru::LruCache;
// use quick_cache::sync::Cache as QuickCache;
// use std::collections::HashMap;
// use std::num::NonZeroUsize;
// use std::sync::{Arc, Mutex};
// use std::thread;
// use std::time::{Duration, Instant};

// const MAX_CAPACITY: usize = 100_000;
// const NUM_INSERTS: u32 = 1_000_000;
// const NUM_THREADS: u32 = 8;
// const NUM_TRIALS: usize = 5;

// /// Per-thread work: does its own timing, returns per-insert latencies.
// /// Keeping raw samples per thread avoids a shared Vec<Duration> becoming
// /// itself a contention point that pollutes the numbers.
// fn run_trial<F>(label: &str, threads: u32, per_thread_inserts: u32, insert_fn: F) -> Vec<Duration>
// where
//     F: FnMut(u32) + Send + Clone + 'static,
// {
//     let mut handles = Vec::with_capacity(threads as usize);
//     let barrier = Arc::new(std::sync::Barrier::new(threads as usize));

//     for t in 0..threads {
//         let mut f = insert_fn.clone();
//         let barrier = Arc::clone(&barrier);
//         handles.push(thread::spawn(move || {
//             let base = t * per_thread_inserts;
//             let mut local_latencies = Vec::with_capacity(per_thread_inserts as usize);
//             barrier.wait(); // start all threads together for real concurrent pressure
//             for i in 0..per_thread_inserts {
//                 let key = base + i;
//                 let t0 = Instant::now();
//                 f(key);
//                 local_latencies.push(t0.elapsed());
//             }
//             local_latencies
//         }));
//     }

//     let mut all_latencies = Vec::with_capacity((threads * per_thread_inserts) as usize);
//     for h in handles {
//         all_latencies.extend(h.join().expect(&format!("{label} thread panicked")));
//     }
//     all_latencies
// }

// fn percentiles(mut latencies: Vec<Duration>) -> (Duration, Duration, Duration, Duration) {
//     latencies.sort_unstable();
//     let len = latencies.len();
//     let p50 = latencies[len * 50 / 100];
//     let p95 = latencies[len * 95 / 100];
//     let p99 = latencies[len * 99 / 100];
//     let max = latencies[len - 1];
//     (p50, p95, p99, max)
// }

// /// Runs NUM_TRIALS trials, returns the trial whose p99 is the median p99
// /// across trials (reduces single-run noise without letting one lucky
// /// warm-cache run flatter the result).
// fn median_trial_by_p99(trials: Vec<Vec<Duration>>) -> Vec<Duration> {
//     let mut indexed: Vec<(usize, Duration)> = trials
//         .iter()
//         .enumerate()
//         .map(|(idx, t)| {
//             let mut sorted = t.clone();
//             sorted.sort_unstable();
//             (idx, sorted[sorted.len() * 99 / 100])
//         })
//         .collect();
//     indexed.sort_by_key(|(_, p99)| *p99);
//     let median_idx = indexed[indexed.len() / 2].0;
//     trials.into_iter().nth(median_idx).unwrap()
// }

// fn print_result(name: &str, latencies: &[Duration], total: Duration) {
//     let (p50, p95, p99, max) = percentiles(latencies.to_vec());
//     println!("{name:<14} total={total:>10.2?}  p50={p50:>9.2?}  p95={p95:>9.2?}  p99={p99:>9.2?}  max={max:>9.2?}");
// }

// fn main() {
//     println!("⏱️  MULTI-THREADED WRITE-PRESSURE LATENCY BENCHMARK ⏱️\n");
//     println!("{NUM_THREADS} concurrent writer threads, {NUM_INSERTS} total inserts, {NUM_TRIALS} trials/cache (median-by-p99 reported).\n");

//     let per_thread = NUM_INSERTS / NUM_THREADS;

//     // ---------- PulseMap ----------
//     let mut pulse_trials = Vec::with_capacity(NUM_TRIALS);
//     let mut pulse_total = Duration::ZERO;
//     for _ in 0..NUM_TRIALS {
//         let map = Arc::new(ShardedPulseMap::<u32, u32>::new(MAX_CAPACITY / 64));
//         let start = Instant::now();
//         let lat = run_trial("PulseMap", NUM_THREADS, per_thread, move |k| {
//             map.insert(k, k);
//         });
//         pulse_total = start.elapsed();
//         pulse_trials.push(lat);
//     }
//     let pulse_lat = median_trial_by_p99(pulse_trials);

//     // ---------- Moka (initial_capacity set so resize cost != eviction cost) ----------
//     let mut moka_trials = Vec::with_capacity(NUM_TRIALS);
//     let mut moka_total = Duration::ZERO;
//     for _ in 0..NUM_TRIALS {
//         let cache = Arc::new(
//             MokaCache::builder()
//                 .max_capacity(MAX_CAPACITY as u64)
//                 .initial_capacity(MAX_CAPACITY)
//                 .build(),
//         );
//         let start = Instant::now();
//         let lat = run_trial("Moka", NUM_THREADS, per_thread, move |k| {
//             cache.insert(k, k);
//         });
//         moka_total = start.elapsed();
//         moka_trials.push(lat);
//     }
//     let moka_lat = median_trial_by_p99(moka_trials);

//     // ---------- QuickCache (natively concurrent, no external Mutex needed —
//     // same fairness tier as PulseMap/Moka) ----------
//     let mut quick_trials = Vec::with_capacity(NUM_TRIALS);
//     let mut quick_total = Duration::ZERO;
//     for _ in 0..NUM_TRIALS {
//         let cache = Arc::new(QuickCache::<u32, u32>::new(MAX_CAPACITY));
//         let start = Instant::now();
//         let lat = run_trial("QuickCache", NUM_THREADS, per_thread, move |k| {
//             cache.insert(k, k);
//         });
//         quick_total = start.elapsed();
//         quick_trials.push(lat);
//     }
//     let quick_lat = median_trial_by_p99(quick_trials);

//     // ---------- LRU crate (single global lock — this is the realistic
//     // way to make a non-concurrent LRU thread-safe, and the lock
//     // contention itself is part of what we're measuring) ----------
//     let mut lru_trials = Vec::with_capacity(NUM_TRIALS);
//     let mut lru_total = Duration::ZERO;
//     for _ in 0..NUM_TRIALS {
//         let cache = Arc::new(Mutex::new(LruCache::<u32, u32>::new(
//             NonZeroUsize::new(MAX_CAPACITY).unwrap(),
//         )));
//         let start = Instant::now();
//         let lat = run_trial("LRU", NUM_THREADS, per_thread, move |k| {
//             cache.lock().unwrap().put(k, k);
//         });
//         lru_total = start.elapsed();
//         lru_trials.push(lat);
//     }
//     let lru_lat = median_trial_by_p99(lru_trials);

//     // ---------- Simple Mutex<HashMap> (no eviction at all — the naive
//     // baseline everyone reaches for first) ----------
//     let mut simple_trials = Vec::with_capacity(NUM_TRIALS);
//     let mut simple_total = Duration::ZERO;
//     for _ in 0..NUM_TRIALS {
//         let cache = Arc::new(Mutex::new(HashMap::<u32, u32>::with_capacity(MAX_CAPACITY)));
//         let start = Instant::now();
//         let lat = run_trial("Simple", NUM_THREADS, per_thread, move |k| {
//             cache.lock().unwrap().insert(k, k);
//         });
//         simple_total = start.elapsed();
//         simple_trials.push(lat);
//     }
//     let simple_lat = median_trial_by_p99(simple_trials);

//     // ---------- Report ----------
//     println!("📊 RESULTS (median trial by p99, {NUM_TRIALS} trials each):");
//     println!("{}", "-".repeat(80));
//     print_result("PulseMap", &pulse_lat, pulse_total);
//     print_result("Moka", &moka_lat, moka_total);
//     print_result("QuickCache", &quick_lat, quick_total);
//     print_result("LRU", &lru_lat, lru_total);
//     print_result("Simple", &simple_lat, simple_total);
//     println!("{}", "-".repeat(80));

//     let (_, _, pulse_p99, pulse_max) = percentiles(pulse_lat);
//     let (_, _, moka_p99, moka_max) = percentiles(moka_lat);
//     let (_, _, quick_p99, quick_max) = percentiles(quick_lat);
//     let (_, _, lru_p99, lru_max) = percentiles(lru_lat);
//     let (_, _, simple_p99, simple_max) = percentiles(simple_lat);

//     println!("\np99 ratio vs PulseMap:");
//     println!("  Moka       : {:.1}x", moka_p99.as_nanos() as f64 / pulse_p99.as_nanos() as f64);
//     println!("  QuickCache : {:.1}x", quick_p99.as_nanos() as f64 / pulse_p99.as_nanos() as f64);
//     println!("  LRU        : {:.1}x", lru_p99.as_nanos() as f64 / pulse_p99.as_nanos() as f64);
//     println!("  Simple     : {:.1}x", simple_p99.as_nanos() as f64 / pulse_p99.as_nanos() as f64);

//     println!("\nmax ratio vs PulseMap:");
//     println!("  Moka       : {:.1}x", moka_max.as_nanos() as f64 / pulse_max.as_nanos() as f64);
//     println!("  QuickCache : {:.1}x", quick_max.as_nanos() as f64 / pulse_max.as_nanos() as f64);
//     println!("  LRU        : {:.1}x", lru_max.as_nanos() as f64 / pulse_max.as_nanos() as f64);
//     println!("  Simple     : {:.1}x", simple_max.as_nanos() as f64 / pulse_max.as_nanos() as f64);
// }
