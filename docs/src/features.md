# Feature Flags

| Feature | Default | Purpose |
|---|---|---|
| `std` | yes | Concurrent/sharded maps and `From<HashMap>` |
| `simd` | no | SSE2 H2 matching on x86_64 |
| `critical-section` | no | portable-atomic fallback when native CAS is absent |

Raw/typed maps, slab storage, iterators and formatting work without std, using
`alloc`. The cache itself requires a heap allocator. Fixed-layout primitives
MetaWord/Slot/Bucket can be used without creating a heap-backed map.

```toml
pulse_map = { version = "0.6", default-features = false }
```

Targets without CAS additionally require the `critical-section` feature **and** a
critical-section implementation supplied by the binary/HAL. See [Embedded](embedded-no-std.md).
SIMD uses multiple SSE2 operations plus state filtering; its benefit needs measurement,
not a fixed percentage promise. Other architectures use scalar matching.

```bash
cargo test --all-features
cargo test --no-default-features
cargo test --features simd
```
