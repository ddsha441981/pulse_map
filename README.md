# PulseMap

A fixed-capacity hash table with built-in LFU+LRU eviction, written in Rust.

[![Crate](https://img.shields.io/crates/v/pulse_map.svg)](https://crates.io/crates/pulse_map)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-58%20passing-brightgreen)]()

---

## What Is PulseMap?

PulseMap is a **hash table with built-in eviction** — not a HashMap replacement.

Use **HashMap** when you need to store all data indefinitely.
Use **PulseMap** when you need a **fixed-memory cache** that automatically evicts cold entries.

```
HashMap  → stores everything, memory grows unbounded
PulseMap → stores hot data, fixed capacity, cold entries evicted automatically
```

The closest comparison in the Rust ecosystem is the [`lru`](https://crates.io/crates/lru) crate.

---

## Why PulseMap?

| Problem | `HashMap + LRU list` | PulseMap |
|---------|---------------------|----------|
| Two structures to manage | HashMap + linked list | Single structure |
| Eviction needs extra fetches | 2–3 pointer chases per eviction | Metadata is in the same bucket |
| Memory per entry | ~48B (LRU pointers) | 14B (packed slot) |
| Cache alignment | Random pointer chasing | 64-byte aligned bucket |

---

## Quick Start

### Raw API (`&[u8]` — maximum control)

```rust
use pulse_map::PulseMap;

let mut map = PulseMap::new(1024); // 1024 buckets × 4 slots = 4096 capacity
map.insert(b"hello", b"world");
assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
map.remove(b"hello");
assert_eq!(map.get(b"hello"), None);
```

### Typed API (recommended)

```rust
use pulse_map::TypedPulseMap;

let mut map = TypedPulseMap::<u32, u64>::new(256);
map.insert(42, 100);
assert_eq!(map.get(&42), Some(100));

// Iterate
for (key, value) in map.iter() {
    println!("{}: {}", key, value);
}

// Bulk insert
map.extend(vec![(1, 10), (2, 20), (3, 30)]);

// From std::HashMap
use std::collections::HashMap;
let std_map: HashMap<u32, u32> = HashMap::from([(1, 10), (2, 20)]);
let pulse = TypedPulseMap::from(std_map);

// Stats
println!("{}", map); // PulseMap(4/1024 entries, 0.4% load, 0 evictions)
```

### Concurrent API (thread-safe)

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
use std::thread;

let map = Arc::new(ConcurrentPulseMap::<u32, u32>::new(1024));

// All methods take &self — safe to share across threads without Mutex
let handles: Vec<_> = (0..4).map(|t| {
    let m = map.clone();
    thread::spawn(move || {
        for i in 0..1000 {
            m.insert(t * 1000 + i, i);
        }
    })
}).collect();
for h in handles { h.join().unwrap(); }

// Auto-resize mode
let growing_map = ConcurrentPulseMap::<u32, u32>::with_auto_resize(64);
// Map auto-grows when load > 75%
```

### ShardedPulseMap (16-shard, no global lock) — v0.6.1+

```rust
use pulse_map::ShardedPulseMap;
use std::sync::Arc;
use std::thread;

// 16 independent shards — near-zero cross-thread contention
let map = Arc::new(ShardedPulseMap::<u32, u32>::new(4096)); // 4096 buckets/shard

let handles: Vec<_> = (0..8).map(|t| {
    let m = map.clone();
    thread::spawn(move || {
        for i in 0..10_000 {
            m.insert(t * 10_000 + i, i);
        }
    })
}).collect();
for h in handles { h.join().unwrap(); }

// resize_all() rehashes one shard at a time — no stop-the-world
map.resize_all(8192);
```

### TTL — Automatic Expiry (v0.6.0+)

```rust
use pulse_map::PulseMap;

let mut cache = PulseMap::new(1024);

// Entries expire after 500 insertions
cache.set_ttl(500);

cache.insert(b"session:abc", b"user_data");

// ...500+ inserts later...
for i in 0u32..501 {
    cache.insert(&i.to_le_bytes(), b"other");
}

assert_eq!(cache.get(b"session:abc"), None); // expired

// Re-inserting refreshes the epoch
cache.insert(b"session:abc", b"refreshed");
assert_eq!(cache.get(b"session:abc"), Some(&b"refreshed"[..]));

println!("TTL: {} epochs", cache.get_ttl());     // 500
println!("Epoch: {}", cache.current_epoch());    // total inserts
```

> TTL is measured in insertion count, not wall-clock time.
> `set_ttl(0)` disables TTL (default — zero overhead).

### Per-Entry TTL (v0.6.1+)

```rust
use pulse_map::PulseMap;

let mut cache = PulseMap::new(1024);
cache.set_ttl(500); // default: 500 inserts

// Per-entry override
cache.insert_ttl(b"session", b"data", 50);      // this entry: 50 inserts
cache.insert_ttl(b"config", b"val", u32::MAX);  // this entry: never expires
cache.insert(b"normal", b"val");                // uses default TTL = 500
```

> `ttl = 0`: use global default. `u32::MAX`: never expire. `N`: expire after N inserts.

### Supported Types

Built-in `PulseKey`/`PulseValue` implementations (zero heap allocation for numeric types):

`u8` · `u16` · `u32` · `u64` · `i32` · `i64` · `String` · `Vec<u8>` · `[u8; N]` · `bool`

Implement `PulseKey` / `PulseValue` for custom types.

---

## Benchmark Results (v0.6.1)

Criterion results on Dell Latitude 7490. Your numbers will vary by hardware.

### Single-Thread (100K ops)

| Benchmark | PulseMap | `lru` | `quick_cache` | `moka` |
|-----------|:-------:|:-----:|:-------------:|:------:|
| **INSERT** | **6.1 ms** | 19.1 ms | 5.6 ms | 161 ms |
| **LOOKUP** | 5.4 ms | 5.4 ms | **2.8 ms** | 40 ms |
| **MIXED** | 10.9 ms | 23.7 ms | **8.4 ms** | 187 ms |
| **EVICTION (50K)** | **1.9 ms** 🥇 | 2.3 ms | 3.3 ms | 55.5 ms |

**Where PulseMap wins:** Eviction-heavy workloads (1.7x faster than quick_cache, 29x faster than moka). This is PulseMap's core strength — metadata lives in the same cache line as data.

**Where PulseMap loses:** Pure lookup is ~1.9x behind quick_cache (serialization overhead for `no_std`/FFI compatibility).

### Multi-Thread — 4 Threads, 100K ops

| Benchmark | ShardedPulseMap | ConcurrentPulseMap | `moka` |
|-----------|:--------------:|:-----------------:|:------:|
| **4T INSERT** | **8.8 ms** 🥇 | 20.2 ms | 104 ms |
| **4T LOOKUP** | **9.0 ms** 🥇 | 35.0 ms | 21.1 ms |
| **4T MIXED** | **15.9 ms** 🥇 | 46.6 ms | 197 ms |

ShardedPulseMap: **2.3x faster** than ConcurrentPulseMap, **6.5-12x faster** than moka on concurrent workloads.

### vs std::HashMap (different category — reference only)

| Benchmark (100K ops) | PulseMap | std::HashMap | Note |
|---------------------|:-------:|:------------:|:----:|
| INSERT | 6.1 ms | 2.5 ms | std has no eviction |
| LOOKUP | 5.4 ms | 2.9 ms | std uses SIMD + native types |
| EVICTION | **1.9 ms** | not possible | — |

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                    ONE BUCKET (64 bytes, cache-line aligned)      │
├──────────────────────────────────────────────────────────────────┤
│  MetaWord (8 bytes)                                              │
│  ┌─────────┬──────────────┬───────────────────────────────────┐  │
│  │ States  │  H2 Finger-  │  Priority Scores                 │  │
│  │ 4×2 bit │  prints 4×7b │  4×7 bit (freq[4] + recency[3]) │  │
│  └─────────┴──────────────┴───────────────────────────────────┘  │
│                                                                  │
│  Slot 0 (14B) │ Slot 1 (14B) │ Slot 2 (14B) │ Slot 3 (14B)     │
│                                                                  │
│  Total: 8 + (4 × 14) = 64 bytes ✓                               │
└──────────────────────────────────────────────────────────────────┘
```

**Slot storage modes:**

```
Inline mode (mode bit = 0) — key ≤ 6B, value ≤ 7B:
  [header][key bytes 1..6][value bytes 7..13]

Slab mode (mode bit = 1) — larger keys or values:
  [header][ext fingerprint][slab index → heap-allocated SlabEntry]
```

### Layered Design

```
Layer 5: sharded.rs → ShardedPulseMap (16 × ConcurrentPulseMap, shard-per-key)
Layer 4: sync.rs    → ConcurrentPulseMap (per-bucket spinlocks + RwLock for resize)
Layer 3: lib.rs     → TypedPulseMap<K,V>, Entry API, PulseKey/PulseValue traits
Layer 2: raw.rs     → PulseMapRaw — insert/get/remove/evict/TTL/per-entry-TTL
Layer 1: engine/    → MetaWord, Slot, Bucket, SlabPool, hash (wyhash)
```

---

## Project Structure

```
pulse_map/
├── Cargo.toml                         # Package manifest
├── src/
│   ├── lib.rs                         # Public API (TypedPulseMap, Entry, traits)
│   ├── raw.rs                         # PulseMapRaw (TTL, per-entry TTL, eviction, slab)
│   ├── sync.rs                        # ConcurrentPulseMap (spinlock + RwLock)
│   ├── sharded.rs                     # ShardedPulseMap (16-shard, no global lock)
│   ├── iter.rs                        # RawIter, TypedIter
│   ├── traits.rs                      # Debug, Display, Extend, From<HashMap>
│   ├── simd.rs                        # SIMD H2 matching (x86_64, optional)
│   └── engine/                        # MetaWord, Slot, Bucket, SlabPool, hash
├── benches/benchmark.rs               # Criterion benchmarks (PulseMap, lru, moka, quick_cache)
├── docs/                              # mdBook documentation
└── examples/                          # basic, concurrent examples
```

---

## API Reference

### PulseMap (raw `&[u8]`)

| Method | Description |
|--------|-------------|
| `PulseMap::new(num_buckets)` | Create with fixed capacity |
| `insert(&mut self, &[u8], &[u8])` | Insert or update (evicts on full bucket) |
| `get(&self, &[u8]) → Option<&[u8]>` | Lookup, updates LFU+LRU priority |
| `peek(&self, &[u8]) → Option<&[u8]>` | Lookup, no priority update |
| `remove(&mut self, &[u8]) → bool` | Delete |
| `set_ttl(u32)` | Expiry in insertion epochs (0 = disabled) |
| `get_ttl() → u32` | Current TTL setting |
| `current_epoch() → u32` | Total insertions |
| `len()`, `capacity()`, `load_factor()`, `eviction_count()` | Stats |

### TypedPulseMap\<K, V\>

| Method | Description |
|--------|-------------|
| `TypedPulseMap::<K,V>::new(n)` | Create typed map |
| `insert(K, V)` | Typed insert |
| `get(&K) → Option<V>` | Typed lookup |
| `peek(&K) → Option<V>` | Lookup, no priority update |
| `contains_key(&K) → bool` | Check existence |
| `remove(&K) → bool` | Delete |
| `entry(K) → Entry` | Entry API (`or_insert`, `and_modify`) |
| `iter() → TypedIter<K,V>` | Iterate all live entries |
| `extend(IntoIterator)` | Bulk insert |
| `From<HashMap<K,V>>` | Convert from std::HashMap |
| `set_ttl(u32)` / `get_ttl()` / `current_epoch()` | TTL |

### ConcurrentPulseMap\<K, V\>

| Method | Description |
|--------|-------------|
| `ConcurrentPulseMap::new(n)` | Fixed-size concurrent map |
| `ConcurrentPulseMap::with_auto_resize(n)` | Auto-grows at 75% load |
| `insert(&self, K, V)` | Thread-safe insert (no `&mut` needed) |
| `get(&self, &K) → Option<V>` | Thread-safe lookup |
| `peek(&self, &K) → Option<V>` | Lookup, no priority update |
| `remove(&self, &K) → bool` | Thread-safe delete |
| `contains_key(&self, &K) → bool` | Check existence |
| `resize(&self, new_size)` | Manual rehash (stop-the-world) |
| `insert_ttl(&self, K, V, u32)` | Thread-safe insert with per-entry TTL |
| `len()`, `capacity()`, `load_factor()` | Stats |

### ShardedPulseMap\<K, V\>

| Method | Description |
|--------|-------------|
| `ShardedPulseMap::new(buckets_per_shard)` | 16-shard concurrent map |
| `ShardedPulseMap::with_auto_resize(n)` | Auto-grows each shard at 75% load |
| `insert(&self, K, V)` | Thread-safe, routed to shard by hash |
| `insert_ttl(&self, K, V, u32)` | Per-entry TTL insert |
| `get(&self, &K) → Option<V>` | Thread-safe lookup |
| `remove(&self, &K) → bool` | Thread-safe delete |
| `resize_all(&self, n)` | Per-shard rehash (no stop-the-world) |
| `set_ttl(u32)` / `get_ttl()` | TTL applied to all shards |
| `len()`, `capacity()`, `load_factor()` | Aggregated stats |

---

## Feature Flags

| Feature | Default | Description |
|---------|:-------:|-------------|
| `std` | ✅ | `ConcurrentPulseMap`, `From<HashMap>`, std traits |
| `simd` | ❌ | SSE2 H2 matching (x86_64 only) |

```toml
# Default
pulse_map = "0.6"

# With SIMD
pulse_map = { version = "0.6", features = ["simd"] }

# no_std (disables ConcurrentPulseMap)
pulse_map = { version = "0.6", default-features = false }
```

> **Note:** `map[&key]` (Index trait) is not implemented. PulseMap returns owned `V` values,
> not references. Use `.get(&key)` instead.

---

## C FFI Bindings (v0.5.0+)

```c
#include "pulse_map.h"
PulseMapHandle *map = pulse_map_new(1024);
pulse_map_insert(map, "hello", 5, "world", 5);
pulse_map_free(map);
```

> Python, Java, and Node.js bindings are available in separate workspace crates.

---

## Use Cases

- **DNS cache** — bounded memory, hot domains stay in
- **API rate limiter** — per-IP counters with automatic cleanup
- **Database query cache** — fixed memory, evicts cold queries
- **Session store** — use TTL to expire old sessions
- **CDN / edge cache** — hot content stays, cold evicted
- **Embedded systems** — predictable memory, no heap growth

---

## Known Limitations

- Lookup is ~1.9x slower than `quick_cache` — serialization trade-off for `no_std`/FFI
- TTL is insertion-count based, not wall-clock time
- No async API yet

---

## Author

**Deendayal Kumawat**

[![LinkedIn](https://img.shields.io/badge/LinkedIn-0077B5?style=flat-square&logo=linkedin&logoColor=white)](https://www.linkedin.com/in/deendayal-kumawat/)
[![GitHub](https://img.shields.io/badge/GitHub-181717?style=flat-square&logo=github&logoColor=white)](https://github.com/ddsha441981)
[![Email](https://img.shields.io/badge/Email-0078D4?style=flat-square&logo=microsoft-outlook&logoColor=white)](mailto:deendayal_kumawat@outlook.com)

---

## License

Licensed under either of:

- **Apache License, Version 2.0** — [LICENSE-APACHE](LICENSE-APACHE)
- **MIT License** — [LICENSE-MIT](LICENSE-MIT)

at your option.
