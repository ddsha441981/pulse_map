//! Does the README's "Eviction Quality" table measure the eviction *policy*?
//!
//! It reports PulseMap at 96.73% against `lru` at 95.83% on a Zipfian workload
//! with "capacity fixed at 10% of the key space". Reading the code behind that
//! number turned up two things worth checking before the claim is repeated:
//!
//! 1. **Capacity.** `examples/quality_and_readheavy.rs` builds
//!    `ShardedPulseMap::new(capacity / 64)` — 10_000/64 = 156 buckets per shard.
//!    But every shard rounds its bucket count up to a power of two
//!    (`src/sync.rs:112`), so 156 becomes 256: 16 × 256 × 4 = **16,384 nominal
//!    slots**, while `lru`, Moka and QuickCache were each handed exactly 10,000.
//!    A cache holding more entries wins on hit rate whatever its policy is.
//!
//! 2. **Reads.** A `ShardedPulseMap` is 16 `ConcurrentPulseMap`s
//!    (`src/sharded.rs:41`), and `ConcurrentPulseMap::get` does not touch the
//!    MetaWord — it pushes to an `AccessBuffer` (`src/sync.rs:429`) that nothing
//!    in the crate ever drains. On this workload ~97% of operations are hits, so
//!    if buffered accesses are dropped then the "LFU+LRU hybrid" was ranked on a
//!    path where **reads contribute nothing to priority** and only the ~3% of
//!    operations that are inserts drive eviction.
//!
//! Same workload as Scenario D of `quality_and_readheavy.rs` (100k key space,
//! Zipf 1.3, 2M accesses, get-then-insert-on-miss, 5 seeded trials), run across
//! variants that separate those two effects:
//!
//! | Variant | Capacity | Sharded | Reads reach the policy |
//! |---|---|---|---|
//! | `ShardedPulseMap`    | 16,384 | yes | no  |
//! | `ConcurrentPulseMap` | 16,384 | no  | no  |
//! | `TypedPulseMap`      | 16,384 | no  | yes (`on_access`, `src/raw.rs:262`) |
//! | `lru` @ 16,384       | 16,384 | —   | yes |
//! | `lru` @ 10,000       | 10,000 | —   | yes |
//!
//! Sharded vs Concurrent isolates sharding. Concurrent vs Typed isolates the
//! read path, since those two differ in nothing else. `lru` appears twice so the
//! capacity confound can be read off directly instead of argued about.
//!
//! PulseMap capacity is always `pow2 × 4`, so 10,000 is not reachable — 16,384
//! is the nearest point where every cache can be given the same budget.
//!
//! Run:
//!   cargo run --release --example eviction_quality_audit

use lru::LruCache;
use pulse_map::{ConcurrentPulseMap, ShardedPulseMap, TypedPulseMap};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rand_distr::{Distribution, Zipf};
use std::num::NonZeroUsize;

const KEY_SPACE: u64 = 100_000;
const TOTAL_ACCESSES: u32 = 2_000_000;
const TRIALS: usize = 5;
const ZIPF_EXPONENT: f64 = 1.3;

/// Single-threaded, so `&mut self` throughout — no locks in the measurement.
trait Cache {
    /// `true` on a hit.
    fn get(&mut self, k: u32) -> bool;
    fn insert(&mut self, k: u32);
    fn len(&self) -> usize;
}

impl Cache for TypedPulseMap<u32, u32> {
    fn get(&mut self, k: u32) -> bool {
        TypedPulseMap::get(self, &k).is_some()
    }
    fn insert(&mut self, k: u32) {
        TypedPulseMap::insert(self, k, k);
    }
    fn len(&self) -> usize {
        TypedPulseMap::len(self)
    }
}

/// The control. Identical map to `TypedPulseMap` above, read through `peek`
/// (`src/lib.rs:403`) instead of `get` — same lookup, no priority update. If the
/// read path is the whole story, this lands on `ConcurrentPulseMap`'s number.
struct PeekMap(TypedPulseMap<u32, u32>);
impl Cache for PeekMap {
    fn get(&mut self, k: u32) -> bool {
        self.0.peek(&k).is_some()
    }
    fn insert(&mut self, k: u32) {
        self.0.insert(k, k);
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl Cache for ConcurrentPulseMap<u32, u32> {
    fn get(&mut self, k: u32) -> bool {
        ConcurrentPulseMap::get(self, &k).is_some()
    }
    fn insert(&mut self, k: u32) {
        ConcurrentPulseMap::insert(self, k, k);
    }
    fn len(&self) -> usize {
        ConcurrentPulseMap::len(self)
    }
}

impl Cache for ShardedPulseMap<u32, u32> {
    fn get(&mut self, k: u32) -> bool {
        ShardedPulseMap::get(self, &k).is_some()
    }
    fn insert(&mut self, k: u32) {
        ShardedPulseMap::insert(self, k, k);
    }
    fn len(&self) -> usize {
        ShardedPulseMap::len(self)
    }
}

impl Cache for LruCache<u32, u32> {
    fn get(&mut self, k: u32) -> bool {
        LruCache::get(self, &k).is_some()
    }
    fn insert(&mut self, k: u32) {
        LruCache::put(self, k, k);
    }
    fn len(&self) -> usize {
        LruCache::len(self)
    }
}

/// One trial: get, and on a miss insert. Identical to Scenario D's inner loop.
/// Returns (hit rate %, resident entries at the end).
fn trial<C: Cache>(mut cache: C, seed: u64) -> (f64, usize) {
    let mut rng = StdRng::seed_from_u64(seed);
    let zipf = Zipf::new(KEY_SPACE, ZIPF_EXPONENT).unwrap();
    let mut hits = 0u64;
    for _ in 0..TOTAL_ACCESSES {
        let key = (zipf.sample(&mut rng) as u32).saturating_sub(1);
        if cache.get(key) {
            hits += 1;
        } else {
            cache.insert(key);
        }
    }
    (hits as f64 / TOTAL_ACCESSES as f64 * 100.0, cache.len())
}

fn mean_std(vals: &[f64]) -> (f64, f64) {
    let n = vals.len() as f64;
    let mean = vals.iter().sum::<f64>() / n;
    let var = vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}

struct Row {
    label: &'static str,
    nominal: usize,
    reads_count: &'static str,
    hit_rates: Vec<f64>,
    resident: usize,
}

fn main() {
    println!("Eviction-quality audit — same workload as Scenario D of quality_and_readheavy.rs");
    println!(
        "Key space {KEY_SPACE}, Zipf {ZIPF_EXPONENT}, {TOTAL_ACCESSES} accesses, {TRIALS} trials"
    );
    println!("Every variant sees the identical access sequence within a trial.\n");

    let mut rows = vec![
        Row {
            label: "ShardedPulseMap",
            nominal: 16 * 256 * 4,
            reads_count: "no (buffer)",
            hit_rates: Vec::new(),
            resident: 0,
        },
        Row {
            label: "ConcurrentPulseMap",
            nominal: 4096 * 4,
            reads_count: "no (buffer)",
            hit_rates: Vec::new(),
            resident: 0,
        },
        Row {
            label: "TypedPulseMap",
            nominal: 4096 * 4,
            reads_count: "yes (on_access)",
            hit_rates: Vec::new(),
            resident: 0,
        },
        Row {
            label: "TypedPulseMap (peek)",
            nominal: 4096 * 4,
            reads_count: "no (peek)",
            hit_rates: Vec::new(),
            resident: 0,
        },
        Row {
            label: "lru @ 16,384",
            nominal: 16_384,
            reads_count: "yes",
            hit_rates: Vec::new(),
            resident: 0,
        },
        Row {
            label: "lru @ 10,000",
            nominal: 10_000,
            reads_count: "yes",
            hit_rates: Vec::new(),
            resident: 0,
        },
    ];

    for seed in 0..TRIALS as u64 {
        // `ShardedPulseMap::new(156)` is what the README's example passes; each
        // shard rounds 156 up to 256, so `new(256)` builds the identical map.
        let runs = [
            trial(ShardedPulseMap::<u32, u32>::new(156), seed),
            trial(ConcurrentPulseMap::<u32, u32>::new(4096), seed),
            trial(TypedPulseMap::<u32, u32>::new(4096), seed),
            trial(PeekMap(TypedPulseMap::<u32, u32>::new(4096)), seed),
            trial(
                LruCache::<u32, u32>::new(NonZeroUsize::new(16_384).unwrap()),
                seed,
            ),
            trial(
                LruCache::<u32, u32>::new(NonZeroUsize::new(10_000).unwrap()),
                seed,
            ),
        ];
        for (row, (rate, resident)) in rows.iter_mut().zip(runs) {
            row.hit_rates.push(rate);
            row.resident = resident;
        }
        println!("trial {seed} done");
    }

    println!();
    println!(
        "{:<20} {:>9} {:>9} {:>18} {:>17}",
        "Variant", "nominal", "resident", "hit rate", "reads reach policy"
    );
    println!("{}", "-".repeat(78));
    for r in &rows {
        let (mean, std) = mean_std(&r.hit_rates);
        println!(
            "{:<20} {:>9} {:>9} {:>11.2}% ± {:.2}% {:>17}",
            r.label, r.nominal, r.resident, mean, std, r.reads_count
        );
    }

    let get = |label: &str| -> f64 {
        mean_std(&rows.iter().find(|r| r.label == label).unwrap().hit_rates).0
    };
    println!();
    println!(
        "read path      (Typed - Concurrent, same size, same structure): {:+.2} pts",
        get("TypedPulseMap") - get("ConcurrentPulseMap")
    );
    println!(
        "peek control   (Typed-peek - Concurrent, reads suppressed both):  {:+.2} pts",
        get("TypedPulseMap (peek)") - get("ConcurrentPulseMap")
    );
    println!(
        "sharding       (Sharded - Concurrent, same size):               {:+.2} pts",
        get("ShardedPulseMap") - get("ConcurrentPulseMap")
    );
    println!(
        "capacity gift  (lru @16,384 - lru @10,000):                     {:+.2} pts",
        get("lru @ 16,384") - get("lru @ 10,000")
    );
    println!(
        "README's pair  (Sharded @16,384 - lru @10,000):                 {:+.2} pts",
        get("ShardedPulseMap") - get("lru @ 10,000")
    );
    println!(
        "same-size pair (Sharded @16,384 - lru @16,384):                 {:+.2} pts",
        get("ShardedPulseMap") - get("lru @ 16,384")
    );
}
