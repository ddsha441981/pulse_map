# Feature Flags

PulseMap uses Cargo feature flags to control optional functionality.

## Available Features

| Feature | Default | Description |
|---------|:-------:|-------------|
| `std` | ✅ | Standard library (ConcurrentPulseMap, ShardedPulseMap, threading) |
| `simd` | ❌ | SIMD H2 matching acceleration (x86_64 SSE2) |

## Feature Details

### `std` (default)

Enables:
- `ConcurrentPulseMap` (requires `RwLock`, `Mutex`, `AtomicU8`)
- `ShardedPulseMap` (16 shards, requires `std`)
- `SlabPool` (requires heap allocation)
- `Display` and `Debug` formatting

```toml
# With std (default) — v0.6.1
pulse_map = "0.6.1"

# Without std (no_std mode)
pulse_map = { version = "0.6.1", default-features = false }
```

### `no_std` Mode

When `std` is disabled, only the core data structures are available:

- `MetaWord` — 8-byte metadata packing
- `Slot` — 14-byte inline/slab storage
- `Bucket` — 64-byte cache line unit
- `PulseMapRaw` — basic insert/get/remove/TTL

**Use case:** Embedded systems, OS kernels, WebAssembly.

Works on targets without native 64-bit atomics (WASM32, ARMv7-M, 32-bit) via `portable-atomic` fallback.

### `simd`

Enables SIMD-accelerated H2 fingerprint matching on x86_64:

```toml
pulse_map = { version = "0.6.1", features = ["simd"] }
```

Uses SSE2 `_mm_cmpeq_epi8` to compare all 4 H2 fingerprints simultaneously:

```rust
// Without SIMD: sequential comparison
fn match_mask_scalar(h2: u8) -> u8 {
    let mut mask = 0;
    for i in 0..4 {
        if self.get_h2(i) == h2 { mask |= 1 << i; }
    }
    mask
}

// With SIMD: single instruction
fn match_mask_simd(h2: u8) -> u8 {
    let needle = _mm_set1_epi8(h2 as i8);
    let result = _mm_cmpeq_epi8(self.as_xmm(), needle);
    _mm_movemask_epi8(result) as u8 & 0x0F
}
```

**Performance impact:** ~15% faster lookups on x86_64 with high bucket occupancy.

## Compile-Time Configuration

```bash
# Build with all features
cargo build --all-features

# Build for no_std
cargo build --no-default-features

# Build with SIMD only
cargo build --features simd

# Test specific feature combination
cargo test --no-default-features
cargo test --features simd

# Docs.rs (all features)
cargo doc --all-features --no-deps --open
```
