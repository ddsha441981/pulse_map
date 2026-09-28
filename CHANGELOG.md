# Changelog

All notable changes to PulseMap will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [0.6.5] — 2026-09-28

### 🔬 Validation & Hardening

A validation and hardening release, not a feature release: it exists to prove the correctness of the `unsafe` and lock-free code that is already shipping. Outside a single behavioural fix — the `AccessBuffer` drain (below), which makes `ConcurrentPulseMap` reads feed the eviction policy — `src/` carries no logic changes, only test attributes and cfg-gated atomic imports.

**cargo-fuzz harness (`fuzz/`) — issue #7, PR #13 (@saiteja00743)**
- New `fuzz_sequences` target driving random `insert` / `get` / `remove` sequences with TTL and eviction pressure, re-reading the last insert to catch silent corruption
- `fuzz/` is its own workspace so it no longer inherits the root `lto = true`, which broke the sanitizer link step with undefined `__sancov_gen_` symbols
- Measured: **4,282,559 executions clean under AddressSanitizer** (181 s)
- The packaged crate is unaffected — `cargo package --list` contains zero fuzz files
- Verified only the most recent insert; eviction correctness for every earlier entry followed in issue #15 below

**24-hour soak example (`examples/soak_test.rs`) — issue #8, PR #14 (@VedantMadane)**
- 8 writer + 4 reader threads against `ShardedPulseMap`, with RSS leak detection via `/proc/self/status` and sentinel-key integrity checks
- Measured: **147.9M ops in 10 s at +0.0 MB RSS drift**; a 190 s run reached 1.82B ops with monotonically increasing eviction counts
- Caveat: at the documented parameters the map saturates to 100% load within a second, so the run exercises eviction churn rather than TTL expiry. `ConcurrentPulseMap` picks slots with `find_free_slot()` (`src/sync.rs:345`), not the `find_free_or_expired()` that `PulseMapRaw` uses (`src/raw.rs:178`), so there is no insert-time reclaim of expired slots to exercise

**Miri in CI — issue #11, PR #16**
- New `miri` job running two configurations: `cargo miri test -p pulse_map --no-default-features` (engine, `PulseMapRaw`, `TypedPulseMap`) and `cargo miri test -p pulse_map --lib`, which adds `ConcurrentPulseMap` and `ShardedPulseMap`
- **Both clean: no undefined behaviour and no leak reports.** First validation of `SlabPool`'s manual `alloc` / `realloc` / `dealloc` pairing and the `repr(C, packed)` `Slot` accesses against Stacked Borrows and provenance rules — AddressSanitizer only sees machine-level errors, never aliasing violations
- Three oversized tests carry `#[cfg_attr(miri, ignore)]` (16 shards x 16384 buckets is 16 MB of tracked allocation: measured at 2.2 GB RSS for 17 minutes without finishing). `cargo test` is unaffected — 58 passed, 0 ignored, because `cfg(miri)` never fires on a normal build
- `fuzz/rust-toolchain.toml`: restored the `components` line that 851b7dc dropped along with the nightly pin; `cargo fuzz coverage` needs `llvm-tools-preview`

**Shadow-map eviction correctness in the fuzz harness — issue #15, PR #18**
- Every operation is now mirrored into a `HashMap<Vec<u8>, Vec<u8>>`, and any `get` / `peek` that returns a value must match what the shadow map recorded. Absence stays legal — which of the 4 slots in a bucket loses is not observable from outside, and TTL expiry is a second source of legitimate absence — but a wrong value never is, and neither is a hit for a key that was removed
- This is what closes the one acceptance criterion #7 could not: the harness from PR #13 tracked only the most recent insert in fixed stack arrays, so every earlier entry went unverified once a bucket overflowed and eviction began replacing slots
- A fingerprint collision cannot make the assertion fire spuriously: `matches_key` (`src/engine/slot.rs:138`) compares the full key in both inline and slab mode, using the 46-bit extended fingerprint only as a pre-filter
- Measured: **5,455,722 executions clean under AddressSanitizer** (1,202 s, 16 buckets so eviction pressure is constant). Throughput is 6.5K exec/s from an empty corpus against the 23.6K/s recorded in #13, the cost of one `Vec` allocation per mirrored insert
- The harness is 54 lines shorter than before, and `cargo clippy` on `fuzz/` is now warning-free

**loom models for the lock-free paths — issue #9, PR #19**
- New `Loom` CI job and two models covering the only two places in the engine that mutate shared state without holding a lock: `MetaWord::on_access` (`tests/loom_meta.rs`) and `AccessBuffer` push/drain (`tests/loom_access_buffer.rs`). `cargo test` exercises these with real threads, which samples one interleaving per run; loom enumerates them
- `on_access` is held to its exact contract: two threads touching distinct slots must each land their freq increment, and the recency pair can only end as (7,6) or (6,7) — the two serializations and nothing in between; two threads on the same slot must count both hits, and the untouched slot must decay exactly twice. Verified load-bearing: replacing the CAS with a plain store fails both tests
- `AccessBuffer` is held to its lossy contract — dropped events are acceptable, wrong ones are not: no fabricated event, none delivered twice, and the `EMPTY` sentinel never handed out as data
- Measured, real wall clock: **0.44 s for the 2 `MetaWord` models, 9.10 s for the 2 buffer models**, against the 5-minute ceiling in the issue
- Known limit, documented in the test file: the buffer model does not cover the store order inside `push`. Releasing the new head before writing the payload is a real bug that loses the event, and both tests still pass on it — loom explored 6 executions at every preemption bound tried (1, 2, 3, 5 and the default unbounded) and never scheduled the drain between the two stores. Removing the payload store outright does fail both tests, so the assertions themselves are live
- `--cfg loom` swaps the atomics in `meta.rs` and `access_buffer.rs` for loom's instrumented ones, and `MetaWord::empty()` loses `const` in that configuration only. **Normal builds are untouched**: loom is a `[target.'cfg(loom)'.dependencies]` entry, so it is resolved into `Cargo.lock` but never compiled — a plain `cargo build -v` passes no `--extern loom`. Both test files are `#![cfg(loom)]`, keeping the 58 unit tests away from loom atomics they would panic on

**Embedded target matrix in CI — PR #20**
- The `no_std` job was a single `cargo check` against `thumbv7m-none-eabi`. It is now 8 jobs, one per atomic capability class, which is the only axis that can break a bare-metal build here: `MetaWord` is an `AtomicU64`, and `portable-atomic` can only hand one out where the target gives it some form of atomic CAS to build on
- **Two targets did not compile before this change.** `thumbv6m-none-eabi` (Cortex-M0 / M0+ — both of the RP2040's cores) and `riscv32imc-unknown-none-elf` (ESP32-C3) both fail identically with `error[E0432]: unresolved import portable_atomic::AtomicU64`. ARMv6-M has no `LDREX`/`STREX` and RISC-V without the A extension has no atomic instructions at all, so portable-atomic's `fallback` spinlock has no CAS to build *itself* out of and does not define `AtomicU64` — the crate's `no_std` claim was real but narrower than advertised
- New opt-in **`critical-section`** feature forwarding to `portable-atomic/critical-section`, which performs the CAS with interrupts masked. With it, both targets check clean. Off by default deliberately: it is only sound on single-core targets, and the impl belongs to the binary rather than the library — a Cortex-M0 user pulls it from `cortex-m`'s `critical-section-single-core`, an ESP32-C3 user from `esp-hal`
- Newly covered and clean with no extra feature: `thumbv7em-none-eabihf` (Cortex-M4F / M7F), `thumbv8m.main-none-eabi` (Cortex-M33), `riscv32imac-unknown-none-elf`, `aarch64-unknown-none`, `wasm32-unknown-unknown`
- `fail-fast: false`, so one unsupported target can't mask the state of the other seven
- Scope, stated plainly: these are compile checks. Nothing was executed on real silicon or under QEMU, and `cargo check` does not link — a downstream binary on thumbv6m or riscv32imc still has to supply the `critical-section` impl or it fails at link time


**Runs on emulated Cortex-M hardware — PR #21**
- PR #20 proved the crate *compiles* for 8 bare-metal targets. `cargo check` does not link and never executes an instruction, so the `critical-section` path on Cortex-M0 was still an untested claim. New `qemu-test/` crate closes that: a real `cortex-m-rt` binary, linked with a panic handler and a `memory.x`, booted under `qemu-system-arm` in CI
- **Cortex-M0 is the case that matters.** ARMv6-M has no CAS instruction, so `MetaWord`'s `AtomicU64` can only work through `critical-section` — and `MetaWord::on_access` runs that CAS on every `get` hit. `-machine microbit` (nRF51822) is the only emulated ARM machine that is actually `thumbv6m`, so it is the only one that exercises the path. `-cpu cortex-m3` (`thumbv7m`, portable-atomic's spinlock fallback) runs alongside it as the control
- 12 assertions: capacity, empty state, hit/miss, insert past 4× capacity to force eviction, then `peek` every key checking that no key ever reads back a value it was not stored with (absence is legal — which of a bucket's 4 slots loses is not observable — a wrong value never is), `eviction_count() > 0`, TTL live-then-expired by insertion count, and `remove`
- Failures are real failures: the checks report through semihosting and exit `EXIT_FAILURE`, which propagates as a non-zero process exit to `cargo run` and fails the job. Verified by deliberately breaking one assertion and confirming CI-visible exit 1
- **Measured: a map costs 128 bytes per bucket**, allocated upfront regardless of occupancy — 64 B for the cache-line `Bucket` plus 4 × 16 B of `SlotTTL` for its slots. Found the hard way: a 64-bucket map exhausted an 8 KiB heap. The test now runs 16 buckets / 2 KiB and prints its own heap usage (3072 B for three maps) into the CI log. The README's 40.0 B/entry figure is consistent with this, but bucket count, not entry count, is the number to budget with on a 16 KiB part
- Still not covered, deliberately: `riscv32imc` (ESP32-C3). `qemu-system-riscv32 -machine virt` has the A extension, so emulating it would test a target that does not need the feature. And nothing has run on physical silicon
- `qemu-test/` is its own workspace with its own `.cargo/config.toml` runner, the same isolation `fuzz/` uses, so it never affects a host build of `pulse_map`

**AccessBuffer drain**
- `ConcurrentPulseMap::get()` buffers its LRU/LFU priority updates in the `AccessBuffer` (introduced v0.6.2), but nothing ever drained the buffer — so reads never fed the eviction policy. Its hit rate sat 0.92 points under `TypedPulseMap`'s: **94.456% vs 95.372%** in the `hitrate_16384` benchmark
- `insert()` now drains the buffer under the target bucket's lock, so read latency is untouched: the drain runs on the write path only
- Ships with a `BucketGuard` fix so the drain is Miri-clean
- Result: `ConcurrentPulseMap::get()` ties `TypedPulseMap` at **95.372%**

**Embedded footprint evidence**
- Answers the two objections an embedded reviewer would actually raise, with measurements instead of prose. Both run in the existing QEMU CI job; no new infrastructure
- **`alloc` is needed only at construction, and that is now machine-checked.** The QEMU test's bump allocator counts its calls: `TypedPulseMap<u32, u32>` makes **2 allocations at `new()` and 0 across 256 `insert`+`get`+`remove`**. The two are `buckets` and `slots_ttl`, so a fixed `buckets × 128`-byte arena is sufficient for the map's whole lifetime. This is the difference between "needs a heap" (a dealbreaker in firmware that deliberately has none) and "needs a static arena at init" (a line in `main`)
- The counterexample is reported too, not hidden: `TypedPulseMap<u64, u64>` exceeds the 6-byte key / 7-byte value inline window, so entries reach the slab and allocate — 14 allocations across 6 inserts. A static arena is not enough for that path
- **Flash and RAM measured against `lru`** on the emulated Cortex-M0, same workload, same `lto = true` profile, both holding 64 resident entries: PulseMap 2,048 B of RAM / 2 allocations, `lru` 4,520 B / 68. **55% less RAM at the same entry count** (32 vs 70.6 B/entry). Flash is a wash — 7,320 B vs 7,176 B over baseline, 2.0% apart
- **Reported against interest:** on Cortex-M3 PulseMap costs 9,792 B of flash to `lru`'s 7,576 B. portable-atomic's spinlock `AtomicU64` fallback is fatter than the critical-section route M0 takes, so where flash is the binding constraint on an M3-class part, `lru` is the smaller choice. `README.md` says so
- Flash figures are quoted from CI (stable rustc 1.98.1) rather than a local build: exact byte counts drift a few dozen bytes per toolchain release, so the reproducible number is the one anyone can read off the workflow. RAM and allocation counts are toolchain-independent
- Three probe binaries under `qemu-test/src/bin/` (`footprint_none`, `footprint_pulse`, `footprint_lru`) plus `qemu-test/size.sh`. The no-cache baseline exists so the table measures the caches rather than `hprintln!` and the panic handler — it is 2,496 B of the total on thumbv6m. Host binutils `size` reads ARM ELF, so no `arm-none-eabi` toolchain is required
- `qemu-test` release profile gained `lto = true` / `codegen-units = 1`, matching the parent crate and what real firmware ships. Without LTO the comparison measured un-inlined cross-crate glue nobody flashes, and it moved the numbers by thousands of bytes
- Bump allocator extracted to `qemu-test/src/bump.rs` and shared by all four binaries
- README's **Known Limitations** list now carries these as first-class entries rather than leaving them in prose: no MCU perf figure exists, `lru` wins on flash on M3-class parts, and every `get` hit CASes the `MetaWord` even single-threaded — which on M0 means reads disable interrupts
- **Still not measured, and now stated in the README:** no throughput or latency figure on any MCU. QEMU is not cycle-accurate — no pipeline model, no flash wait states — so timing it would produce a number worth less than no number. Every performance figure in the docs is x86_64. Closing that needs real silicon; an RP2040 is the honest choice, being the Cortex-M0+ part that needs `critical-section`

---

## [v0.6.4] — 2026-08-19

### 🌍 Portable AtomicU64 — Cross-Platform Compatibility

- **Replaced `core::sync::atomic::AtomicU64` / `std::sync::atomic::AtomicU64` with `portable-atomic::AtomicU64`** across `src/engine/meta.rs` and `src/sync.rs`
- Enables the crate to compile and run on targets without native 64-bit atomics: **WASM32**, **ARMv7-M** (thumbv7m), and other **32-bit embedded platforms**
- New dependency: `portable-atomic = { version = "1.6", default-features = false, features = ["fallback"] }`
- **No API changes** — drop-in replacement, zero user-facing impact
- CI already validates `thumbv7m-none-eabi` target; portable-atomic provides the fallback implementation where the CPU lacks `LDAXR`/`STLXR` 64-bit instructions

---

## [v0.6.3] — 2026-08-17

### 📚 Documentation & Crates.io Links Fix

- **Updated Crates.io Documentation URL:** Pointed `documentation` field in `Cargo.toml` directly to the hosted mdBook documentation site (`https://ddsha441981.github.io/pulse_map/`).
- **Added Docs.rs Metadata:** Added `[package.metadata.docs.rs]` configuration in `Cargo.toml` with `all-features = true` and `rustdoc-args = ["--cfg", "docsrs"]` so docs.rs builds with complete feature flags.

---

## [v0.6.2] — 2026-08-11

### 🚀 Lock-Free Reads + Data Race Fixes + Latency Reductions

Major stability and performance release: fixed UB, eliminated lock contention on reads, and reduced GET latency by over 60%.

### Added

**Atomic MetaWord + Access Buffer (`src/engine/*`, `src/raw.rs`, `src/sync.rs`) — PR-8**
- `MetaWord(u64)` → `MetaWord(AtomicU64)` — all reads use `Relaxed` atomic loads
- `on_access()` uses CAS loop instead of exclusive mutation
- NEW: `AccessBuffer` — lock-free lossy ring buffer for deferred eviction tracking
- `get()` pushes access events to buffer instead of mutating MetaWord inline
- Removed unsafe raw pointer cast from `get()` in `raw.rs`
- `Bucket` no longer derives `Copy` (AtomicU64 is !Copy)
- Result: **66% improvement in GET p99 latency vs v0.6.1 baseline**

### Changed

**Upgrade TTL Epoch Types u32 → u64 (`src/raw.rs`, `src/sync.rs`, `src/sharded.rs`, `src/lib.rs`) — PR-4**
- `current_epoch`: `AtomicU32` → `AtomicU64`
- `default_ttl`: `AtomicU32` → `AtomicU64`
- `SlotTTL.epoch`: `u32` → `u64`
- All public TTL API signatures updated: `set_ttl(u64)`, `get_ttl() -> u64`, `current_epoch() -> u64`, `insert_ttl(..., ttl: u64)`
- Eliminates epoch wrap-around after 4.29B inserts
- **BREAKING CHANGE**: TTL parameter types changed from `u32` to `u64`

**Lazy Slab Lock in `get()` (`src/sync.rs`) — PR-7**
- Inline keys (mode=0, key ≤ 6 bytes) now skip the `slab_pool.lock()` mutex entirely during reads
- Slab-mode keys check 46-bit fingerprint BEFORE acquiring the lock
- Result: **60% improvement in GET p99 latency**

### Fixed

**Fix UB & Data Race in `raw.rs` (`src/raw.rs`) — PR-1**
- Removed `unsafe impl Sync for PulseMapRaw` — `PulseMapRaw` is now `Send` but NOT `Sync`
- Users must use `ConcurrentPulseMap` or `ShardedPulseMap` for multi-threaded access

**Fix Data Loss & TTL Wipe During `resize` (`src/sync.rs`) — PR-2**
- Fixed silent data loss when bucket overflows during rehash (added overflow retry loop that doubles capacity)
- Fixed TTL wipe: epochs/TTL metadata is now properly migrated during resize

**Fix Fingerprint Entropy Collapse in `ShardedPulseMap` (`src/sharded.rs`) — PR-3**
- Shard routing changed from `h1 >> 60` (bits 60-63) to `h1 as usize & mask` (low bits)
- This eliminated overlap with h2 fingerprint bits (57-63), restoring full 7-bit (128 values) fingerprint entropy within each shard

**SIMD Dispatch Fix (`src/engine/meta.rs`) — PR-5, PR-6**
- PR #5 removed SIMD dispatch based on agent analysis (WRONG — caused 20% throughput regression)
- PR #6 immediately restored SIMD dispatch — benchmarks proved SSE2 path IS faster in release builds
- Lesson learned: always benchmark before removing optimizations

### Benchmarks (v0.6.1 → v0.6.2)

| Metric | v0.6.1 | v0.6.2 | Change |
|--------|--------|--------|--------|
| GET p99 (Mixed Workload) | 1.244 µs | 964 ns | 22.5% faster |
| Throughput (5M inserts) | 5.99M ops/s | 7.47M ops/s | 24.6% faster |
| Contention p99 (Hot Keys) | 1.277 µs | 1.134 µs | 11.2% faster |
| Memory per entry | 34.0 B | 34.0 B | Zero overhead |

### Testing

- 58 unit tests + 11 doc-tests passing
- All `cargo clippy`, `cargo fmt --check`, `cargo test` passed for every PR

---

## [v0.6.1] — 2026-08-03

### 🚀 Sharded Concurrency + Per-Entry TTL + Real Competitor Benchmarks

Major release: 16-shard concurrent map (2.4-3.1x faster), per-entry TTL, and honest benchmarks against moka + quick_cache.

### Added

**ShardedPulseMap (`src/sharded.rs`) — PR-3**
- `ShardedPulseMap<K,V>` — 16 independent `ConcurrentPulseMap` shards
- Shard selection: `h1 >> 60` (top 4 bits, independent from bucket selection)
- `insert()`, `get()`, `peek()`, `remove()`, `contains_key()` — routed to shard by hash
- `resize_all(n)` — per-shard rehash, no stop-the-world pause
- TTL propagation: `set_ttl()` applied to all shards, `current_epoch()` = max
- `len()`, `capacity()`, `load_factor()`, `eviction_count()` — aggregated stats

**Per-Entry TTL (`raw.rs`, `lib.rs`, `sync.rs`, `sharded.rs`) — PR-4**
- `insert_ttl(key, value, ttl)` on all map types (PulseMap, TypedPulseMap, ConcurrentPulseMap, ShardedPulseMap)
- `ttl = 0`: use global default (`set_ttl()`), `u32::MAX`: never expire, `N`: expire after N inserts
- `SlotTTL { epoch, ttl }` replaces `Vec<u32>` epochs (8 bytes/slot, was 4)
- Re-inserting refreshes both epoch and per-entry TTL
- Backward compatible: `set_ttl()`, `get_ttl()`, `insert()` behavior unchanged

**Zero-Copy Key Borrow (`lib.rs`, `sync.rs`) — PR-2**
- `PulseKey::key_bytes()` — borrow key bytes without allocation on read path
- Numeric types return stack-allocated `[u8; N]` via `with_key_bytes()`
- String lookup improved by -4.8%

**Real Competitor Benchmarks — PR-5**
- moka + quick_cache benchmarks (single-thread + 4-thread)
- Honest README benchmark table (losses documented alongside wins)

### Changed

- `raw.rs`: `epochs: Vec<u32>` → `slots_ttl: Vec<SlotTTL>`, `ttl_epochs` → `default_ttl`
- `raw.rs`: `insert()` refactored to `insert_internal(key, value, ttl)`
- `sync.rs`: epoch storage updated to `Vec<SlotTTL>`, `ttl_epochs` → `default_ttl`
- `is_expired()` now checks per-entry TTL with fallback to default
- `find_free_or_expired()` no longer requires global TTL to be set

### Benchmarks (v0.6.1)

**Single-Thread (100K ops)**

| Benchmark | PulseMap | `lru` | `quick_cache` | `moka` |
|-----------|:-------:|:-----:|:-------------:|:------:|
| INSERT | **6.1 ms** | 19.1 ms | 5.6 ms | 161 ms |
| LOOKUP | 5.4 ms | 5.4 ms | **2.8 ms** | 40 ms |
| EVICTION (50K) | **1.9 ms** 🥇 | 2.3 ms | 3.3 ms | 55.5 ms |

**Multi-Thread — 4 Threads, 100K ops**

| Benchmark | ShardedPulseMap | ConcurrentPulseMap | `moka` |
|-----------|:--------------:|:-----------------:|:------:|
| 4T INSERT | **8.8 ms** 🥇 | 20.2 ms | 104 ms |
| 4T LOOKUP | **9.0 ms** 🥇 | 35.0 ms | 21.1 ms |
| 4T MIXED | **15.9 ms** 🥇 | 46.6 ms | 197 ms |

### Testing

- **58 tests passing** (up from 57)
- 5 new ShardedPulseMap tests (basic, 4-thread, resize_all, TTL, len-sum)
- 6 new per-entry TTL tests (different expiries, never-expire, overrides-global, typed, concurrent, refresh)

### Rejected

- **PR-1 AHash**: A/B benchmark showed AHash 12.8% SLOWER than wyhash. wyhash retained.

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

## [v0.5.0] — 2026-05-26

### 🌐 FFI Bindings — Use PulseMap from Any Language

**Workspace architecture** — all bindings live in separate crates under one workspace.

### Added

**Workspace (`Cargo.toml`)**
- Rust workspace with 5 members: `pulse_map`, `pulse_map_ffi`, `pulse_map_py`, `pulse_map_java`, `pulse_map_node`

**Phase 1: C FFI (`pulse_map_ffi/`)**
- `libpulse_map_ffi.so` + `libpulse_map_ffi.a` (418K release)
- `include/pulse_map.h` — clean C header with opaque `PulseMapHandle*`
- 12 extern "C" functions: `new`, `new_auto_resize`, `free`, `insert`, `get`, `contains`, `remove`, `len`, `capacity`, `load_factor`, `eviction_count`, `resize`
- NULL-safe, buffer overflow protection (`-2` return code)
- 11 C tests passing

**Phase 2: Python (`pulse_map_py/`)**
- PyO3 bindings via `maturin`
- Dict-like API: `cache["key"] = "value"`, `cache["key"]`, `del cache["key"]`, `"key" in cache`
- Bytes API: `cache.insert(b"k", b"v")`, `cache.get(b"k")`
- Properties: `len()`, `capacity`, `load_factor`, `eviction_count`
- `repr()`: `PulseMap(len=1, capacity=256, load=0.4%)`
- 11 Python tests passing

**Phase 3: Java (`pulse_map_java/`)**
- Java 22+ Panama FFM API (no JNI!)
- Rust cdylib → `libpulse_map_java.so` → Java `Linker.downcallHandle()`
- `PulseMap` class: `put()`, `get()`, `remove()`, `size()`, `capacity()`
- `AutoCloseable` — `try (var cache = new PulseMap(1024)) { ... }`
- Unicode support (UTF-8 round-trip)
- 10 Java tests passing

**Phase 4: Node.js (`pulse_map_node/`)**
- napi-rs bindings → `pulse-map.node` (604K)
- String API: `cache.set()`, `cache.get()`, `cache.delete()`, `cache.has()`
- Bytes API: `cache.insertBytes()`, `cache.getBytes()`
- Getters: `size`, `capacity`, `loadFactor`, `evictionCount`
- 10 Node.js tests passing

### Testing

| Binding | Tests |
|---------|:-----:|
| C FFI | **11/11** |
| Python | **11/11** |
| Java | **10/10** |
| Node.js | **10/10** |
| **Total** | **42/42** |

---

## [v0.6.0] — 2026-06-15

### ⚡ Performance + Memory + TTL

Algorithmic fixes, memory correctness, and a new TTL feature.

### Added

**TTL via Epoch Counter (`raw.rs`)**
- `set_ttl(n: u32)` — entries expire after `n` insertions (0 = disabled)
- `get_ttl() → u32` — query current TTL setting
- `current_epoch() → u32` — total insertions so far
- Zero overhead when TTL is disabled (`ttl_epochs == 0` → single compare, skipped)
- Re-inserting a key refreshes its epoch (extends lifetime)
- Expired slots lazily reclaimed on next insert — no background thread needed
- Available on both `PulseMap` (raw) and `TypedPulseMap<K,V>`

```rust
let mut cache = PulseMap::new(1024);
cache.set_ttl(500);             // entries expire after 500 insertions
cache.insert(b"session", b"abc123");
// ...500 inserts later...
assert_eq!(cache.get(b"session"), None); // expired ✓
```

**Slab Free List (`engine/slab.rs`)**
- `SlabPool` now uses `Vec<Option<Box<SlabEntry>>>` + `free_list: Vec<usize>`
- Evicted slab entries returned to free list via `free(idx)` — reused on next alloc
- `SlabEntry::rewrite()` — in-place key/value rewrite (realloc only if new data is larger)
- **Fixes memory leak**: previously, evicted slab entries were abandoned until map dropped
- High-churn workloads (e.g., DNS cache, session store) now have stable memory

**Slot Layout Change (pointer → index)**
- Slab slots now store `usize` index into `SlabPool` (bytes 6–13)
- Previously stored raw `*const SlabEntry` pointer
- Enables free list: `raw.rs` calls `slab_pool.free(slot.slab_idx())` on eviction
- `slab_idx()` method replaces old `slab_ptr()`

### Changed

**`peek()` + `remove()` now use `match_mask()` (`raw.rs`, `sync.rs`)**
- Previously used brute-force per-slot loop: 8 individual `get_state()` + `get_h2()` calls
- Now identical to `get()`: single branchless `match_mask(h2)` bit operation
- Consistent hot path across all 3 lookup functions

**`SlotState::Deleted` removed (`lib.rs`)**
- Variant was never written — only `Tombstone` is set by `remove()`
- `Deleted = 2, Tombstone = 3` → `Tombstone = 2` (simpler encoding)
- `find_free_slot()` simplified from 3-way OR to single `!= Full` check
- `from_bits()` updated accordingly

### Fixed

- **Memory leak on eviction**: slab entries now returned to free list instead of abandoned
- **Slab memory on `remove()`**: `slab_pool.free(idx)` called on explicit key removal
- **Slab memory on update**: old slab entry freed before allocating new one

### Testing

- **57 tests passing** (up from 50)
- 4 new slab free list tests (reuse, larger rewrite, bulk reuse)
- 5 new TTL tests (basic expiry, update refresh, typed map, zero disables, epoch counter)

### Benchmarks (v0.6.0) — Actual Measured Results

> Run: `cargo bench -- lookup` on same machine. Numbers vary per run.

| Benchmark (100K ops) | v0.5.0 (est.) | v0.6.0 (measured) | Note |
|---|:---:|:---:|:---:|
| raw_lookup | ~7.2 ms | **8.38 ms** | No algorithmic change |
| typed_lookup | ~7.5 ms | **8.71 ms** | No algorithmic change |
| raw_mixed | 17.46 ms | not re-measured | Minor improvement from match_mask in remove() |
| lru_lookup | 3.40 ms | **3.17 ms** | Reference — not our code |

> **Correction from earlier estimate:** The "-8% mixed improvement" claim was based on
> one run and not reliably reproducible. v0.6.0 is a **correctness + memory release**,
> not a performance release. The lookup gap vs `lru` is unchanged.

### Known Remaining Gap

```
Measured (100K ops):
  PulseMap typed lookup : 8.71 ms
  lru lookup            : 3.17 ms
  Gap                   : 2.7x  ← UNCHANGED from v0.5.0

Root cause (profiled):
  from_bytes deserialization → only ~6% of lookup time (NOT the bottleneck)

  Actual bottlenecks:
    wyhash compute_hash()    → ~35-40% of lookup
    to_bytes() on every get  → ~10%  (key serialized even for read)
    cache misses on bucket   → ~35-40%

→ v0.7.0 will target wyhash replacement (AHash) and zero-copy key borrow.
  TypedSlabPool approach was investigated and rejected — low ROI for numeric types.
```
