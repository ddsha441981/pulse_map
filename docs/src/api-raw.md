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

## When to Use PulseMapRaw

- You already have byte-serialized keys/values
- Maximum performance (no serialization overhead)
- Building custom protocols over raw bytes
- Interfacing with FFI (C, Java, Python, Node.js)
