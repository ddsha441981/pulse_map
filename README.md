# PulseMap

**A CPU cache-line hash table with zero-cost eviction.**

[![Crate](https://img.shields.io/crates/v/pulse_map.svg)](https://crates.io/crates/pulse_map)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> Every bucket fits in exactly **one 64-byte CPU cache line** with embedded LFU+LRU eviction metadata. Eviction decisions cost **zero additional cache misses**.

## What Is PulseMap?

PulseMap is a **hash table with built-in eviction** — not a HashMap replacement.

Use **HashMap** when you need to store all data forever.
Use **PulseMap** when you need a **fixed-memory cache** that automatically evicts cold entries.

```
HashMap       = Store everything, memory grows unbounded
PulseMap      = Store hot data only, memory stays fixed, cold data evicted
```

## Why PulseMap?

| Problem | `HashMap + LRU list` | PulseMap |
|---------|---------------------|----------|
| Two data structures to manage | HashMap + linked list | **Single structure** |
| Eviction = extra cache misses | 2-3 pointer chases per eviction | **Zero extra fetches** |
| Memory overhead | Pointers for LRU list (~48B/entry) | **Packed in 14B/entry** |
| Cache-friendliness | Random pointer chasing | **64-byte aligned, 1 fetch** |

## Quick Start

### Raw API (`&[u8]` keys — power users)

```rust
use pulse_map::PulseMap;

let mut map = PulseMap::new(1024); // 1024 buckets × 4 slots = 4096 capacity
map.insert(b"hello", b"world");
assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
map.remove(b"hello");
assert_eq!(map.get(b"hello"), None);
```

### Typed API (generic keys — recommended)

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

### Supported Types

Built-in `PulseKey`/`PulseValue` implementations (zero-alloc for numerics):

`u8` · `u16` · `u32` · `u64` · `i32` · `i64` · `String` · `Vec<u8>` · `[u8; N]` · `bool`

## Benchmark Results (v0.3.0)

### PulseMap vs `lru` crate (same category — bounded cache)

| Benchmark (100K ops) | PulseMap | `lru` crate | Result |
|---------------------|:-------:|:-----------:|:------:|
| **INSERT** | **15 ms** | 32 ms | ✅ **2.1x faster** |
| **MIXED (insert+lookup)** | **32 ms** | 44 ms | ✅ **1.4x faster** |
| **EVICTION (50K)** | **1.8 ms** | 3.2 ms | ✅ **1.8x faster** |
| LOOKUP | 18 ms | **8.3 ms** | lru 2.2x faster |

### Reference: PulseMap vs std::HashMap (different category)

| Benchmark (100K ops) | PulseMap | std::HashMap | Note |
|---------------------|:-------:|:------------:|:----:|
| INSERT | 15 ms | 3.6 ms | std has no eviction |
| LOOKUP | 18 ms | 4.3 ms | std uses SIMD + native types |
| EVICTION | **1.8 ms** | impossible | ∞ |

> **Note:** std::HashMap and PulseMap solve different problems. HashMap stores everything forever.
> PulseMap is a bounded cache. Compare with `lru`/`moka` for fair comparison.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                    ONE CPU CACHE LINE (64 bytes)                  │
├──────────────────────────────────────────────────────────────────┤
│  MetaWord (8 bytes)                                              │
│  ┌─────────┬──────────────┬───────────────────────────────────┐  │
│  │ States  │  H2 Finger-  │  Priority Scores                 │  │
│  │ 4×2 bit │  prints 4×7b │  4×7 bit (freq[4]+recency[3])    │  │
│  └─────────┴──────────────┴───────────────────────────────────┘  │
│                                                                  │
│  Slot 0 (14B) │ Slot 1 (14B) │ Slot 2 (14B) │ Slot 3 (14B)     │
│                                                                  │
│  Total: 8 + (4 × 14) = 64 bytes ✓                               │
└──────────────────────────────────────────────────────────────────┘
```

### Layered Design

```
Layer 3: lib.rs          → User API (TypedPulseMap<K,V>, PulseMap alias)
Layer 2: raw.rs          → Hash table logic (insert/get/remove/evict)
Layer 1: core/           → Building blocks (MetaWord, Slot, Bucket, hash)
```

## Project Structure

```
pulse_map/
├── Cargo.toml
├── README.md
├── CHANGELOG.md
├── src/
│   ├── lib.rs              # Public API (PulseMap, TypedPulseMap<K,V>, Entry API)
│   ├── raw.rs              # PulseMapRaw — raw byte engine (power-of-2 + prefetch)
│   ├── iter.rs             # RawIter + TypedIter
│   ├── traits.rs           # Debug, Display, Extend, From<HashMap>
│   ├── simd.rs             # SIMD H2 matching (--features simd, x86_64)
│   └── core/               # Core engine (64-byte internals)
│       ├── mod.rs           # Module re-exports
│       ├── meta.rs          # MetaWord (branchless match_mask + SIMD dispatch)
│       ├── slot.rs          # Slot (14-byte inline/slab)
│       ├── bucket.rs        # Bucket (64B cache-line aligned)
│       ├── slab.rs          # SlabPool (arena allocator)
│       └── hash.rs          # wyhash → H1/H2/ext_fp
└── benches/
    └── benchmark.rs         # Criterion benchmarks (vs lru + std)
```

## API Reference

### PulseMap (raw `&[u8]` API)

| Method | Description |
|--------|-------------|
| `PulseMap::new(num_buckets)` | Create with fixed capacity |
| `insert(&mut self, &[u8], &[u8])` | Insert or update (evicts on full) |
| `get(&self, &[u8]) → Option<&[u8]>` | Lookup (updates priority) |
| `peek(&self, &[u8]) → Option<&[u8]>` | Lookup (no priority update) |
| `remove(&mut self, &[u8]) → bool` | Delete a key |
| `len()`, `capacity()`, `load_factor()` | Stats |

### TypedPulseMap<K, V> (generic API)

| Method | Description |
|--------|-------------|
| `TypedPulseMap::<K,V>::new(n)` | Create typed map |
| `insert(K, V)` | Typed insert |
| `get(&K) → Option<V>` | Typed lookup |
| `peek(&K) → Option<V>` | Lookup (no priority update) |
| `contains_key(&K) → bool` | Existence check |
| `remove(&K) → bool` | Typed removal |
| `entry(K) → Entry` | Entry API (or_insert, and_modify) |
| `iter() → TypedIter<K,V>` | Iterate all pairs |
| `extend(IntoIterator)` | Bulk insert |
| `From<HashMap<K,V>>` | Convert from std::HashMap |

## Feature Flags

| Feature | Default | Description |
|---------|:-------:|-------------|
| `std` | ✅ | Enables `From<HashMap>`, standard library |
| `simd` | ❌ | SSE2 SIMD H2 matching (x86_64 only) |

```toml
# Default (std)
pulse_map = "0.3"

# With SIMD
pulse_map = { version = "0.3", features = ["simd"] }

# no_std
pulse_map = { version = "0.3", default-features = false }
```

### Design Notes

- **`map[&key]` (Index trait) is intentionally NOT supported.** PulseMap stores bytes and deserializes on read — it returns `V` (owned copy), not `&V` (reference). Use `.get(&key)` instead.

## Use Cases

- **DNS cache** — bounded, hot domains stay
- **API rate limiter** — per-IP counters with auto-cleanup
- **Database query cache** — fixed memory, LRU+LFU eviction
- **CDN edge cache** — hot content stays, cold evicted
- **Network routers** — per-packet, latency-critical lookup
- **Embedded systems** — deterministic memory, no heap growth
- **Game engines** — asset cache with fixed VRAM budget

## Author

<table>
<tr>
<td>

**Deendayal Kumawat**

[![LinkedIn](https://img.shields.io/badge/LinkedIn-0077B5?style=flat-square&logo=linkedin&logoColor=white)](https://www.linkedin.com/in/deendayal-kumawat/)
[![GitHub](https://img.shields.io/badge/GitHub-181717?style=flat-square&logo=github&logoColor=white)](https://github.com/ddsha441981)
[![Email](https://img.shields.io/badge/Email-0078D4?style=flat-square&logo=microsoft-outlook&logoColor=white)](mailto:deendayal_kumawat@outlook.com)

</td>
</tr>
</table>

---

## License

Licensed under either of:

- **Apache License, Version 2.0** — [LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>
- **MIT License** — [LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>

at your option.
