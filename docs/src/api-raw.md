# PulseMapRaw (Raw Byte API)

The lowest-level API. Works directly with `&[u8]` byte slices.

```rust
pub type PulseMap = PulseMapRaw;
```

## Construction

```rust
use pulse_map::PulseMap;

// Fixed-size (256 slots)
let map = PulseMap::new(64);

// With auto-resize at 75% load
let map = PulseMap::with_auto_resize(64);
```

## Operations

```rust
// Insert raw bytes
map.insert(b"hello", b"world");

// Lookup — returns Option<&[u8]>
if let Some(value) = map.get(b"hello") {
    println!("Found: {} bytes", value.len());
}

// Remove
let removed: bool = map.remove(b"hello");

// Contains
let exists: bool = map.contains_key(b"hello");
```

## Stats

```rust
println!("Entries:   {}", map.len());
println!("Capacity:  {}", map.capacity());
println!("Load:      {:.1}%", map.load_factor() * 100.0);
println!("Evictions: {}", map.eviction_count());
```

## TTL (v0.6.0+)

```rust
// Set global TTL: entries expire after 500 insertions
map.set_ttl(500);

// Query state
println!("TTL setting: {}", map.get_ttl());       // 500
println!("Epoch:       {}", map.current_epoch()); // total inserts

// Disable TTL
map.set_ttl(0);
```

## Per-Entry TTL (v0.6.1+)

```rust
// Per-entry override
map.insert_ttl(b"session", b"data", 50);      // expires after 50 inserts
map.insert_ttl(b"config", b"val", u64::MAX);  // never expires
map.insert(b"normal", b"val");                 // uses global TTL
```

See the [TTL page](./ttl.md) for full details.

## When to Use PulseMapRaw

- You already have byte-serialized keys/values
- Maximum performance (no serialization overhead)
- Building custom protocols over raw bytes
- Interfacing with C FFI bindings

> **Thread Safety:** `PulseMapRaw` is `Send` but **NOT** `Sync` (fixed in v0.6.2). It cannot be shared across threads via `&PulseMapRaw`.
