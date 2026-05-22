use criterion::{criterion_group, criterion_main, Criterion, black_box};
use pulse_map::PulseMap;
use std::collections::HashMap;

// ═══════════════════════════════════════
// PulseMap Benchmarks
// ═══════════════════════════════════════

fn pulse_insert_100k(c: &mut Criterion) {
    c.bench_function("pulse_insert_100k", |b| {
        b.iter(|| {
            let mut map = PulseMap::new(38461); // ~65% load
            for i in 0u32..100_000 {
                map.insert(&i.to_le_bytes(), b"val");
            }
            black_box(&map);
        });
    });
}

fn pulse_lookup_100k(c: &mut Criterion) {
    let mut map = PulseMap::new(38461);
    for i in 0u32..100_000 {
        map.insert(&i.to_le_bytes(), b"val");
    }

    c.bench_function("pulse_lookup_100k", |b| {
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

fn pulse_mixed_100k(c: &mut Criterion) {
    c.bench_function("pulse_mixed_100k", |b| {
        b.iter(|| {
            let mut map = PulseMap::new(38461);
            // 50% insert, 50% lookup
            for i in 0u32..100_000 {
                map.insert(&i.to_le_bytes(), b"val");
                if i > 0 {
                    let lookup_key = (i / 2).to_le_bytes();
                    black_box(map.get(&lookup_key));
                }
            }
        });
    });
}

fn pulse_eviction_pressure(c: &mut Criterion) {
    c.bench_function("pulse_eviction_50k_into_1k", |b| {
        b.iter(|| {
            let mut map = PulseMap::new(256); // 1024 slots
            for i in 0u32..50_000 {
                map.insert(&i.to_le_bytes(), b"val");
            }
            black_box(map.eviction_count());
        });
    });
}

// ═══════════════════════════════════════
// std::HashMap Benchmarks (baseline)
// ═══════════════════════════════════════

fn std_insert_100k(c: &mut Criterion) {
    c.bench_function("std_insert_100k", |b| {
        b.iter(|| {
            let mut map = HashMap::with_capacity(100_000);
            for i in 0u32..100_000 {
                map.insert(i.to_le_bytes().to_vec(), b"val".to_vec());
            }
            black_box(&map);
        });
    });
}

fn std_lookup_100k(c: &mut Criterion) {
    let mut map = HashMap::with_capacity(100_000);
    for i in 0u32..100_000 {
        map.insert(i.to_le_bytes().to_vec(), b"val".to_vec());
    }

    c.bench_function("std_lookup_100k", |b| {
        b.iter(|| {
            let mut hits = 0u64;
            for i in 0u32..100_000 {
                if map.get(&i.to_le_bytes().to_vec()).is_some() {
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
            let mut map: HashMap<Vec<u8>, Vec<u8>> = HashMap::with_capacity(100_000);
            for i in 0u32..100_000 {
                map.insert(i.to_le_bytes().to_vec(), b"val".to_vec());
                if i > 0 {
                    let lookup_key = (i / 2).to_le_bytes().to_vec();
                    black_box(map.get(&lookup_key));
                }
            }
        });
    });
}

criterion_group!(
    benches,
    pulse_insert_100k,
    pulse_lookup_100k,
    pulse_mixed_100k,
    pulse_eviction_pressure,
    std_insert_100k,
    std_lookup_100k,
    std_mixed_100k,
);
criterion_main!(benches);
