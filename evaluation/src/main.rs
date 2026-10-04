//! Reproducible candidate, published baseline and competitor evaluation.
//! See ../README.md; performance output is evidence, not a CI speed threshold.
mod scenarios;
mod workload;

use std::hash::Hash;
use std::hint::black_box;
use std::num::NonZeroUsize;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Instant;

trait Number:
    pulse_map::PulseKey
    + pulse_map::PulseValue
    + baseline::PulseKey
    + baseline::PulseValue
    + Copy
    + Eq
    + Hash
    + Send
    + Sync
    + 'static
{
    fn number(v: u64) -> Self;
}
impl Number for u32 {
    fn number(v: u64) -> Self {
        v as u32
    }
}
impl Number for u64 {
    fn number(v: u64) -> Self {
        v
    }
}

trait Cache<T> {
    fn get(&mut self, k: &T) -> Option<T>;
    fn insert(&mut self, k: T, v: T);
    fn len(&self) -> usize;
    fn settle(&self) {}
}

macro_rules! typed_cache {
    ($ty:path) => {
        impl<T: Number> Cache<T> for $ty {
            fn get(&mut self, k: &T) -> Option<T> {
                <$ty>::get(self, k)
            }
            fn insert(&mut self, k: T, v: T) {
                self.insert(k, v);
            }
            fn len(&self) -> usize {
                self.len()
            }
        }
    };
}
typed_cache!(pulse_map::TypedPulseMap<T, T>);
typed_cache!(baseline::TypedPulseMap<T, T>);

trait Shared<T>: Send + Sync {
    fn get(&self, k: &T) -> Option<T>;
    fn insert(&self, k: T, v: T);
    fn len(&self) -> usize;
    fn settle(&self) {}
}
macro_rules! shared_cache {
    ($ty:path) => {
        impl<T: Number> Shared<T> for $ty {
            fn get(&self, k: &T) -> Option<T> {
                self.get(k)
            }
            fn insert(&self, k: T, v: T) {
                self.insert(k, v);
            }
            fn len(&self) -> usize {
                self.len()
            }
        }
    };
}
shared_cache!(pulse_map::ConcurrentPulseMap<T, T>);
shared_cache!(pulse_map::ShardedPulseMap<T, T>);
shared_cache!(baseline::ConcurrentPulseMap<T, T>);
shared_cache!(baseline::ShardedPulseMap<T, T>);
shared_cache!(quick_cache::sync::Cache<T, T>);

impl<T: Number> Shared<T> for moka::sync::Cache<T, T> {
    fn get(&self, k: &T) -> Option<T> {
        self.get(k)
    }
    fn insert(&self, k: T, v: T) {
        self.insert(k, v);
    }
    fn len(&self) -> usize {
        self.entry_count() as usize
    }
    fn settle(&self) {
        self.run_pending_tasks();
    }
}
impl<T: Number> Shared<T> for Mutex<lru::LruCache<T, T>> {
    fn get(&self, k: &T) -> Option<T> {
        self.lock().unwrap().get(k).copied()
    }
    fn insert(&self, k: T, v: T) {
        self.lock().unwrap().put(k, v);
    }
    fn len(&self) -> usize {
        self.lock().unwrap().len()
    }
}
impl<T: Number> Cache<T> for lru::LruCache<T, T> {
    fn get(&mut self, k: &T) -> Option<T> {
        self.get(k).copied()
    }
    fn insert(&mut self, k: T, v: T) {
        self.put(k, v);
    }
    fn len(&self) -> usize {
        self.len()
    }
}
struct SharedAdapter<T>(Box<dyn Shared<T>>);
impl<T: Number> Cache<T> for SharedAdapter<T> {
    fn get(&mut self, k: &T) -> Option<T> {
        self.0.get(k)
    }
    fn insert(&mut self, k: T, v: T) {
        self.0.insert(k, v);
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn settle(&self) {
        self.0.settle();
    }
}

const NAMES: [&str; 9] = [
    "baseline-typed",
    "candidate-typed",
    "baseline-concurrent",
    "candidate-concurrent",
    "baseline-sharded",
    "candidate-sharded",
    "lru",
    "quick",
    "moka",
];

fn shared<T: Number>(name: &str, cap: usize) -> Box<dyn Shared<T>> {
    workload::validate_capacity(cap);
    match name {
        "baseline-concurrent" => Box::new(baseline::ConcurrentPulseMap::new(cap / 4)),
        "candidate-concurrent" => Box::new(pulse_map::ConcurrentPulseMap::new(cap / 4)),
        "baseline-sharded" => Box::new(baseline::ShardedPulseMap::new(cap / 64)),
        "candidate-sharded" => Box::new(pulse_map::ShardedPulseMap::new(cap / 64)),
        "lru" => Box::new(Mutex::new(lru::LruCache::new(
            NonZeroUsize::new(cap).unwrap(),
        ))),
        "quick" => Box::new(quick_cache::sync::Cache::new(cap)),
        "moka" => Box::new(
            moka::sync::Cache::builder()
                .max_capacity(cap as u64)
                .initial_capacity(cap)
                .build(),
        ),
        _ => panic!("unknown concurrent cache: {name}"),
    }
}
fn cache<T: Number>(name: &str, cap: usize) -> Box<dyn Cache<T>> {
    workload::validate_capacity(cap);
    match name {
        "baseline-typed" => Box::new(baseline::TypedPulseMap::new(cap / 4)),
        "candidate-typed" => Box::new(pulse_map::TypedPulseMap::new(cap / 4)),
        "lru" => Box::new(lru::LruCache::new(NonZeroUsize::new(cap).unwrap())),
        _ => Box::new(SharedAdapter(shared(name, cap))),
    }
}

fn hits<T: Number>(name: &str, cap: usize, keys: &[u64]) -> (usize, usize, f64) {
    let mut c = cache::<T>(name, cap);
    let start = Instant::now();
    let mut hits = 0;
    for &key in keys {
        let k = T::number(key);
        if c.get(&k).is_some() {
            hits += 1;
        } else {
            c.insert(k, T::number(key.wrapping_mul(0x9E37_79B9)));
        }
    }
    let secs = start.elapsed().as_secs_f64();
    c.settle();
    (hits, c.len(), secs)
}

fn hitrate(cap: usize, count: usize, trials: usize) {
    println!("kind,trace,seed,cache,type,capacity,ops,hits,resident,seconds");
    for kind in [
        "zipf070",
        "zipf099",
        "zipf130",
        "uniform",
        "scan",
        "hot_scan",
        "phase_shift",
    ] {
        for trial in 0..trials {
            let keys = workload::trace(kind, cap, count, 42 + trial as u64);
            // Rotate order across seeds to reduce systematic temperature/order bias.
            for offset in 0..NAMES.len() {
                let name = NAMES[(offset + trial) % NAMES.len()];
                let (h, n, secs) = hits::<u64>(name, cap, &keys);
                println!(
                    "hit,{kind},{},{name},u64,{cap},{count},{h},{n},{secs:.6}",
                    42 + trial
                );
            }
        }
    }
}

fn throughput<T: Number>(cap: usize, count: usize, trials: usize, shape: &str) {
    for trial in 0..trials {
        let keys = workload::trace("zipf099", cap, count, 42 + trial as u64);
        let ops = workload::operations(&keys, 42 + trial as u64);
        let writes = ops.iter().filter(|(_, w)| *w).count();
        let names = &NAMES[2..];
        for threads in [1, 4, 8] {
            for offset in 0..names.len() {
                let name = names[(offset + trial) % names.len()];
                let c: Arc<dyn Shared<T>> = Arc::from(shared(name, cap));
                for &k in keys.iter().take(cap) {
                    c.insert(T::number(k), T::number(k));
                }
                c.settle();
                let ready = Barrier::new(threads + 1);
                let go = Barrier::new(threads + 1);
                let (elapsed, hits) = std::thread::scope(|s| {
                    let mut handles = Vec::new();
                    for t in 0..threads {
                        let slice = &ops[t * ops.len() / threads..(t + 1) * ops.len() / threads];
                        let (c, ready, go) = (&c, &ready, &go);
                        handles.push(s.spawn(move || {
                            ready.wait();
                            go.wait();
                            let mut hits = 0;
                            for &(k, write) in slice {
                                if write {
                                    c.insert(T::number(k), T::number(k));
                                } else {
                                    hits += usize::from(black_box(c.get(&T::number(k))).is_some());
                                }
                            }
                            hits
                        }));
                    }
                    ready.wait();
                    let start = Instant::now();
                    go.wait();
                    let hits: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
                    (start.elapsed().as_secs_f64(), hits)
                });
                c.settle();
                println!("throughput,{trial},{name},{shape},{cap},{threads},{count},{writes},{hits},{},{elapsed:.6},{:.3}", c.len(), count as f64 / elapsed / 1e6);
            }
        }
    }
}

fn rss_kib() -> u64 {
    let text =
        std::fs::read_to_string("/proc/self/status").expect("RSS measurement requires Linux /proc");
    text.lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|s| s.split_whitespace().next()?.parse().ok())
        })
        .unwrap()
}
fn memory<T: Number>(name: &str, cap: usize, shape: &str) {
    let before = rss_kib();
    let mut c = cache::<T>(name, cap);
    let empty = rss_kib();
    for k in 0..cap as u64 {
        c.insert(T::number(k), T::number(k));
    }
    c.settle();
    let resident = c.len();
    let filled = rss_kib();
    black_box(&c);
    let bytes = filled.saturating_sub(before) * 1024;
    println!(
        "memory,{name},{shape},{cap},{resident},{before},{empty},{filled},{:.2},{:.2}",
        bytes as f64 / cap as f64,
        bytes as f64 / resident.max(1) as f64
    );
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let scenario = args.get(1).map(String::as_str).unwrap_or("all");
    let smoke = args.iter().any(|s| s == "--smoke");
    let (cap, count, trials) = if smoke {
        (1024, 12_000, 1)
    } else {
        (65_536, 2_000_000, 3)
    };
    if scenario == "mem-child" {
        let name = &args[2];
        let cap = args[4].parse().unwrap();
        match args[3].as_str() {
            "u32" => memory::<u32>(name, cap, "u32"),
            "u64" => memory::<u64>(name, cap, "u64"),
            _ => panic!("expected u32 or u64"),
        }
        return;
    }
    println!(
        "# baseline=registry:0.6.5 candidate=path:.. capacity={cap} ops={count} trials={trials}"
    );
    match scenario {
        "hitrate" => hitrate(cap, count, trials),
        "adapt" => scenarios::adaptation(smoke),
        "semantics" => scenarios::semantics(),
        "boundary" => scenarios::boundary(smoke),
        "embedded" => scenarios::embedded(smoke),
        "contention" => scenarios::contention(smoke),
        "throughput" => {
            println!(
                "kind,trial,cache,type,capacity,threads,ops,writes,hits,resident,seconds,mops"
            );
            throughput::<u32>(cap, count, trials, "u32");
            throughput::<u64>(cap, count, trials, "u64");
        }
        "memory" => {
            println!("kind,cache,type,capacity,resident,base_kib,empty_kib,filled_kib,bytes_per_slot,bytes_per_resident");
            for shape in ["u32", "u64"] {
                for name in NAMES {
                    let status = std::process::Command::new(std::env::current_exe().unwrap())
                        .args(["mem-child", name, shape, &cap.to_string()])
                        .status()
                        .unwrap();
                    assert!(status.success(), "memory child failed: {name}/{shape}");
                }
            }
        }
        "all" => {
            for scenario in ["hitrate", "throughput", "memory", "adapt", "semantics", "boundary", "embedded", "contention"] {
                let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
                cmd.arg(scenario);
                if smoke {
                    cmd.arg("--smoke");
                }
                assert!(cmd.status().unwrap().success());
            }
        }
        _ => panic!("use all | hitrate | throughput | memory | adapt | semantics | boundary | embedded | contention [--smoke]"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapters_preserve_values_and_capacity_contract() {
        for name in NAMES {
            let mut c = cache::<u64>(name, 1024);
            c.insert(7, 91);
            assert_eq!(c.get(&7), Some(91), "{name}");
            c.insert(7, 92);
            assert_eq!(c.get(&7), Some(92), "{name}");
            assert_eq!(c.get(&99), None, "{name}");
            for k in 0..4096 {
                c.insert(k, k + 1);
            }
            c.settle();
            assert!(c.len() <= 1024, "{name}");
            for k in 0..4096 {
                if let Some(v) = c.get(&k) {
                    assert_eq!(v, k + 1, "{name}");
                }
            }
        }
        assert_eq!(
            pulse_map::TypedPulseMap::<u32, u32>::new(1024 / 4).capacity(),
            1024
        );
        assert_eq!(
            pulse_map::ShardedPulseMap::<u32, u32>::new(1024 / 64).capacity(),
            1024
        );
    }
}
