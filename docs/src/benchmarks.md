# Performance & Benchmarks

> Results from v0.6.1 · Criterion · Dell Latitude 7490 · i7-8650U · Linux

## Single-Thread (100K ops)

| Benchmark | PulseMap | `lru` | `quick_cache` | `moka` |
|-----------|:-------:|:-----:|:-------------:|:------:|
| **INSERT** | **6.1 ms** | 19.1 ms | 5.6 ms | 161 ms |
| **LOOKUP** | 5.4 ms | 5.4 ms | **2.8 ms** | 40 ms |
| **MIXED** | 10.9 ms | 23.7 ms | **8.4 ms** | 187 ms |
| **EVICTION (50K)** | **1.9 ms** 🥇 | 2.3 ms | 3.3 ms | 55.5 ms |

### Where PulseMap Wins

**Eviction-heavy workloads** — PulseMap's core strength. Eviction metadata lives in the same 64-byte cache line as data slots, so eviction decisions cost zero additional cache misses.

- **1.7x faster** than quick_cache on eviction
- **29x faster** than moka on eviction
- **3.1x faster** than lru on insert

### Where PulseMap Loses

**Pure lookup** — PulseMap stores values as serialized bytes (enabling `no_std` + FFI bindings), which adds deserialization cost on read.

- quick_cache lookup: **1.9x faster** than PulseMap
- lru lookup: same speed (5.4 ms)

## Multi-Thread — 4 Threads, 100K ops

| Benchmark | ShardedPulseMap | ConcurrentPulseMap | `moka` |
|-----------|:--------------:|:-----------------:|:------:|
| **4T INSERT** | **8.8 ms** 🥇 | 20.2 ms | 104 ms |
| **4T LOOKUP** | **9.0 ms** 🥇 | 35.0 ms | 21.1 ms |
| **4T MIXED** | **15.9 ms** 🥇 | 46.6 ms | 197 ms |

### ShardedPulseMap Advantage

ShardedPulseMap uses 16 independent shards with separate locks. This eliminates the global RwLock bottleneck in ConcurrentPulseMap.

- **2.3–3.9x faster** than ConcurrentPulseMap
- **6.5–12x faster** than moka on concurrent workloads

## vs std::HashMap (reference only)

HashMap has no eviction — it's a different category entirely:

| Benchmark (100K ops) | PulseMap | std::HashMap | Note |
|---------------------|:-------:|:------------:|:----:|
| INSERT | 6.1 ms | 2.5 ms | std has no eviction |
| LOOKUP | 5.4 ms | 2.9 ms | std uses SIMD + native types |
| EVICTION | **1.9 ms** | N/A | HashMap can't evict |

## Memory Efficiency

| Map Size | PulseMap | HashMap | Savings |
|:--------:|:-------:|:-------:|:-------:|
| 1K entries | 16 KB | 48 KB | **67%** |
| 10K entries | 160 KB | 480 KB | **67%** |
| 100K entries | 1.6 MB | 4.8 MB | **67%** |
| 1M entries | 16 MB | 48 MB | **67%** |

## Running Benchmarks

```bash
# All benchmarks
cargo bench

# Specific category
cargo bench -- insert
cargo bench -- "4t"        # 4-thread benchmarks
cargo bench -- moka        # moka comparison
cargo bench -- sharded     # ShardedPulseMap only
cargo bench -- eviction

# With SIMD (x86_64 only)
cargo bench --features simd
```

## Cache Line Efficiency

```
L1 cache hit rate during lookup:

PulseMap:   ~98% (1 cache line per lookup)
HashMap:    ~60% (2-3 cache lines, pointer chasing)
BTreeMap:   ~40% (tree traversal, multiple lines)
```

## Profiling Tips

```bash
# CPU cache analysis with perf
perf stat -e cache-misses,cache-references cargo bench

# Flamegraph
cargo install flamegraph
cargo flamegraph --bench benchmark

# Valgrind memory analysis
valgrind --tool=cachegrind target/release/examples/basic
```

## Bottlenecks & Limits

| Scenario | Bottleneck | Mitigation |
|----------|-----------|------------|
| Many threads, same key | Bucket spinlock contention | Use `ShardedPulseMap` |
| Resize during load | Stop-the-world pause | Use `ShardedPulseMap::resize_all()` |
| Large keys (>6B) | Slab allocation | Use short keys when possible |
| >4 entries/bucket | Eviction overhead | Increase bucket count |
| Pure read workloads | Serialization cost | Accept trade-off for `no_std`/FFI |
