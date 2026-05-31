# Changelog

See the full [CHANGELOG.md](https://github.com/ddsha441981/pulse_map/blob/main/CHANGELOG.md) in the repository root.

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
