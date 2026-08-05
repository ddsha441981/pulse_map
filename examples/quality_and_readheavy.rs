//! Two more angles on "where does this actually fit":
//!
//! Scenario D — Eviction Quality (hit rate, not speed). Capacity is
//!              deliberately smaller than the key space, so every cache is
//!              forced to make real eviction decisions under a skewed
//!              (Zipfian) access pattern. The question here isn't "how
//!              fast is an insert" — it's "does the eviction policy keep
//!              the *right* keys hot." A cache can be blazing fast and
//!              still evict badly.
//!
//! Scenario E — Read-Heavy (99% GET / 1% INSERT). Most real caches are
//!              read-dominated (session lookups, config reads, DNS
//!              resolution) — writes are rare. This is a different shape
//!              from the 80/20 mix tested earlier and often the more
//!              common real-world ratio.
//!
//! Cargo.toml: same deps as before (pulse_map, moka, quick_cache, lru,
//! rand, rand_distr).
//!
//! Run:
//!   cargo run --release --example quality_and_readheavy

use pulse_map::ShardedPulseMap;
use moka::sync::Cache as MokaCache;
use lru::LruCache;
use quick_cache::sync::Cache as QuickCache;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use rand_distr::{Distribution, Zipf};
use std::num::NonZeroUsize;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ============================================================
// Shared trait (same shape as production_suite.rs)
// ============================================================

trait BenchCache: Send + Sync + 'static {
    fn insert(&self, k: u32, v: u32);
    fn get(&self, k: u32) -> Option<u32>;
    fn name(&self) -> &'static str;
}

struct PulseAdapter(Arc<ShardedPulseMap<u32, u32>>);
impl BenchCache for PulseAdapter {
    fn insert(&self, k: u32, v: u32) { self.0.insert(k, v); }
    fn get(&self, k: u32) -> Option<u32> { self.0.get(&k) }
    fn name(&self) -> &'static str { "PulseMap" }
}

struct MokaAdapter(Arc<MokaCache<u32, u32>>);
impl BenchCache for MokaAdapter {
    fn insert(&self, k: u32, v: u32) { self.0.insert(k, v); }
    fn get(&self, k: u32) -> Option<u32> { self.0.get(&k) }
    fn name(&self) -> &'static str { "Moka" }
}

struct QuickAdapter(Arc<QuickCache<u32, u32>>);
impl BenchCache for QuickAdapter {
    fn insert(&self, k: u32, v: u32) { self.0.insert(k, v); }
    fn get(&self, k: u32) -> Option<u32> { self.0.get(&k) }
    fn name(&self) -> &'static str { "QuickCache" }
}

struct LruAdapter(Arc<Mutex<LruCache<u32, u32>>>);
impl BenchCache for LruAdapter {
    fn insert(&self, k: u32, v: u32) { self.0.lock().unwrap().put(k, v); }
    fn get(&self, k: u32) -> Option<u32> { self.0.lock().unwrap().get(&k).copied() }
    fn name(&self) -> &'static str { "LRU" }
}

fn quality_caches(capacity: usize) -> Vec<Box<dyn BenchCache>> {
    // Simple/Mutex<HashMap> excluded here on purpose — it has no eviction
    // policy at all, so a "hit rate" comparison against it isn't meaningful.
    vec![
        Box::new(PulseAdapter(Arc::new(ShardedPulseMap::<u32, u32>::new(capacity / 64)))),
        Box::new(MokaAdapter(Arc::new(
            MokaCache::builder()
                .max_capacity(capacity as u64)
                .initial_capacity(capacity)
                .build(),
        ))),
        Box::new(QuickAdapter(Arc::new(QuickCache::<u32, u32>::new(capacity)))),
        Box::new(LruAdapter(Arc::new(Mutex::new(LruCache::<u32, u32>::new(
            NonZeroUsize::new(capacity).unwrap(),
        ))))),
    ]
}

fn mean_std(vals: &[f64]) -> (f64, f64) {
    let n = vals.len() as f64;
    let mean = vals.iter().sum::<f64>() / n;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}

fn percentiles_ns(latencies: &[Duration]) -> (f64, f64) {
    let mut ns: Vec<u128> = latencies.iter().map(|d| d.as_nanos()).collect();
    ns.sort_unstable();
    let len = ns.len();
    (ns[len * 50 / 100] as f64, ns[len * 99 / 100] as f64)
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

// ============================================================
// Scenario D — Eviction Quality (hit rate under memory pressure)
// ============================================================

fn scenario_d() {
    println!("\n================================================================");
    println!("SCENARIO D — Eviction Quality: Hit Rate Under Memory Pressure");
    println!("================================================================");
    println!("Capacity: 10,000 | Key space: 100,000 (cache is 10% of working set)");
    println!("Zipfian exponent 1.3 (strongly skewed — a real 'hot/cold' access pattern).");
    println!("Each access: try GET; on miss, INSERT (classic cache-fill-on-miss simulation).");
    println!("This measures whether the eviction POLICY keeps the right keys hot —");
    println!("not insert speed. Single-threaded so eviction-policy differences aren't");
    println!("muddied by lock-contention noise.\n");

    const CAPACITY: usize = 10_000;
    const KEY_SPACE: u64 = 100_000;
    const TOTAL_ACCESSES: u32 = 2_000_000;
    const TRIALS: usize = 5;
    const ZIPF_EXPONENT: f64 = 1.3;

    struct Result {
        name: &'static str,
        hit_rates: Vec<f64>,
    }
    let mut results: Vec<Result> = Vec::new();

    for trial in 0..TRIALS {
        let caches = quality_caches(CAPACITY);
        for cache in caches {
            // deterministic-ish per trial so every cache sees the SAME
            // access sequence within a trial (fair comparison), but the
            // sequence differs across trials (so we're not overfitting to
            // one lucky/unlucky ordering).
            let mut rng = StdRng::seed_from_u64(trial as u64);
            let zipf = Zipf::new(KEY_SPACE, ZIPF_EXPONENT).unwrap();

            let mut hits = 0u64;
            for _ in 0..TOTAL_ACCESSES {
                let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
                if cache.get(key).is_some() {
                    hits += 1;
                } else {
                    cache.insert(key, key);
                }
            }
            let hit_rate = hits as f64 / TOTAL_ACCESSES as f64 * 100.0;

            let name = cache.name();
            match results.iter_mut().find(|r| r.name == name) {
                Some(r) => r.hit_rates.push(hit_rate),
                None => results.push(Result { name, hit_rates: vec![hit_rate] }),
            }
        }
    }

    println!("{:<12} {:>16}", "Cache", "Hit rate (mean ± stddev)");
    println!("{}", "-".repeat(40));
    let mut sorted = results;
    sorted.sort_by(|a, b| {
        let (ma, _) = mean_std(&a.hit_rates);
        let (mb, _) = mean_std(&b.hit_rates);
        mb.partial_cmp(&ma).unwrap()
    });
    for r in &sorted {
        let (mean, std) = mean_std(&r.hit_rates);
        println!("{:<12} {:>10.2}% ± {:.2}%", r.name, mean, std);
    }
    println!("\nHigher hit rate = the eviction policy is better at keeping the keys");
    println!("that actually get re-accessed, not just evicting fast.");
}

// ============================================================
// Scenario E — Read-Heavy (99% GET / 1% INSERT)
// ============================================================

fn scenario_e() {
    println!("\n================================================================");
    println!("SCENARIO E — Read-Heavy Workload (99% GET / 1% INSERT, Zipfian hot keys)");
    println!("================================================================");
    println!("Key space: 200,000 | Capacity: 50,000 | 8 threads | Zipfian exponent 1.1");
    println!("Most production caches look like this: reads dominate, writes are rare");
    println!("(session lookups, config reads, DNS resolution, feature-flag checks).\n");

    const KEY_SPACE: u64 = 200_000;
    const CAPACITY: usize = 50_000;
    const THREADS: u32 = 8;
    const OPS_PER_THREAD: u32 = 125_000; // 1M total
    const TRIALS: usize = 5;

    struct Result {
        name: &'static str,
        get_p99: Vec<f64>,
        total_ms: Vec<f64>,
        hit_rate: Vec<f64>,
    }
    let mut results: Vec<Result> = Vec::new();

    for _ in 0..TRIALS {
        let caches = quality_caches(CAPACITY); // reuse the 4-cache set (no Simple)
        for cache in caches {
            let cache: Arc<dyn BenchCache> = Arc::from(cache);
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
                        let mut hits = 0u32;
                        let mut gets = 0u32;
                        barrier.wait();
                        for _ in 0..OPS_PER_THREAD {
                            let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
                            if rng.gen_bool(0.99) {
                                let t0 = Instant::now();
                                let v = cache.get(key);
                                get_lat.push(t0.elapsed());
                                gets += 1;
                                if v.is_some() {
                                    hits += 1;
                                }
                            } else {
                                cache.insert(key, key);
                            }
                        }
                        (get_lat, hits, gets)
                    })
                })
                .collect();

            let mut all_get = Vec::new();
            let mut total_hits = 0u32;
            let mut total_gets = 0u32;
            for h in handles {
                let (g, hits, gets) = h.join().unwrap();
                all_get.extend(g);
                total_hits += hits;
                total_gets += gets;
            }
            let total_ms = start.elapsed().as_secs_f64() * 1000.0;
            let (_, get_p99) = percentiles_ns(&all_get);
            let hit_rate = total_hits as f64 / total_gets as f64 * 100.0;

            let name = cache.name();
            match results.iter_mut().find(|r| r.name == name) {
                Some(r) => {
                    r.get_p99.push(get_p99);
                    r.total_ms.push(total_ms);
                    r.hit_rate.push(hit_rate);
                }
                None => results.push(Result {
                    name,
                    get_p99: vec![get_p99],
                    total_ms: vec![total_ms],
                    hit_rate: vec![hit_rate],
                }),
            }
        }
    }

    println!("{:<12} {:>14} {:>18} {:>12}", "Cache", "Total (ms)", "GET p99", "Hit rate");
    println!("{}", "-".repeat(62));
    for r in &results {
        let (t_mean, _) = mean_std(&r.total_ms);
        let (g_mean, g_std) = mean_std(&r.get_p99);
        let (hit_mean, _) = mean_std(&r.hit_rate);
        println!(
            "{:<12} {:>11.1}ms {:>14} ± {:<9} {:>10.1}%",
            r.name, t_mean, fmt_ns(g_mean), fmt_ns(g_std), hit_mean
        );
    }
}

fn main() {
    println!("🔬 EVICTION QUALITY & READ-HEAVY BENCHMARK 🔬\n");
    scenario_d();
    scenario_e();
    println!("\n================================================================");
    println!("Done. Scenario D tells you if the eviction POLICY is smart, not just fast.");
    println!("Scenario E tells you what happens at the read ratio most real caches see.");
    println!("================================================================");
}