//! Focused policy/semantic probes. Timing is reported separately from correctness.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn phase(c: &mut dyn Cache<u32>, keys: impl Iterator<Item = u32>) -> (usize, usize) {
    let (mut ops, mut hits) = (0, 0);
    for k in keys {
        ops += 1;
        if let Some(v) = c.get(&k) {
            assert_eq!(v, k);
            hits += 1;
        } else {
            c.insert(k, k);
        }
    }
    c.settle();
    (ops, hits)
}

pub fn adaptation(smoke: bool) {
    println!("kind,cache,capacity,phase,ops,hits,resident");
    for cap in if smoke {
        vec![4, 1024]
    } else {
        vec![4, 1024, 65536]
    } {
        let names: &[&str] = if cap == 4 {
            &[
                "baseline-typed",
                "candidate-typed",
                "baseline-concurrent",
                "candidate-concurrent",
                "lru",
            ]
        } else {
            &NAMES
        };
        for &name in names {
            let mut c: Box<dyn Cache<u32>> = if cap == 4 {
                match name {
                    "baseline-typed" => Box::new(baseline::TypedPulseMap::new(1)),
                    "candidate-typed" => Box::new(pulse_map::TypedPulseMap::new(1)),
                    "baseline-concurrent" => Box::new(SharedAdapter(Box::new(
                        baseline::ConcurrentPulseMap::new(1),
                    ))),
                    "candidate-concurrent" => Box::new(SharedAdapter(Box::new(
                        pulse_map::ConcurrentPulseMap::new(1),
                    ))),
                    _ => Box::new(lru::LruCache::new(NonZeroUsize::new(4).unwrap())),
                }
            } else {
                cache(name, cap)
            };
            let hot = if cap == 4 { 4 } else { cap as u32 / 4 };
            for (label, start, rounds) in [("warm", 0, 100), ("shift", hot, 100)] {
                let (ops, h) = phase(&mut *c, (0..hot * rounds).map(|i| start + i % hot));
                println!("adapt,{name},{cap},{label},{ops},{h},{}", c.len());
            }
            let (ops, h) = phase(&mut *c, 2 * hot..2 * hot + cap as u32 * 3);
            println!("adapt,{name},{cap},scan,{ops},{h},{}", c.len());
            let (ops, h) = phase(&mut *c, (0..hot * 10).map(|i| hot + i % hot));
            println!("adapt,{name},{cap},post_scan,{ops},{h},{}", c.len());
        }
    }
}

pub fn semantics() {
    // Execute identical observable contracts on both versions. These are semantics,
    // not expected-before failures; targeted bug regressions live in root tests/.
    macro_rules! check {
        ($ty:path, $name:expr) => {{
            let mut map = <$ty>::new(16);
            map.set_ttl(1);
            map.insert(1, 10);
            map.insert(2, 20);
            assert_eq!(map.get(&1), Some(10));
            map.insert(2, 21);
            assert_eq!(map.get(&1), None);
            assert_eq!(map.len(), 2);
            assert_eq!(map.iter().count(), 2);
            map.set_ttl(0);
            assert_eq!(map.peek(&1), Some(10));
            map.set_ttl(1);
            map.insert_ttl(1, 11, u64::MAX);
            for _ in 0..10 {
                map.insert(2, 20);
            }
            assert_eq!(map.peek(&1), Some(11));
            println!("semantics,{},pass", $name);
        }};
    }
    println!("kind,cache,result");
    check!(baseline::TypedPulseMap<u32,u32>, "baseline-typed");
    check!(pulse_map::TypedPulseMap<u32,u32>, "candidate-typed");
}

pub fn boundary(smoke: bool) {
    println!("kind,cache,buckets_per_shard,capacity,inserts,resident");
    let inserts = if smoke { 4096 } else { 1_000_000 };
    let buckets = if smoke {
        [16, 32, 64]
    } else {
        [8192, 16384, 32768]
    };
    for b in buckets {
        for name in ["baseline-sharded", "candidate-sharded"] {
            let mut c = cache::<u32>(name, b * 64);
            for k in 0..inserts {
                c.insert(k, k);
            }
            for k in 0..inserts {
                if let Some(v) = c.get(&k) {
                    assert_eq!(v, k);
                }
            }
            println!("boundary,{name},{b},{},{inserts},{}", b * 64, c.len());
        }
    }
}

pub fn embedded(smoke: bool) {
    use rand::{rngs::StdRng, Rng, SeedableRng};
    use rand_distr::{Distribution, Zipf};
    println!("kind,trace,seed,cache,type,capacity,ops,hits,resident,seconds");
    for seed in 42..if smoke { 43 } else { 45 } {
        for (label, cap, count) in [
            ("host_routing", 2048, 100_000),
            ("host_sensors", 256, 10_000),
        ] {
            let mut rng = StdRng::seed_from_u64(seed);
            let zipf = Zipf::new(1000, 1.1).unwrap();
            let keys: Vec<u64> = (0..count)
                .map(|_| {
                    if label == "host_routing" {
                        rng.gen_range(0..5000)
                    } else {
                        zipf.sample(&mut rng) as u64 - 1
                    }
                })
                .collect();
            for name in ["baseline-typed", "candidate-typed", "lru"] {
                let (h, n, secs) = hits::<u32>(name, cap, &keys);
                println!("embedded,{label},{seed},{name},u32,{cap},{count},{h},{n},{secs:.6}");
            }
        }
    }
}

pub fn contention(smoke: bool) {
    println!(
        "kind,trial,cache,capacity,writes,reads,hits,samples_while_writer_active,p50_ns,p99_ns"
    );
    let writes = if smoke { 10_000 } else { 1_000_000 };
    let names = [
        "baseline-concurrent",
        "candidate-concurrent",
        "baseline-sharded",
        "candidate-sharded",
        "lru",
    ];
    for trial in 0..if smoke { 1 } else { 3 } {
        for offset in 0..names.len() {
            let name = names[(offset + trial) % names.len()];
            let c: Arc<dyn Shared<u32>> = Arc::from(shared(name, 1024));
            // One always-resident hot key: equal hit work and deliberate contention.
            c.insert(0, 0);
            let ready = Barrier::new(2);
            let done = AtomicBool::new(false);
            let mut samples = Vec::with_capacity(1_000_000);
            let (mut hits, mut active) = (0, 0);
            std::thread::scope(|s| {
                let (c, ready, done) = (&c, &ready, &done);
                s.spawn(move || {
                    ready.wait();
                    for _ in 0..writes {
                        c.insert(0, 0);
                    }
                    done.store(true, Ordering::Release);
                });
                ready.wait();
                while samples.len() < 1_000_000 {
                    let running = !done.load(Ordering::Acquire);
                    if !running && samples.len() >= 10_000 {
                        break;
                    }
                    active += usize::from(running);
                    let start = Instant::now();
                    let value = c.get(&0); // adapter drops LRU guard before returning
                    let ns = start.elapsed().as_nanos();
                    hits += usize::from(value == Some(0));
                    samples.push(ns); // sample recording outside all map guards
                }
            });
            assert_eq!(hits, samples.len());
            samples.sort_unstable();
            let n = samples.len();
            let p99 = samples[(99 * n).div_ceil(100) - 1];
            println!(
                "contention,{trial},{name},1024,{writes},{n},{hits},{active},{},{p99}",
                samples[n / 2]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn documented_epoch_and_occupancy_contracts() {
        super::semantics();
    }
}
