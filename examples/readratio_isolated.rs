//! Follow-up to Scenario D/E: isolates WHY PulseMap's hit rate looked
//! different in the read-heavy test. Two things changed between D and E
//! at once (capacity/keyspace ratio, Zipf skew, AND a pre-population step
//! that D didn't have) — so E's result wasn't a clean measurement of "does
//! read ratio matter." This test controls for all of that:
//!
//!   - Same capacity/keyspace ratio as D (10%), same Zipf skew (1.3)
//!   - NO pre-population — cache starts empty, exactly like D
//!   - Single-threaded — exactly like D (no contention noise)
//!   - 5 seeded trials, mean ± stddev reported (E didn't report stddev)
//!   - Runs BOTH 80/20 and 99/1 read ratios so you see the isolated effect
//!     of read ratio alone, with everything else held constant
//!
//! If PulseMap's hit rate is still lower at 99/1 under these controlled
//! conditions, that's a real, reportable characteristic. If the gap
//! disappears, it confirms Scenario E's result was a confound (from the
//! pre-population step and/or the different capacity ratio), not a real
//! read-ratio effect.
//!
//! Run:
//!   cargo run --release --example readratio_isolated

use lru::LruCache;
use moka::sync::Cache as MokaCache;
use pulse_map::ShardedPulseMap;
use quick_cache::sync::Cache as QuickCache;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand_distr::{Distribution, Zipf};
use std::num::NonZeroUsize;

trait BenchCache {
    fn insert(&mut self, k: u32, v: u32);
    fn get(&mut self, k: u32) -> Option<u32>;
    fn name(&self) -> &'static str;
}

struct PulseAdapter(ShardedPulseMap<u32, u32>);
impl BenchCache for PulseAdapter {
    fn insert(&mut self, k: u32, v: u32) {
        self.0.insert(k, v);
    }
    fn get(&mut self, k: u32) -> Option<u32> {
        self.0.get(&k)
    }
    fn name(&self) -> &'static str {
        "PulseMap"
    }
}

struct MokaAdapter(MokaCache<u32, u32>);
impl BenchCache for MokaAdapter {
    fn insert(&mut self, k: u32, v: u32) {
        self.0.insert(k, v);
    }
    fn get(&mut self, k: u32) -> Option<u32> {
        self.0.get(&k)
    }
    fn name(&self) -> &'static str {
        "Moka"
    }
}

struct QuickAdapter(QuickCache<u32, u32>);
impl BenchCache for QuickAdapter {
    fn insert(&mut self, k: u32, v: u32) {
        self.0.insert(k, v);
    }
    fn get(&mut self, k: u32) -> Option<u32> {
        self.0.get(&k)
    }
    fn name(&self) -> &'static str {
        "QuickCache"
    }
}

struct LruAdapter(LruCache<u32, u32>);
impl BenchCache for LruAdapter {
    fn insert(&mut self, k: u32, v: u32) {
        self.0.put(k, v);
    }
    fn get(&mut self, k: u32) -> Option<u32> {
        self.0.get(&k).copied()
    }
    fn name(&self) -> &'static str {
        "LRU"
    }
}

fn make_caches(capacity: usize) -> Vec<Box<dyn BenchCache>> {
    vec![
        Box::new(PulseAdapter(ShardedPulseMap::<u32, u32>::new(
            capacity / 64,
        ))),
        Box::new(MokaAdapter(
            MokaCache::builder()
                .max_capacity(capacity as u64)
                .initial_capacity(capacity)
                .build(),
        )),
        Box::new(QuickAdapter(QuickCache::<u32, u32>::new(capacity))),
        Box::new(LruAdapter(LruCache::<u32, u32>::new(
            NonZeroUsize::new(capacity).unwrap(),
        ))),
    ]
}

fn mean_std(vals: &[f64]) -> (f64, f64) {
    let n = vals.len() as f64;
    let mean = vals.iter().sum::<f64>() / n;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}

/// Runs the SAME access-sequence methodology as Scenario D, but with a
/// given GET probability (read_ratio) instead of always get-or-insert.
/// On a GET miss OR when the coin flip picks a "write", we insert —
/// this mirrors realistic behavior: reads that miss still populate the
/// cache, and explicit writes update it too.
fn run_hit_rate_test(
    capacity: usize,
    key_space: u64,
    zipf_exp: f64,
    read_ratio: f64,
    total_ops: u32,
    trials: usize,
) {
    struct Result {
        name: &'static str,
        hit_rates: Vec<f64>,
    }
    let mut results: Vec<Result> = Vec::new();

    for trial in 0..trials {
        let caches = make_caches(capacity);
        for mut cache in caches {
            let mut rng = StdRng::seed_from_u64(trial as u64);
            let zipf = Zipf::new(key_space, zipf_exp).unwrap();

            let mut hits = 0u64;
            let mut get_attempts = 0u64;
            for _ in 0..total_ops {
                let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
                if rng.gen_bool(read_ratio) {
                    get_attempts += 1;
                    if cache.get(key).is_some() {
                        hits += 1;
                    } else {
                        // miss — populate it, like a real cache-aside pattern
                        cache.insert(key, key);
                    }
                } else {
                    cache.insert(key, key);
                }
            }
            let hit_rate = hits as f64 / get_attempts as f64 * 100.0;

            let name = cache.name();
            match results.iter_mut().find(|r| r.name == name) {
                Some(r) => r.hit_rates.push(hit_rate),
                None => results.push(Result {
                    name,
                    hit_rates: vec![hit_rate],
                }),
            }
        }
    }

    results.sort_by(|a, b| {
        let (ma, _) = mean_std(&a.hit_rates);
        let (mb, _) = mean_std(&b.hit_rates);
        mb.partial_cmp(&ma).unwrap()
    });
    for r in &results {
        let (mean, std) = mean_std(&r.hit_rates);
        println!("  {:<12} {:>8.3}% ± {:.3}%", r.name, mean, std);
    }
}

fn main() {
    println!("🔬 READ-RATIO ISOLATION TEST 🔬");
    println!("Controls: capacity=10,000 (10% of 100,000 key space), Zipf exponent=1.3,");
    println!("no pre-population, single-threaded, 5 seeded trials — identical to Scenario D");
    println!("except for the read/write ratio, so we isolate ONLY that variable.\n");

    const CAPACITY: usize = 10_000;
    const KEY_SPACE: u64 = 100_000;
    const ZIPF_EXP: f64 = 1.3;
    const TOTAL_OPS: u32 = 2_000_000;
    const TRIALS: usize = 5;

    println!("--- 80% GET / 20% INSERT (matches Scenario A's ratio) ---");
    run_hit_rate_test(CAPACITY, KEY_SPACE, ZIPF_EXP, 0.80, TOTAL_OPS, TRIALS);

    println!("\n--- 99% GET / 1% INSERT (matches Scenario E's ratio) ---");
    run_hit_rate_test(CAPACITY, KEY_SPACE, ZIPF_EXP, 0.99, TOTAL_OPS, TRIALS);

    println!(
        "\n--- 100% GET-OR-INSERT-ON-MISS (matches Scenario D exactly, as a sanity check) ---"
    );
    run_hit_rate_test(CAPACITY, KEY_SPACE, ZIPF_EXP, 1.0, TOTAL_OPS, TRIALS);

    println!("\n================================================================");
    println!("If PulseMap's relative ranking flips between the 80/20 and 99/1 rows,");
    println!("that's a real read-ratio effect worth documenting. If it stays consistent");
    println!("with Scenario D across all three, Scenario E's result was a confound from");
    println!("the pre-population step and/or the different capacity ratio it used.");
    println!("================================================================");
}
