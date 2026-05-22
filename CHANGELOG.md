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

## [v0.2.0] — Planned

### 🚀 Generic Types + Iterator + Performance

**Planned Features**
- [ ] Generic key/value types: `PulseMap<K: Hash + Eq, V>`
- [ ] Iterator support: `.iter()`, `.keys()`, `.values()`
- [ ] `Entry` API: `map.entry(key).or_insert(value)`
- [ ] `Debug` and `Display` trait implementations
- [ ] `From`/`Into` conversions from std::HashMap
- [ ] `Extend` trait for bulk insertion

**Performance**
- [ ] SIMD H2 matching (SSE2/NEON) — target: match std::HashMap lookup speed
- [ ] Prefetch hints (`_mm_prefetch`) for next bucket
- [ ] Power-of-2 bucket count for bitwise modulo (avoid expensive `%`)

**Quality**
- [ ] `#![no_std]` support (optional)
- [ ] Miri safety verification
- [ ] Fuzzing with `cargo-fuzz`
- [ ] Property-based testing with `proptest`

---

## [v0.3.0] — Planned

### 🔒 Thread Safety + Dynamic Resize

- [ ] Per-bucket spinlock for concurrent access
- [ ] `PulseMapSync<K, V>` wrapper with `RwLock` per bucket
- [ ] Dynamic resizing (grow/shrink with hysteresis)
- [ ] Slab memory pool with configurable arena size

---

## [v0.4.0] — Planned

### 🌐 FFI Bindings

- [ ] C FFI via `cbindgen` (`libpulsemap.h`)
- [ ] Python binding via PyO3 (`pip install pulsemap`)
- [ ] Java binding via JNI
- [ ] Node.js binding via napi-rs
