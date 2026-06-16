// Copyright (c) 2026 Deendayal Kumawat. All rights reserved.
// Licensed under the MIT OR Apache-2.0 license.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lru::LruCache;
use pulse_map::{PulseMap, TypedPulseMap};
use std::collections::HashMap;
use std::num::NonZeroUsize;

// ═══════════════════════════════════════
// PulseMap (Raw) Benchmarks
// ═══════════════════════════════════════

fn pulse_raw_insert_100k(c: &mut Criterion) {
    c.bench_function("pulse_raw_insert_100k", |b| {
        b.iter(|| {
            let mut map = PulseMap::new(38461);
            for i in 0u32..100_000 {
                map.insert(&i.to_le_bytes(), b"val");
            }
            black_box(&map);
        });
    });
}

fn pulse_raw_lookup_100k(c: &mut Criterion) {
    let mut map = PulseMap::new(38461);
    for i in 0u32..100_000 {
        map.insert(&i.to_le_bytes(), b"val");
    }
    c.bench_function("pulse_raw_lookup_100k", |b| {
        b.iter(|| {
            let mut hits = 0u64;
            for i in 0u32..100_000 {
                if map.get(&i.to_le_bytes()).is_some() {
                    hits += 1;
                }
            }
            black_box(hits)
        });
    });
}

fn pulse_raw_mixed_100k(c: &mut Criterion) {
    c.bench_function("pulse_raw_mixed_100k", |b| {
        b.iter(|| {
            let mut map = PulseMap::new(38461);
            for i in 0u32..100_000 {
                map.insert(&i.to_le_bytes(), b"val");
                if i > 0 {
                    black_box(map.get(&(i / 2).to_le_bytes()));
                }
            }
        });
    });
}

fn pulse_raw_eviction(c: &mut Criterion) {
    c.bench_function("pulse_raw_eviction_50k", |b| {
        b.iter(|| {
            let mut map = PulseMap::new(256);
            for i in 0u32..50_000 {
                map.insert(&i.to_le_bytes(), b"val");
            }
            black_box(map.eviction_count());
        });
    });
}

// ═══════════════════════════════════════
// TypedPulseMap<u32, u32> Benchmarks
// ═══════════════════════════════════════

fn pulse_typed_insert_100k(c: &mut Criterion) {
    c.bench_function("pulse_typed_insert_100k", |b| {
        b.iter(|| {
            let mut map = TypedPulseMap::<u32, u32>::new(38461);
            for i in 0u32..100_000 {
                map.insert(i, i * 2);
            }
            black_box(map.len());
        });
    });
}

fn pulse_typed_lookup_100k(c: &mut Criterion) {
    let mut map = TypedPulseMap::<u32, u32>::new(38461);
    for i in 0u32..100_000 {
        map.insert(i, i * 2);
    }
    c.bench_function("pulse_typed_lookup_100k", |b| {
        b.iter(|| {
            let mut hits = 0u64;
            for i in 0u32..100_000 {
                if map.get(&i).is_some() {
                    hits += 1;
                }
            }
            black_box(hits)
        });
    });
}

fn pulse_typed_mixed_100k(c: &mut Criterion) {
    c.bench_function("pulse_typed_mixed_100k", |b| {
        b.iter(|| {
            let mut map = TypedPulseMap::<u32, u32>::new(38461);
            for i in 0u32..100_000 {
                map.insert(i, i * 2);
                if i > 0 {
                    black_box(map.get(&(i / 2)));
                }
            }
        });
    });
}

fn pulse_typed_iterator(c: &mut Criterion) {
    let mut map = TypedPulseMap::<u32, u32>::new(16384);
    for i in 0u32..50_000 {
        map.insert(i, i * 2);
    }
    c.bench_function("pulse_typed_iter_50k", |b| {
        b.iter(|| {
            let sum: u64 = map.iter().map(|(_, v)| v as u64).sum();
            black_box(sum)
        });
    });
}

// ═══════════════════════════════════════
// LRU Cache Benchmarks (FAIR comparison)
// ═══════════════════════════════════════

fn lru_insert_100k(c: &mut Criterion) {
    c.bench_function("lru_insert_100k", |b| {
        b.iter(|| {
            let cap = NonZeroUsize::new(154_000).unwrap(); // ~same capacity as PulseMap
            let mut cache = LruCache::<u32, u32>::new(cap);
            for i in 0u32..100_000 {
                cache.put(i, i * 2);
            }
            black_box(cache.len());
        });
    });
}

fn lru_lookup_100k(c: &mut Criterion) {
    let cap = NonZeroUsize::new(154_000).unwrap();
    let mut cache = LruCache::<u32, u32>::new(cap);
    for i in 0u32..100_000 {
        cache.put(i, i * 2);
    }
    c.bench_function("lru_lookup_100k", |b| {
        b.iter(|| {
            let mut hits = 0u64;
            for i in 0u32..100_000 {
                if cache.get(&i).is_some() {
                    hits += 1;
                }
            }
            black_box(hits)
        });
    });
}

fn lru_mixed_100k(c: &mut Criterion) {
    c.bench_function("lru_mixed_100k", |b| {
        b.iter(|| {
            let cap = NonZeroUsize::new(154_000).unwrap();
            let mut cache = LruCache::<u32, u32>::new(cap);
            for i in 0u32..100_000 {
                cache.put(i, i * 2);
                if i > 0 {
                    black_box(cache.get(&(i / 2)));
                }
            }
        });
    });
}

fn lru_eviction_50k(c: &mut Criterion) {
    c.bench_function("lru_eviction_50k", |b| {
        b.iter(|| {
            let cap = NonZeroUsize::new(1024).unwrap(); // small cap = evictions
            let mut cache = LruCache::<u32, u32>::new(cap);
            for i in 0u32..50_000 {
                cache.put(i, i * 2);
            }
            black_box(cache.len());
        });
    });
}

// ═══════════════════════════════════════
// std::HashMap Benchmarks (reference)
// ═══════════════════════════════════════

fn std_insert_100k(c: &mut Criterion) {
    c.bench_function("std_insert_100k", |b| {
        b.iter(|| {
            let mut map: HashMap<u32, u32> = HashMap::with_capacity(100_000);
            for i in 0u32..100_000 {
                map.insert(i, i * 2);
            }
            black_box(&map);
        });
    });
}

fn std_lookup_100k(c: &mut Criterion) {
    let mut map: HashMap<u32, u32> = HashMap::with_capacity(100_000);
    for i in 0u32..100_000 {
        map.insert(i, i * 2);
    }
    c.bench_function("std_lookup_100k", |b| {
        b.iter(|| {
            let mut hits = 0u64;
            for i in 0u32..100_000 {
                if map.contains_key(&i) {
                    hits += 1;
                }
            }
            black_box(hits)
        });
    });
}

fn std_mixed_100k(c: &mut Criterion) {
    c.bench_function("std_mixed_100k", |b| {
        b.iter(|| {
            let mut map: HashMap<u32, u32> = HashMap::with_capacity(100_000);
            for i in 0u32..100_000 {
                map.insert(i, i * 2);
                if i > 0 {
                    black_box(map.get(&(i / 2)));
                }
            }
        });
    });
}

fn std_iterator(c: &mut Criterion) {
    let mut map: HashMap<u32, u32> = HashMap::with_capacity(50_000);
    for i in 0u32..50_000 {
        map.insert(i, i * 2);
    }
    c.bench_function("std_iter_50k", |b| {
        b.iter(|| {
            let sum: u64 = map.values().map(|&v| v as u64).sum();
            black_box(sum)
        });
    });
}

// ═══════════════════════════════════════
// ConcurrentPulseMap Benchmarks
// ═══════════════════════════════════════

use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
use std::thread;

fn concurrent_insert_1t_100k(c: &mut Criterion) {
    c.bench_function("conc_1t_insert_100k", |b| {
        b.iter(|| {
            let map = ConcurrentPulseMap::<u32, u32>::new(38461);
            for i in 0u32..100_000 {
                map.insert(i, i * 2);
            }
            black_box(map.len());
        });
    });
}

fn concurrent_insert_4t_100k(c: &mut Criterion) {
    c.bench_function("conc_4t_insert_100k", |b| {
        b.iter(|| {
            let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(38461));
            let handles: Vec<_> = (0..4u32)
                .map(|t| {
                    let m = map.clone();
                    thread::spawn(move || {
                        for i in 0..25_000u32 {
                            m.insert(t * 100_000 + i, i);
                        }
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            black_box(map.len());
        });
    });
}

fn concurrent_lookup_4t_100k(c: &mut Criterion) {
    let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(38461));
    for i in 0u32..100_000 {
        map.insert(i, i * 2);
    }
    c.bench_function("conc_4t_lookup_100k", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..4u32)
                .map(|t| {
                    let m = map.clone();
                    thread::spawn(move || {
                        let mut hits = 0u64;
                        for i in 0..25_000u32 {
                            if m.get(&(t * 25_000 + i)).is_some() {
                                hits += 1;
                            }
                        }
                        hits
                    })
                })
                .collect();
            let total: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
            black_box(total);
        });
    });
}

fn concurrent_mixed_4t_100k(c: &mut Criterion) {
    c.bench_function("conc_4t_mixed_100k", |b| {
        b.iter(|| {
            let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(38461));
            let handles: Vec<_> = (0..4u32)
                .map(|t| {
                    let m = map.clone();
                    thread::spawn(move || {
                        for i in 0..25_000u32 {
                            let key = t * 100_000 + i;
                            m.insert(key, i);
                            if i > 0 {
                                black_box(m.get(&(key - 1)));
                            }
                        }
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            black_box(map.len());
        });
    });
}

criterion_group!(
    benches,
    // PulseMap Raw
    pulse_raw_insert_100k,
    pulse_raw_lookup_100k,
    pulse_raw_mixed_100k,
    pulse_raw_eviction,
    // PulseMap Typed
    pulse_typed_insert_100k,
    pulse_typed_lookup_100k,
    pulse_typed_mixed_100k,
    pulse_typed_iterator,
    // LRU Cache (fair comparison)
    lru_insert_100k,
    lru_lookup_100k,
    lru_mixed_100k,
    lru_eviction_50k,
    // std::HashMap (reference)
    std_insert_100k,
    std_lookup_100k,
    std_mixed_100k,
    std_iterator,
    // ConcurrentPulseMap
    concurrent_insert_1t_100k,
    concurrent_insert_4t_100k,
    concurrent_lookup_4t_100k,
    concurrent_mixed_4t_100k,
);
criterion_main!(benches);
