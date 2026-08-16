# Changelog

See the full [CHANGELOG.md](https://github.com/ddsha441981/pulse_map/blob/main/CHANGELOG.md) in the repository root.

## v0.6.2 (2026-08-11)

### ⚡ Lock-Free Reads + AccessBuffer + u64 TTL

**8 PRs merged** — correctness fixes, performance optimizations, and a breaking TTL type change.

### Breaking Changes
- **TTL types widened to `u64`**: `set_ttl(u64)`, `get_ttl() -> u64`, `current_epoch() -> u64`, `insert_ttl(..., ttl: u64)`
- Sentinel value for "never expire" is now `u64::MAX` (was `u32::MAX`)
- `SlotTTL` layout updated: `{ epoch: u64, ttl: u64 }` (16 bytes per slot)

### Added
- **`AccessBuffer` module** (`engine/access_buffer.rs`): Lock-free lossy ring buffer for deferred LRU/LFU priority updates
- **Lock-free reads**: `MetaWord` backed by `AtomicU64`, enabling relaxed atomic loads without dirtying cache lines
- **Lazy slab lock**: Inline keys skip `slab_pool.lock()` entirely during reads

### Fixed
- **UB Fix**: Removed `unsafe impl Sync` from `PulseMapRaw` — now `Send` only
- **Data loss during resize**: Overflow retry loop ensures zero data loss during rehash
- **TTL wipe during resize**: Epoch/TTL metadata properly migrated
- **Fingerprint entropy collapse**: Shard routing no longer overlaps with h2 fingerprint bits

### Performance (v0.6.1 → v0.6.2)
- GET p99: 1.244µs → 964ns (22.5% faster)
- Throughput: 5.99M → 7.47M ops/s (24.6% faster)
- Contention p99: 1.277µs → 1.134µs (11.2% faster)
- Memory: 34.0 B/entry (unchanged, zero overhead)

---

## v0.6.1 (2026-08-03)

### Added
- **ShardedPulseMap** — 16-shard concurrent map, 2.4–3.1x faster than ConcurrentPulseMap
- **Per-entry TTL** — `insert_ttl(key, value, ttl)` on all map types
  - `ttl = 0`: use global default, `u32::MAX`: never expire, `N`: expire after N inserts
- **Zero-copy key borrow** — `PulseKey::with_key_bytes()` for read-path optimization
- **Competitor benchmarks** — moka + quick_cache single-thread and 4-thread comparisons

### Changed
- `SlotTTL { epoch, ttl }` replaces `Vec<u32>` epochs (8 bytes/slot)
- `is_expired()` now supports per-entry TTL with fallback to default

### Tests
- **58 tests** (up from 57)

---

## v0.6.0 (2026-06-16)

### Added
- **TTL via epoch counter** — `set_ttl(n)` expires entries after `n` insertions
- `get_ttl()`, `current_epoch()` — query TTL state
- **Slab free list** — evicted slab entries reused instead of leaked
- `SlabEntry::rewrite()` — in-place rewrite on free-list reuse

### Changed
- `peek()` + `remove()` now use `match_mask()` — same branchless path as `get()`
- `SlotState::Deleted` removed — was never written, `Tombstone` is now value `2`
- `find_free_slot()` simplified to `!= Full` check

### Fixed
- **Memory leak**: slab entries on eviction/remove now returned to free list
- Slot layout: slab slots store `usize` index instead of raw `*const SlabEntry`

### Tests
- **57 tests** (up from 50)

---

## v0.5.0 (2026-05-26)


### Added
- **Multi-language FFI bindings** — C, Python (PyO3), Java (Panama FFM), Node.js (napi-rs)
- `ConcurrentPulseMap` — thread-safe wrapper with per-bucket spinlocks
- Auto-resize support (`with_auto_resize()`)
- `peek()` method — lookup without eviction priority update
- Unicode support across all bindings
- `Cleaner`-based GC safety net for Java bindings
- Null-safety checks across all bindings (63 total tests)

### Changed
- Workspace split: `pulse_map` (core) + `pulse_map_bindings` (FFI)
- Documentation URL: `https://docs.rs/pulse_map`
- MSRV declared: `rust-version = "1.70.0"`

## v0.4.0 (2026-05-26)

### Added
- Benchmark suite via Criterion
- SIMD H2 matching (optional, x86_64)
- `TypedPulseMap<K, V>` with `PulseKey`/`PulseValue` traits
- Iteration support (`RawIter`, `TypedIter`)

## v0.3.0 (2026-05-26)

### Added
- Dynamic resize support
- `no_std` compatibility
- Entry API improvements

## v0.2.0 (2026-05-22)

### Added
- LFU+LRU hybrid eviction (MetaWord)
- WyHash integration
- H2 fingerprint matching

## v0.1.0 (2026-05-22)

### Added
- Initial release
- 64-byte cache-line bucket architecture
- Inline + slab dual-mode storage
