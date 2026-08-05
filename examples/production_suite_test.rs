//! Production-like benchmark suite. Three scenarios, each testing a
//! different real-world access pattern, so you can see *where* PulseMap
//! actually wins instead of just "it wins at synthetic uniform inserts."
//!
//! Scenario A — Realistic mixed workload: 80% reads / 20% writes, Zipfian
//!              key distribution (a few keys are hot, most are cold — this
//!              is what almost every real cache workload looks like: DNS,
//!              API rate limiting, session stores, CDN edge caches).
//! Scenario B — Large scale: 5M inserts into a 200K-capacity cache
//!              (constant heavy eviction), single-threaded, with RSS memory
//!              measured before/after so you see actual memory overhead,
//!              not just speed.
//! Scenario C — Extreme hot-key contention: only 64 distinct keys, 8
//!              threads hammering the same tiny keyspace with 50/50
//!              get/insert. This isolates lock/contention behavior from
//!              eviction behavior.
//!
//! Cargo.toml additions:
//!   pulse_map = { path = "../pulse_map" }
//!   moka = { version = "0.12", features = ["sync"] }
//!   quick_cache = "0.6"
//!   lru = "0.12"
//!   rand = "0.8"
//!   rand_distr = "0.4"
//!
//! Run:
//!   cargo run --release --example production_suite

use lru::LruCache;
use moka::sync::Cache as MokaCache;
use pulse_map::ShardedPulseMap;
use quick_cache::sync::Cache as QuickCache;
use rand::Rng;
use rand_distr::{Distribution, Zipf};
use std::collections::HashMap;
use std::fs;
use std::num::NonZeroUsize;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ============================================================
// Common trait so all 5 caches can be driven through one path
// ============================================================

trait BenchCache: Send + Sync + 'static {
    fn insert(&self, k: u32, v: u32);
    fn get(&self, k: u32) -> Option<u32>;
    fn name(&self) -> &'static str;
}

struct PulseAdapter(Arc<ShardedPulseMap<u32, u32>>);
impl BenchCache for PulseAdapter {
    fn insert(&self, k: u32, v: u32) {
        self.0.insert(k, v);
    }
    fn get(&self, k: u32) -> Option<u32> {
        self.0.get(&k)
    }
    fn name(&self) -> &'static str {
        "PulseMap"
    }
}

struct MokaAdapter(Arc<MokaCache<u32, u32>>);
impl BenchCache for MokaAdapter {
    fn insert(&self, k: u32, v: u32) {
        self.0.insert(k, v);
    }
    fn get(&self, k: u32) -> Option<u32> {
        self.0.get(&k)
    }
    fn name(&self) -> &'static str {
        "Moka"
    }
}

struct QuickAdapter(Arc<QuickCache<u32, u32>>);
impl BenchCache for QuickAdapter {
    fn insert(&self, k: u32, v: u32) {
        self.0.insert(k, v);
    }
    fn get(&self, k: u32) -> Option<u32> {
        self.0.get(&k)
    }
    fn name(&self) -> &'static str {
        "QuickCache"
    }
}

struct LruAdapter(Arc<Mutex<LruCache<u32, u32>>>);
impl BenchCache for LruAdapter {
    fn insert(&self, k: u32, v: u32) {
        self.0.lock().unwrap().put(k, v);
    }
    fn get(&self, k: u32) -> Option<u32> {
        self.0.lock().unwrap().get(&k).copied()
    }
    fn name(&self) -> &'static str {
        "LRU"
    }
}

struct SimpleAdapter(Arc<Mutex<HashMap<u32, u32>>>);
impl BenchCache for SimpleAdapter {
    fn insert(&self, k: u32, v: u32) {
        self.0.lock().unwrap().insert(k, v);
    }
    fn get(&self, k: u32) -> Option<u32> {
        self.0.lock().unwrap().get(&k).copied()
    }
    fn name(&self) -> &'static str {
        "Simple"
    }
}

fn all_caches(capacity: usize) -> Vec<Box<dyn BenchCache>> {
    vec![
        Box::new(PulseAdapter(Arc::new(ShardedPulseMap::<u32, u32>::new(
            capacity / 64,
        )))),
        Box::new(MokaAdapter(Arc::new(
            MokaCache::builder()
                .max_capacity(capacity as u64)
                .initial_capacity(capacity)
                .build(),
        ))),
        Box::new(QuickAdapter(Arc::new(QuickCache::<u32, u32>::new(
            capacity,
        )))),
        Box::new(LruAdapter(Arc::new(Mutex::new(LruCache::<u32, u32>::new(
            NonZeroUsize::new(capacity).unwrap(),
        ))))),
        Box::new(SimpleAdapter(Arc::new(Mutex::new(
            HashMap::<u32, u32>::with_capacity(capacity),
        )))),
    ]
}

// ============================================================
// Stats helpers
// ============================================================

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
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
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

/// Linux-only: current process RSS in KB, read from /proc/self/status.
fn rss_kb() -> u64 {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                if let Some(num) = rest.trim().split_whitespace().next() {
                    return num.parse().unwrap_or(0);
                }
            }
        }
    }
    0
}

// ============================================================
// Scenario A — Realistic mixed workload (Zipfian, 80% read / 20% write)
// ============================================================

fn scenario_a() {
    println!("\n================================================================");
    println!("SCENARIO A — Realistic Mixed Workload (80% GET / 20% INSERT, Zipfian hot keys)");
    println!("================================================================");
    println!("Key space: 200,000 | Cache capacity: 50,000 (forces real eviction)");
    println!("Zipfian exponent 1.1 — a small set of keys gets most of the traffic,");
    println!("mirroring real cache access patterns (hot users, hot API routes, hot DNS names).\n");

    const KEY_SPACE: u64 = 200_000;
    const CAPACITY: usize = 50_000;
    const THREADS: u32 = 8;
    const OPS_PER_THREAD: u32 = 125_000; // 1M total
    const TRIALS: usize = 5;

    struct Result {
        name: &'static str,
        get_p99: Vec<f64>,
        insert_p99: Vec<f64>,
        hit_rate: Vec<f64>,
        total_ms: Vec<f64>,
    }

    let mut results: Vec<Result> = Vec::new();

    for _ in 0..TRIALS {
        let caches = all_caches(CAPACITY);
        for cache in caches {
            let cache: Arc<dyn BenchCache> = Arc::from(cache);
            // pre-populate half the capacity so GETs have something to hit
            for i in 0..(CAPACITY as u32 / 2) {
                cache.insert(i, i);
            }

            let barrier = Arc::new(Barrier::new(THREADS as usize));
            let start = Instant::now();
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let cache = Arc::clone(&cache);
                    let barrier = Arc::clone(&barrier);
                    thread::spawn(move || {
                        let mut rng = rand::thread_rng();
                        let zipf = Zipf::new(KEY_SPACE, 1.1).unwrap();
                        let mut get_lat = Vec::with_capacity(OPS_PER_THREAD as usize);
                        let mut insert_lat = Vec::with_capacity(OPS_PER_THREAD as usize / 4);
                        let mut hits = 0u32;
                        let mut gets = 0u32;
                        barrier.wait();
                        for _ in 0..OPS_PER_THREAD {
                            let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
                            if rng.gen_bool(0.8) {
                                let t0 = Instant::now();
                                let v = cache.get(key);
                                get_lat.push(t0.elapsed());
                                gets += 1;
                                if v.is_some() {
                                    hits += 1;
                                }
                            } else {
                                let t0 = Instant::now();
                                cache.insert(key, key);
                                insert_lat.push(t0.elapsed());
                            }
                        }
                        (get_lat, insert_lat, hits, gets)
                    })
                })
                .collect();

            let mut all_get = Vec::new();
            let mut all_insert = Vec::new();
            let mut total_hits = 0u32;
            let mut total_gets = 0u32;
            for h in handles {
                let (g, i, hits, gets) = h.join().unwrap();
                all_get.extend(g);
                all_insert.extend(i);
                total_hits += hits;
                total_gets += gets;
            }
            let total_ms = start.elapsed().as_secs_f64() * 1000.0;

            let (_, get_p99, _) = percentiles_ns(&all_get);
            let (_, insert_p99, _) = percentiles_ns(&all_insert);
            let hit_rate = total_hits as f64 / total_gets as f64 * 100.0;

            let name = cache.name();
            match results.iter_mut().find(|r| r.name == name) {
                Some(r) => {
                    r.get_p99.push(get_p99);
                    r.insert_p99.push(insert_p99);
                    r.hit_rate.push(hit_rate);
                    r.total_ms.push(total_ms);
                }
                None => results.push(Result {
                    name,
                    get_p99: vec![get_p99],
                    insert_p99: vec![insert_p99],
                    hit_rate: vec![hit_rate],
                    total_ms: vec![total_ms],
                }),
            }
        }
    }

    println!(
        "{:<12} {:>14} {:>18} {:>18} {:>12}",
        "Cache", "Total (ms)", "GET p99", "INSERT p99", "Hit rate"
    );
    println!("{}", "-".repeat(80));
    for r in &results {
        let (t_mean, _) = mean_std(&r.total_ms);
        let (g_mean, g_std) = mean_std(&r.get_p99);
        let (i_mean, i_std) = mean_std(&r.insert_p99);
        let (hit_mean, _) = mean_std(&r.hit_rate);
        println!(
            "{:<12} {:>11.1}ms {:>14} ± {:<9} {:>14} ± {:<9} {:>10.1}%",
            r.name,
            t_mean,
            fmt_ns(g_mean),
            fmt_ns(g_std),
            fmt_ns(i_mean),
            fmt_ns(i_std),
            hit_mean
        );
    }
}

// ============================================================
// Scenario B — Large scale + memory footprint
// ============================================================

fn scenario_b() {
    println!("\n================================================================");
    println!("SCENARIO B — Large Scale: 5M inserts, 200K capacity, memory footprint");
    println!("================================================================");
    println!("Single-threaded, constant heavy eviction (cache is 2.5% of insert volume).");
    println!("Measures RSS growth (actual memory cost per cache design), not just speed.\n");

    const CAPACITY: usize = 200_000;
    const NUM_INSERTS: u32 = 5_000_000;
    const TRIALS: usize = 3;

    struct Result {
        name: &'static str,
        total_ms: Vec<f64>,
        rss_delta_mb: Vec<f64>,
    }
    let mut results: Vec<Result> = Vec::new();

    for _ in 0..TRIALS {
        let caches = all_caches(CAPACITY);
        for cache in caches {
            let name = cache.name();
            let rss_before = rss_kb();
            let start = Instant::now();
            for i in 0..NUM_INSERTS {
                cache.insert(i, i);
            }
            let total_ms = start.elapsed().as_secs_f64() * 1000.0;
            let rss_after = rss_kb();
            let rss_delta_mb = (rss_after as f64 - rss_before as f64) / 1024.0;
            drop(cache);

            match results.iter_mut().find(|r| r.name == name) {
                Some(r) => {
                    r.total_ms.push(total_ms);
                    r.rss_delta_mb.push(rss_delta_mb);
                }
                None => results.push(Result {
                    name,
                    total_ms: vec![total_ms],
                    rss_delta_mb: vec![rss_delta_mb],
                }),
            }
        }
    }

    println!(
        "{:<12} {:>14} {:>18} {:>16}",
        "Cache", "Total (ms)", "Throughput (ops/s)", "RSS delta (MB)"
    );
    println!("{}", "-".repeat(65));
    for r in &results {
        let (t_mean, _) = mean_std(&r.total_ms);
        let (rss_mean, _) = mean_std(&r.rss_delta_mb);
        let throughput = NUM_INSERTS as f64 / (t_mean / 1000.0);
        println!(
            "{:<12} {:>11.1}ms {:>18.0} {:>14.1}MB",
            r.name, t_mean, throughput, rss_mean
        );
    }
    println!("\nNote: RSS delta is a coarse proxy (process-wide, includes allocator/OS noise).");
    println!("Trust the relative ordering more than the absolute MB numbers.");
}

// ============================================================
// Scenario C — Extreme hot-key contention (64 keys, 8 threads, 50/50 R/W)
// ============================================================

fn scenario_c() {
    println!("\n================================================================");
    println!("SCENARIO C — Extreme Hot-Key Contention (64 keys, 8 threads, 50/50 GET/INSERT)");
    println!("================================================================");
    println!("Isolates lock/contention cost from eviction cost: tiny keyspace means");
    println!("every thread is constantly touching the same handful of buckets/locks.\n");

    const KEY_SPACE: u32 = 64;
    const THREADS: u32 = 8;
    const OPS_PER_THREAD: u32 = 125_000; // 1M total
    const TRIALS: usize = 5;

    struct Result {
        name: &'static str,
        p99: Vec<f64>,
        total_ms: Vec<f64>,
    }
    let mut results: Vec<Result> = Vec::new();

    for _ in 0..TRIALS {
        let caches = all_caches(128); // capacity > keyspace, so no eviction noise here
        for cache in caches {
            let cache: Arc<dyn BenchCache> = Arc::from(cache);
            for i in 0..KEY_SPACE {
                cache.insert(i, i);
            }

            let barrier = Arc::new(Barrier::new(THREADS as usize));
            let start = Instant::now();
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let cache = Arc::clone(&cache);
                    let barrier = Arc::clone(&barrier);
                    thread::spawn(move || {
                        let mut rng = rand::thread_rng();
                        let mut lat = Vec::with_capacity(OPS_PER_THREAD as usize);
                        barrier.wait();
                        for _ in 0..OPS_PER_THREAD {
                            let key = rng.gen_range(0..KEY_SPACE);
                            let t0 = Instant::now();
                            if rng.gen_bool(0.5) {
                                cache.get(key);
                            } else {
                                cache.insert(key, key);
                            }
                            lat.push(t0.elapsed());
                        }
                        lat
                    })
                })
                .collect();

            let mut all_lat = Vec::new();
            for h in handles {
                all_lat.extend(h.join().unwrap());
            }
            let total_ms = start.elapsed().as_secs_f64() * 1000.0;
            let (_, p99, _) = percentiles_ns(&all_lat);

            let name = cache.name();
            match results.iter_mut().find(|r| r.name == name) {
                Some(r) => {
                    r.p99.push(p99);
                    r.total_ms.push(total_ms);
                }
                None => results.push(Result {
                    name,
                    p99: vec![p99],
                    total_ms: vec![total_ms],
                }),
            }
        }
    }

    println!("{:<12} {:>14} {:>18}", "Cache", "Total (ms)", "p99 latency");
    println!("{}", "-".repeat(50));
    for r in &results {
        let (t_mean, _) = mean_std(&r.total_ms);
        let (p_mean, p_std) = mean_std(&r.p99);
        println!(
            "{:<12} {:>11.1}ms {:>14} ± {:<9}",
            r.name,
            t_mean,
            fmt_ns(p_mean),
            fmt_ns(p_std)
        );
    }
}

fn main() {
    println!("🏭 PRODUCTION-LIKE BENCHMARK SUITE 🏭");
    println!("Three scenarios to find out WHERE each cache actually fits, not just which is fastest overall.\n");
    println!("This will take a few minutes (multiple trials x 3 scenarios x 5 caches).");

    scenario_a();
    scenario_b();
    scenario_c();

    println!("\n================================================================");
    println!("Done. Read the three sections above — a cache that wins Scenario A");
    println!("(hot-key mixed traffic) but loses Scenario C (extreme contention)");
    println!("tells you something different than one that wins everywhere.");
    println!("================================================================");
}
