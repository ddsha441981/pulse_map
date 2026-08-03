# Contributing

See the full [CONTRIBUTING.md](https://github.com/ddsha441981/pulse_map/blob/main/CONTRIBUTING.md) in the repository root.

## Quick Start

```bash
git clone https://github.com/ddsha441981/pulse_map.git
cd pulse_map
cargo build
cargo test
```

## Before Submitting

```bash
cargo fmt               # Format
cargo clippy -- -D warnings  # Lint (zero warnings)
cargo test              # All tests pass
cargo doc --no-deps     # No doc warnings
```

## Architecture

```
Layer 5: sharded.rs → ShardedPulseMap (16 shards)
Layer 4: sync.rs    → ConcurrentPulseMap
Layer 3: lib.rs     → User API (TypedPulseMap, PulseMap)
Layer 2: raw.rs     → Hash table logic + per-entry TTL
Layer 1: engine/    → MetaWord, Slot, Bucket, hash, slab
```

## Key Rules

1. Every bucket = exactly 64 bytes
2. Eviction is zero-cost (metadata in cache line)
3. No heap allocation in hot path
4. Thread safety via `&self` (no `&mut self` for CRUD)

## License

By contributing, you agree your contributions are licensed under **MIT OR Apache-2.0**.
