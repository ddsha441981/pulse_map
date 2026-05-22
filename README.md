# PulseMap

**A CPU cache-line hash table with zero-cost eviction.**

[![Crate](https://img.shields.io/crates/v/pulse_map.svg)](https://crates.io/crates/pulse_map)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> Every bucket fits in exactly **one 64-byte CPU cache line** with embedded LFU+LRU eviction metadata. Eviction decisions cost **zero additional cache misses**.

## Why PulseMap?

| Problem | Existing Solutions | PulseMap |
|---------|-------------------|----------|
| Hash table + cache = 2 data structures | HashMap + LRU list (extra pointer chasing) | **Single structure, built-in eviction** |
| Eviction requires extra memory fetches | W-TinyLFU needs 5-7 cache line touches | **Zero extra fetches** |
| Unbounded memory growth | HashMap grows forever, must resize | **Fixed memory budget** |
| Metadata in separate cache line | Swiss Table: control bytes ≠ slot array | **Metadata + slots in same 64B** |

## Quick Start

```rust
use pulse_map::PulseMap;

let mut map = PulseMap::new(1024); // 1024 buckets × 4 slots = 4096 capacity

map.insert(b"hello", b"world");
assert_eq!(map.get(b"hello"), Some(&b"world"[..]));

map.remove(b"hello");
assert_eq!(map.get(b"hello"), None);
```

## Benchmark Results (v0.1.0)

**PulseMap vs std::HashMap** (which uses Swiss Table/hashbrown internally)

| Benchmark (100K ops) | PulseMap | std::HashMap | Speedup |
|---------------------|:-------:|:------------:|:-------:|
| **INSERT** | **22.7 ms** | 78.0 ms | **3.4x faster** |
| **LOOKUP** | 23.4 ms | 17.1 ms | std 1.4x faster |
| **MIXED (insert+lookup)** | **37.3 ms** | 91.8 ms | **2.5x faster** |
| **EVICTION (50K→1K)** | **2.5 ms** | impossible | ∞ |

### `perf stat` (Zig prototype, same algorithm)

| Counter | PulseMap | Swiss Table | Reduction |
|---------|:-------:|:-----------:|:---------:|
| cache-misses | **1.8M** | 3.5M | **47% fewer** |
| IPC | **1.03** | 0.65 | **58% better** |

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

## Project Structure

```
pulse_map/
├── Cargo.toml
├── README.md
├── CHANGELOG.md
├── src/
│   ├── lib.rs              # Public API (PulseMap struct)
│   └── core/               # Core engine
│       ├── mod.rs           # Module re-exports
│       ├── meta.rs          # MetaWord (64-bit packed metadata)
│       ├── slot.rs          # Slot (14-byte inline/slab)
│       ├── bucket.rs        # Bucket (64B cache-line aligned)
│       ├── slab.rs          # SlabPool (arena allocator)
│       └── hash.rs          # wyhash → H1/H2/ext_fp
└── benches/
    └── benchmark.rs         # Criterion benchmarks
```

## API

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `PulseMap::new(num_buckets: usize)` | Create with fixed capacity |
| `insert` | `&mut self, key: &[u8], value: &[u8]` | Insert or update (evicts on full) |
| `get` | `&self, key: &[u8] → Option<&[u8]>` | Lookup (updates priority) |
| `peek` | `&self, key: &[u8] → Option<&[u8]>` | Lookup (no priority update) |
| `remove` | `&mut self, key: &[u8] → bool` | Delete a key |
| `len` | `&self → usize` | Entry count |
| `capacity` | `&self → usize` | Total slots |
| `load_factor` | `&self → f64` | Current load |
| `eviction_count` | `&self → usize` | Total evictions |

## Use Cases

- **L4 / Software-defined CPU cache tiers**
- **Network routers/switches** — per-packet latency critical
- **Database buffer pools** — bounded cache with constant eviction
- **Embedded systems** — no dynamic allocation, deterministic latency
- **CDN edge caches** — hot content stays, cold evicted
- **Game engines** — asset caching with fixed VRAM budget

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

---
## License

Licensed under either of:

- **Apache License, Version 2.0** ? [LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>
- **MIT License** ? [LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>

at your option.


