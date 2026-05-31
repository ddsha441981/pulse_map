# API Reference

PulseMap provides three main types, each suited for different use cases:

| Type | Thread-Safe | Key/Value Types | When to Use |
|------|:-----------:|:-:|:-:|
| `PulseMapRaw` | ❌ | `[u8]` bytes | Maximum performance, raw byte access |
| `TypedPulseMap<K, V>` | ❌ | Any `PulseKey`/`PulseValue` | Type-safe single-threaded |
| `ConcurrentPulseMap<K, V>` | ✅ | Any `PulseKey`/`PulseValue` | Production concurrent cache |

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

## Common Methods

All three map types share these methods:

| Method | Signature | Description |
|--------|-----------|-------------|
| `new()` | `fn new(num_buckets: usize) -> Self` | Fixed-size map |
| `insert()` | `fn insert(key, value)` | Insert or update |
| `get()` | `fn get(&key) -> Option<V>` | Lookup (updates eviction priority) |
| `peek()` | `fn peek(&key) -> Option<V>` | Lookup (no priority update) |
| `remove()` | `fn remove(&key) -> bool` | Delete entry |
| `contains_key()` | `fn contains_key(&key) -> bool` | Existence check |
| `len()` | `fn len() -> usize` | Number of entries |
| `is_empty()` | `fn is_empty() -> bool` | Check if empty |
| `capacity()` | `fn capacity() -> usize` | Total slot count |
| `load_factor()` | `fn load_factor() -> f64` | `len / capacity` |
| `eviction_count()` | `fn eviction_count() -> usize` | Total evictions |

See sub-pages for type-specific APIs.
