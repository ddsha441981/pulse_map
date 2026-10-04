# PulseMap

A fixed-capacity Rust cache with bucket-local LFU+LRU eviction.

[![Crates.io](https://img.shields.io/crates/v/pulse_map.svg)](https://crates.io/crates/pulse_map)
[![Docs](https://docs.rs/pulse_map/badge.svg)](https://docs.rs/pulse_map)
[Guide](https://ddsha441981.github.io/pulse_map/) ·
[Changelog](CHANGELOG.md) · [Benchmark methodology](evaluation/README.md)

## What it does

Every bucket is a 64-byte aligned block containing four slots and their eviction
metadata. The metadata needed to choose a victim is in the bucket already fetched
for lookup: **no additional metadata cache-line fetch for the eviction decision**.
Operations still cost instructions, and TTL/slab/concurrency structures live outside
the bucket. “Zero-cost eviction” refers to that layout property, not free operations.

Use PulseMap for a bounded, disposable cache. Use `HashMap` when every entry must
be retained. A full bucket evicts locally even when other buckets have free slots.

## Quick start

```toml
pulse_map = "0.6"
```

```rust
use pulse_map::{PulseMap, TypedPulseMap};

let mut raw = PulseMap::new(16); // 16 buckets × 4 slots = 64 nominal entries
raw.insert(b"hello", b"world");
assert_eq!(raw.get(b"hello"), Some(&b"world"[..]));
assert!(raw.remove(b"hello"));

let mut typed = TypedPulseMap::<u32, u32>::new(16);
typed.insert(42, 100);
assert_eq!(typed.get(&42), Some(100));
typed.entry(42).and_modify(|v| *v += 1).or_insert(0);
assert_eq!(typed.peek(&42), Some(101));
```

Keys implement `PulseKey`, values implement `PulseValue`. Numeric built-ins are
`u8`, `u16`, `u32`, `u64`, `i32`, `i64`; both traits support `String` and `Vec<u8>`.
Arrays `[u8; N]` implement `PulseKey`; `bool` implements `PulseValue`.
Typed lookups deserialize an owned `V`; there is no indexing/reference-to-V API.

### Concurrent maps

```rust
use pulse_map::ShardedPulseMap;
use std::sync::Arc;

let cache = Arc::new(ShardedPulseMap::<u32, u32>::new(64));
// 16 shards × 64 buckets × 4 slots = 4,096 nominal entries.
let writer = cache.clone();
std::thread::spawn(move || writer.insert(7, 70)).join().unwrap();
assert_eq!(cache.get(&7), Some(70));
cache.resize_all(128);
assert_eq!(cache.get(&7), Some(70));
```

| Type | API / concurrency | Capacity argument |
|---|---|---|
| `PulseMap` / `PulseMapRaw` | Raw byte slices; `Send`, not `Sync` | Buckets |
| `TypedPulseMap<K,V>` | Single-owner typed cache | Buckets |
| `ConcurrentPulseMap<K,V>` | RwLock + per-bucket spinlocks + shared metadata/pool mutexes | Buckets |
| `ShardedPulseMap<K,V>` | 16 independent concurrent maps | Buckets **per shard** |

Reads in concurrent maps **take locks**. `get()` records a deferred priority event;
`insert()` drains at most 64 events. The bounded queue may drop events under
contention/fullness. Sequence tags prevent duplicate deliveries on slot reuse.
Its try operations do not wait, but a stalled producer can delay consumption, so
the queue is not advertised as a formally lock-free FIFO.

Concurrent types support `with_auto_resize(n)` and growth-only manual resize.
Raw/typed maps are fixed-size. Auto-resize triggers above 75% occupied slots and
does not promise to prevent all collision evictions. A shard's resize blocks that
shard; ordinary `ConcurrentPulseMap::resize` blocks the entire map.

### Insertion-epoch TTL

```rust
use pulse_map::TypedPulseMap;

let mut cache = TypedPulseMap::<u32, u32>::new(16);
cache.insert_ttl(1, 100, 1);
cache.insert(2, 200); // age = 1, still valid
assert_eq!(cache.get(&1), Some(100));
cache.insert(2, 201); // age = 2 > TTL = 1
assert_eq!(cache.get(&1), None);
```

- `set_ttl(n)` sets the default; `insert_ttl(k,v,n)` overrides it.
- `0` uses the default (default `0` disables expiry); `u64::MAX` never expires.
  Never-expire entries can still be evicted.
- Age is **insertions**, including updates, not elapsed time. Expiry is `age > ttl`.
- Each shard has its own insertion counter. Reads do not advance it or refresh TTL.
- Expiry hides entries from `get`/`peek`; it does not eagerly remove them.
  `len()` counts occupied slots, including expired entries, and `iter()` includes
  expired occupied entries. Raw insert prefers free/expired slots; concurrent insert
  currently uses free slots or the policy victim without expired-first selection.

## Memory and fit

Inline mode requires **key ≤6 bytes AND value ≤7 bytes**. `u32 → u32` fits;
ordinary `u64` keys or values do not. Larger payloads use a reusable heap-backed
slab. Fixed entry capacity is **not a fixed byte budget** for arbitrary-size payloads.

Raw/typed maps allocate 128 bytes per bucket: 64 for the bucket plus 64 for four
TTL records. Inline insert/get/remove make no further heap allocations. Concurrent
lookups currently copy value bytes into a temporary vector, including inline hits.

The v0.6.6 candidate's access queue uses two `usize` atomics per event. On x86_64,
4,096 events cost 64 KiB per Concurrent map (1 MiB over 16 shards), plus locks and
map storage. This is **48 KiB/map more than v0.6.5**, paid for sequence-tagged reuse.

Measured niche: compact-key, memory-conscious caches with scan-heavy traffic.
The policy retains frequent entries through cold scans; frequency does not age,
so changing hot sets can adapt poorly. Allocate headroom and evaluate your trace.
General-purpose read-heavy throughput often favours QuickCache in our measurements.

### Benchmarks and evidence

[`evaluation/`](evaluation/README.md) runs the exact published v0.6.5 baseline and
local candidate in one optimized executable with pinned competitors. It reports:

- equal actual nominal capacities and identical seeded traces;
- read/write counts, resident entries and matching u32/u32 or u64/u64 payloads;
- Zipf, uniform, repeated scan, hot+scan and changing-hot-set hit rates;
- 1/4/8-thread throughput and fresh-process RSS with both slot/resident denominators.

See [baseline](evaluation/BASELINE.md), [ticket evidence](evaluation/PROGRESS.md),
and [raw paired candidate results](evaluation/results/after-correctness/).
All performance/RSS figures here are **host measurements**, not MCU timings.
Historical v0.6.2 headline speed/memory rankings are superseded by versioned results:
some old harnesses used rounded, unequal capacities and nominal-entry denominators.
Correctness fixes can add overhead; the candidate is not universally faster.

## Embedded / no_std

```toml
pulse_map = { version = "0.6", default-features = false }
# Targets without atomic CAS also need a critical-section implementation from the HAL:
# pulse_map = { version = "0.6", default-features = false, features = ["critical-section"] }
```

Requires `alloc`. Core raw/typed caches support no_std; concurrent maps require std.
CI checks Cortex-M3/M4F/M33, Cortex-M0, RISC-V with/without atomic extension,
AArch64 bare metal and WASM. `qemu-test/` executes Cortex-M0 and M3 checks.

QEMU evidence: a 16-bucket inline map uses 2,048 heap bytes with two construction
allocations and no further allocations for inline operations. The historical
64-resident-entry footprint probe measured 2,048 B vs LRU's 4,520 B RAM. Flash cost
depends on target/toolchain; on the measured M3 build LRU was smaller. Every raw/typed
get hit updates an AtomicU64, which can mask interrupts on M0. No physical-MCU timing
claim follows from QEMU; see [embedded guide](docs/src/embedded-no-std.md).

## Features, bindings and validation

| Feature | Default | Purpose |
|---|---|---|
| `std` | yes | Concurrent/sharded maps, `From<HashMap>` |
| `simd` | no | SSE2 H2 matching on x86_64 |
| `critical-section` | no | portable-atomic fallback on targets without CAS |

C/Python/Java/Node bindings live in the separate
[pulse_map_bindings](https://github.com/ddsha441981/pulse_map_bindings) project.
This crate contains no C ABI or language bindings; use that project's versioned
build/API instructions.

```bash
cargo test --all-features
cargo test --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
RUSTFLAGS='--cfg loom' cargo test --test loom_meta --test loom_access_buffer
cargo +nightly miri test --lib
cargo test --manifest-path evaluation/Cargo.toml --locked
```

Guide Rust examples are included in rustdoc tests. CI also exercises fuzz/Miri/Loom
infrastructure and embedded targets; passing tests validate their covered cases,
not every possible workload.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
