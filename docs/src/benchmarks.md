# Performance & Benchmarks

> Results from v0.6.0 · Criterion · Linux · i7-8650U

## PulseMap vs `lru` crate (same category — bounded cache)

| Benchmark (100K ops) | PulseMap | `lru` crate | Result |
|---------------------|:-------:|:-----------:|:------:|
| **INSERT** | **13.8 ms** | 19.1 ms | ✅ **1.4x faster** |
| **MIXED (insert+lookup)** | **16.0 ms** | 23.7 ms | ✅ **1.5x faster** |
| **EVICTION (50K)** | **1.5 ms** | 2.2 ms | ✅ **1.5x faster** |
| LOOKUP | 9.8 ms | **5.4 ms** | lru 1.8x faster |

> **Why is lookup slower?** `lru` stores typed values as native pointers. PulseMap stores serialized
> bytes — enabling `no_std`, multi-language bindings, and a stable memory layout.
> Closing this gap is the v0.7.0 roadmap item.

## v0.5.0 → v0.6.0 Improvement

| Benchmark | v0.5.0 | v0.6.0 | Delta |
|-----------|:------:|:------:|:-----:|
| raw_mixed_100k | 17.46 ms | **16.0 ms** | ✅ -8% |
| raw_eviction_50k | 1.10 ms | 1.10 ms | no change |
| raw_lookup_100k | 7.22 ms | 7.22 ms | no change |

The -8% mixed improvement comes from `remove()` now using `match_mask()` (branchless)
instead of the old 8-call per-slot brute-force loop.

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

# Specific benchmark
cargo bench -- insert
cargo bench -- concurrent
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
| Many threads, same key | Bucket spinlock contention | Shard across multiple maps |
| Resize during load | Stop-the-world pause | Pre-size correctly |
| Large keys (>6B) | Slab allocation | Use short keys when possible |
| >4 entries/bucket | Eviction overhead | Increase bucket count |
