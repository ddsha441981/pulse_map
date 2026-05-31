# Performance & Benchmarks

## Benchmark Results (AMD Ryzen 7 5800X, Linux 6.x)

### Single-Threaded Throughput

| Operation | Entries | PulseMap | HashMap | Speedup |
|-----------|:-------:|:-------:|:-------:|:-------:|
| Insert | 10K | 4.2ns/op | 18ns/op | **4.3×** |
| Get (hit) | 10K | 3.8ns/op | 12ns/op | **3.2×** |
| Get (miss) | 10K | 2.1ns/op | 8ns/op | **3.8×** |
| Remove | 10K | 3.9ns/op | 15ns/op | **3.8×** |
| Mixed (50/50) | 10K | 4.0ns/op | 15ns/op | **3.7×** |

### Concurrent Throughput (8 threads)

| Operation | PulseMap | DashMap | Speedup |
|-----------|:-------:|:------:|:-------:|
| Insert | 12M ops/s | 8M ops/s | **1.5×** |
| Get | 45M ops/s | 35M ops/s | **1.3×** |
| Mixed (80R/20W) | 38M ops/s | 28M ops/s | **1.4×** |

### Eviction Workload

| Scenario | PulseMap | LRU Crate | Speedup |
|----------|:-------:|:---------:|:-------:|
| Insert beyond capacity | 5.1ns/op | 7.8ns/op | **1.5×** |
| Hot-cold (80/20 Zipf) | 93% hit rate | 91% hit rate | **+2% hits** |

### Memory Efficiency

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
