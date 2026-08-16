# API Reference

PulseMap provides four map types. Choose based on your concurrency requirements:

| Type | Threads | Key/Value | When to Use |
|------|:-------:|:---------:|-------------|
| `PulseMapRaw` | ❌ Single | `[u8]` bytes | Max perf, raw bytes, FFI (`Send + !Sync`) |
| `TypedPulseMap<K, V>` | ❌ Single | Any `PulseKey`/`PulseValue` | Type-safe single-threaded |
| `ConcurrentPulseMap<K, V>` | ✅ 1–2T | Any `PulseKey`/`PulseValue` | Low-contention concurrent |
| `ShardedPulseMap<K, V>` | ✅ **3+T** | Any `PulseKey`/`PulseValue` | **High-concurrency production** |

## Type Aliases

```rust
/// Raw byte-level map — maximum control
pub type PulseMap = PulseMapRaw;
```

## Traits

### PulseKey

```rust
pub trait PulseKey: Clone + PartialEq {
    type Bytes: AsRef<[u8]>;
    fn to_bytes(&self) -> Self::Bytes;
    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}
```

**Implemented for:** `String`, `Vec<u8>`, `u8`, `u16`, `u32`, `u64`, `u128`, `i8`, `i16`, `i32`, `i64`, `i128`

### PulseValue

```rust
pub trait PulseValue: Clone {
    type Bytes: AsRef<[u8]>;
    fn to_bytes(&self) -> Self::Bytes;
    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}
```

**Implemented for:** Same types as `PulseKey`.

## Common Methods (all map types)

| Method | Description |
|--------|-------------|
| `new(num_buckets)` | Fixed-size map |
| `insert(key, value)` | Insert or update (uses global TTL) |
| `insert_ttl(key, value, ttl: u64)` | Insert with per-entry TTL override *(v0.6.1+)* |
| `get(&key)` | Lookup — updates eviction priority |
| `peek(&key)` | Lookup — no priority update (pure read) |
| `remove(&key)` | Delete, returns `bool` |
| `contains_key(&key)` | Existence check |
| `len()` | Number of live entries |
| `is_empty()` | Check if empty |
| `capacity()` | Total slot count |
| `load_factor()` | `len / capacity` |
| `eviction_count()` | Total evicted entries |
| `set_ttl(n: u64)` | Global TTL in insertion epochs |
| `get_ttl() -> u64` | Current global TTL |
| `current_epoch() -> u64` | Total insertions so far |

See sub-pages for type-specific APIs and examples.
