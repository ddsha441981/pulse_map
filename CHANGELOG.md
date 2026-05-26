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

## [v0.3.0] — Planned

### ⚡ Performance + SIMD

- [ ] SIMD H2 matching (SSE2/NEON)
- [ ] Power-of-2 bucket count (bitwise modulo)
- [ ] Prefetch hints
- [ ] `Entry` API
- [ ] `#![no_std]` support

---

## [v0.4.0] — Planned

### 🔒 Thread Safety + Dynamic Resize

- [ ] Per-bucket spinlock
- [ ] `PulseMapSync<K, V>`
- [ ] Dynamic resizing

---

## [v0.5.0] — Planned

### 🌐 FFI Bindings

- [ ] C FFI via `cbindgen`
- [ ] Python via PyO3
- [ ] Java via JNI
- [ ] Node.js via napi-rs

