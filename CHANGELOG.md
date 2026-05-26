# Changelog

All notable changes to PulseMap will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [v0.1.0] — 2026-05-22

### 🎉 Initial Release — Core Engine

The foundation of PulseMap: a 64-byte cache-line hash table with built-in eviction.

### Added

**Core Engine (`src/core/`)**
- `MetaWord` — 64-bit packed metadata storing state (2b), H2 fingerprint (7b), and priority (7b) for 4 slots
- `Slot` — 14-byte entry with two modes:
  - Inline mode: keys ≤6 bytes + values ≤7 bytes stored directly in cache line
  - Slab mode: 46-bit fingerprint + pointer to heap-allocated entry
- `Bucket` — 64-byte `#[repr(C, align(64))]` struct = exactly 1 CPU cache line (compile-time verified)
- `SlabPool` — Arena-based allocator for variable-length key+value entries
- `hash` — wyhash splitting into H1 (bucket index), H2 (7-bit fingerprint), ext_fp (46-bit slab fingerprint)

**PulseMap API (`src/lib.rs`)**
- `PulseMap::new(num_buckets)` — fixed-capacity construction
- `insert(&mut self, key, value)` — insert with automatic eviction on full buckets
- `get(&self, key)` — immutable lookup with interior priority update
- `peek(&self, key)` — lookup without priority update
- `remove(&mut self, key)` — key deletion
- `len()`, `capacity()`, `load_factor()`, `eviction_count()` — stats

**Eviction Policy**
- Hybrid LFU+LRU: 4-bit frequency + 3-bit recency = 7-bit priority per slot
- `on_access()`: boost frequency, set recency to max, decay other slots
- `on_insert()`: cold start (freq=0, recency=1)
- `find_evict_target()`: lowest priority slot evicted
- **Zero extra cache misses** — all priority data in MetaWord (already fetched)

**Optimizations**
- `match_mask()` — bitmask-based H2 scan (compiler-friendly unrolled)
- `get(&self)` not `get(&mut self)` — allows shared references
- `Send + Sync` implemented for thread-safe reads

**Testing**
- 16 tests passing (15 unit + 1 doc test)
- Bucket size compile-time assertion (must be 64 bytes)

**Benchmarks (vs std::HashMap / Swiss Table)**
- INSERT: 3.4x faster (22.7ms vs 78.0ms for 100K ops)
- MIXED: 2.5x faster (37.3ms vs 91.8ms)
- EVICTION: 2.5ms for 50K ops (std::HashMap: impossible)
- Cache misses: 47% fewer (perf stat verified)

### Known Limitations
- `&[u8]` keys only (no generic types yet)
- No iterator support
- No dynamic resizing
- Lookup 1.4x slower than std::HashMap (no SIMD yet)
- Single-threaded only (Send+Sync but no internal locking)

---

## [v0.2.0] — 2026-05-22

### 🚀 Generic Types + Iterator + Traits

Layered architecture: `core/` → `raw.rs` → `lib.rs`. Users get typed API, power users get raw bytes.

### Added

**Architecture Refactor**
- `raw.rs` — `PulseMapRaw` (v0.1.0 PulseMap renamed) — raw `&[u8]` engine
- `PulseMap` is now a type alias for `PulseMapRaw` (backward compatible)
- `TypedPulseMap<K, V>` — generic wrapper over PulseMapRaw

**Traits (`PulseKey` / `PulseValue`)**
- `PulseKey` trait with `to_bytes()` + `from_bytes()` for key serialization
- `PulseValue` trait with `to_bytes()` + `from_bytes()` for value serialization
- Built-in impls: `u8`, `u16`, `u32`, `u64`, `i32`, `i64`, `String`, `Vec<u8>`, `[u8; N]`, `bool`

**TypedPulseMap<K, V> API**
- `insert(K, V)`, `get(&K)→Option<V>`, `peek(&K)→Option<V>`
- `remove(&K)→bool`, `contains_key(&K)→bool`
- `iter()→TypedIter<K,V>` — typed iteration over all entries

**Iterator Support (`src/iter.rs`)**
- `RawIter` — iterates `(&[u8], &[u8])` raw pairs
- `TypedIter<K, V>` — iterates `(K, V)` with auto-deserialization

**Std Traits**
- `Debug` — shows len, capacity, load%, evictions
- `Display` — human-readable `PulseMap(n/cap entries, x% load, y evictions)`
- `Extend<(K, V)>` — bulk insertion from any iterator
- `From<HashMap<K, V>>` — convert std::HashMap to TypedPulseMap (auto-calculates bucket count)

**Zero-Alloc Serialization**
- `PulseKey`/`PulseValue` traits now use associated type `Bytes`
- Numeric types (`u32`, `u64`, etc.) return `[u8; N]` on stack — **zero heap allocation**
- `String`/`Vec<u8>` still use `Vec<u8>` (unavoidable)

### Design Decision: `Index<&K>` NOT Implemented

`map[&key]` syntax requires returning `&V` (a reference to the value). PulseMap stores values
as raw bytes and deserializes them on read — it returns `V` (an owned copy), not `&V`.

Implementing `Index` would require either:
1. Panicking (unsafe, bad UX) — rejected
2. Caching deserialized values (extra memory, defeats purpose) — rejected
3. Leaking memory (unsafe) — rejected

**Use `map.get(&key)` instead.** Returns `Option<V>`.

**Testing**
- 29 tests passing (25 unit + 4 doc tests)

### Benchmarks (v0.2.0) — Fair Comparison

**PulseMap vs `lru` crate (SAME CATEGORY — bounded cache with eviction)**

| Benchmark (100K) | PulseMap Typed | `lru` crate | PulseMap wins? |
|-------------------|:------------:|:-----------:|:--------------:|
| **INSERT** | **36.3 ms** | 79.3 ms | ✅ **2.2x faster** |
| **MIXED** | **63.0 ms** | 87.6 ms | ✅ **1.4x faster** |
| **EVICTION (50K)** | **4.6 ms** | 4.8 ms | ✅ **~same** |
| LOOKUP | 34.2 ms | **15.1 ms** | ❌ lru 2.3x faster |

**PulseMap vs std::HashMap (DIFFERENT CATEGORY — reference only)**

| Benchmark (100K) | PulseMap Typed | std::HashMap | Note |
|-------------------|:------------:|:------------:|:----:|
| INSERT | 36.3 ms | 7.4 ms | std has no eviction |
| LOOKUP | 34.2 ms | 10.8 ms | std uses SIMD |
| MIXED | 63.0 ms | 19.5 ms | std uses native types |

---

## [v0.3.0] — 2026-05-26

### ⚡ Performance + SIMD + Entry API + no_std

**2x overall speedup.** Power-of-2 buckets, branchless H2 matching, SIMD support, and prefetch hints.

### Added

**Power-of-2 Bucket Count (`raw.rs`)**
- `num_buckets` auto-rounded to next power of 2
- `% num_buckets` → `& bucket_mask` — modulo replaced with bitwise AND
- Applied across all 4 hot paths: `insert()`, `get()`, `peek()`, `remove()`

**SIMD H2 Matching (`simd.rs`)**
- SSE2 `_mm_cmpeq_epi8` + `_mm_movemask_epi8` for parallel H2 comparison
- Behind `--features simd` flag (x86_64 only)
- Default scalar path uses branchless bit arithmetic (`meta.rs`)

**Prefetch Hints (`raw.rs`)**
- `_mm_prefetch` in `get()` — preloads bucket into L1 cache before access

**Entry API (`lib.rs`)**
- `map.entry(key).or_insert(value)` — insert if vacant
- `map.entry(key).or_insert_with(|| compute())` — lazy insert
- `map.entry(key).and_modify(|v| *v += 1).or_insert(0)` — modify-or-insert
- `OccupiedEntry`: `get()`, `key()`, `insert()`, `remove()`
- `VacantEntry`: `key()`, `insert()`

**`#![no_std]` Support**
- `default = ["std"]` — backward compatible
- `default-features = false` enables `no_std` with `alloc`
- `From<HashMap>` gated behind `#[cfg(feature = "std")]`

**Testing**
- 35 tests passing (30 unit + 5 doc tests)

### Benchmarks (v0.3.0)

**v0.2.0 → v0.3.0 Speedup**

| Benchmark (100K) | v0.2.0 | v0.3.0 | Speedup |
|---|:---:|:---:|:---:|
| INSERT | 36 ms | **15 ms** | **2.4x faster** |
| LOOKUP | 34 ms | **18 ms** | **1.9x faster** |
| MIXED | 63 ms | **32 ms** | **2.0x faster** |
| EVICTION | 4.6 ms | **1.8 ms** | **2.6x faster** |

**PulseMap vs `lru` crate (same category)**

| Benchmark (100K) | PulseMap | `lru` | Result |
|---|:---:|:---:|:---:|
| **INSERT** | **15 ms** | 32 ms | ✅ **2.1x faster** |
| **MIXED** | **32 ms** | 44 ms | ✅ **1.4x faster** |
| **EVICTION** | **1.8 ms** | 3.2 ms | ✅ **1.8x faster** |
| LOOKUP | 18 ms | **8.3 ms** | ❌ lru 2.2x faster |

---

## [v0.4.0] — 2026-05-26

### 🔒 Thread Safety + Dynamic Resize

**ConcurrentPulseMap** — thread-safe with per-bucket spinlocks. Only 7% overhead vs single-threaded.

### Added

**ConcurrentPulseMap (`sync.rs`)**
- `ConcurrentPulseMap::<K, V>::new(n)` — fixed-size concurrent map
- `ConcurrentPulseMap::with_auto_resize(n)` — auto-grows at 75% load
- All methods take `&self` (not `&mut self`) — safe via `Arc`
- `insert()`, `get()`, `peek()`, `remove()`, `contains_key()`
- `len()`, `capacity()`, `load_factor()`, `eviction_count()`, `num_buckets()`
- `Debug` and `Display` trait implementations

**Per-Bucket Spinlock Architecture**
- `BucketLocks` — `Vec<AtomicU8>` (1 lock per bucket)
- `BucketGuard` — RAII guard (auto-unlock on drop)
- `compare_exchange_weak` + `spin_loop()` for low-latency locking
- Different buckets accessed fully in parallel

**Dynamic Resize**
- `map.resize(new_size)` — manual stop-the-world rehash
- `with_auto_resize(n)` — auto-doubles at 75% load factor
- `RwLock<MapInner>` — read lock for ops, write lock for resize

**Slot Helpers (`slot.rs`)**
- `get_key_bytes()` — extract key (inline or slab) for rehashing
- `get_value_bytes()` — extract value (inline or slab) for rehashing

**Testing**
- 46 tests passing (38 unit + 8 doc tests)
- Multi-threaded insert test (4 threads × 1000 entries)
- Concurrent read/write test
- Manual resize + auto-resize tests

### Benchmarks (v0.4.0)

**Concurrency Overhead**

| Benchmark (100K) | TypedPulseMap | ConcurrentPulseMap (1T) | Overhead |
|---|:---:|:---:|:---:|
| INSERT | 13.8 ms | 14.8 ms | **7%** |

**4-Thread Concurrent**

| Benchmark (100K) | ConcurrentPulseMap |
|---|:---:|
| 4T INSERT | **20.8 ms** |
| 4T LOOKUP | **15.2 ms** |
| 4T MIXED | **35.6 ms** |

**PulseMap vs `lru` crate (final score)**

| Benchmark (100K) | PulseMap | `lru` | Result |
|---|:---:|:---:|:---:|
| **INSERT** | **13.8 ms** | 19.1 ms | ✅ **1.4x faster** |
| **MIXED** | **17.9 ms** | 23.7 ms | ✅ **1.3x faster** |
| **EVICTION** | **1.5 ms** | 2.2 ms | ✅ **1.5x faster** |
| LOOKUP | 9.8 ms | **5.4 ms** | ❌ lru 1.8x faster |

---

## [v0.5.0] — Planned

### 🌐 FFI Bindings

- [ ] C FFI via `cbindgen`
- [ ] Python via PyO3
- [ ] Java via JNI
- [ ] Node.js via napi-rs
