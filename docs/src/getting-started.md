# Getting Started

## Installation

Add PulseMap to your `Cargo.toml`:

```toml
[dependencies]
pulse_map = "0.5"
```

Or via command line:

```bash
cargo add pulse_map
```

## Minimum Supported Rust Version

PulseMap requires **Rust 1.70.0** or later.

## Your First PulseMap

### Basic Usage (Single-Threaded)

```rust
use pulse_map::TypedPulseMap;

fn main() {
    // Create a map with 64 buckets (256 slot capacity)
    let mut map: TypedPulseMap<String, String> = TypedPulseMap::new(64);

    // Insert
    map.insert("name".to_string(), "Deendayal".to_string());
    map.insert("lang".to_string(), "Rust".to_string());

    // Lookup
    assert_eq!(map.get(&"name".to_string()), Some("Deendayal".to_string()));
    assert_eq!(map.get(&"missing".to_string()), None);

    // Remove
    assert!(map.remove(&"lang".to_string()));
    assert_eq!(map.len(), 1);

    // Stats
    println!("Capacity: {}", map.capacity());      // 256
    println!("Load: {:.1}%", map.load_factor() * 100.0);
    println!("Evictions: {}", map.eviction_count());
}
```

### Concurrent Usage (Multi-Threaded)

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
use std::thread;

fn main() {
    // Thread-safe, auto-resizing map
    let map = Arc::new(ConcurrentPulseMap::<String, u64>::with_auto_resize(64));

    // 4 writer threads
    let handles: Vec<_> = (0..4).map(|t| {
        let m = map.clone();
        thread::spawn(move || {
            for i in 0..1000 {
                m.insert(format!("t{}_{}", t, i), i as u64);
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }

    // All methods take &self — no mutex needed!
    println!("Total: {}", map.len());
    println!("Capacity: {}", map.capacity());
}
```

## Choosing the Right Type

| Type | Thread-Safe | Use Case |
|------|:-----------:|----------|
| `PulseMapRaw` | ❌ | Raw `[u8]` key/value, maximum performance |
| `TypedPulseMap<K, V>` | ❌ | Type-safe single-threaded cache |
| `ConcurrentPulseMap<K, V>` | ✅ | Production concurrent cache |

### Type Rules

- `K` must implement `PulseKey` (provided for: `String`, `Vec<u8>`, `u8`..`u128`, `i8`..`i128`)
- `V` must implement `PulseValue` (same types)
- Both traits require: `to_bytes()` and `from_bytes()`

## Capacity Planning

PulseMap capacity is calculated as:

```
actual_buckets = next_power_of_2(num_buckets)
total_capacity = actual_buckets × 4  (4 slots per bucket)
```

| Input `num_buckets` | Actual Buckets | Capacity | Memory |
|:---:|:---:|:---:|:---:|
| 16 | 16 | 64 | 1 KB |
| 64 | 64 | 256 | 4 KB |
| 256 | 256 | 1,024 | 16 KB |
| 1,024 | 1,024 | 4,096 | 64 KB |
| 65,536 | 65,536 | 262,144 | 4 MB |

> **Rule of thumb:** Start with `num_buckets = expected_entries / 3`. The ~75% fill rate balances performance with memory.

## Running Examples

```bash
# Clone
git clone https://github.com/ddsha441981/pulse_map.git
cd pulse_map

# Basic example
cargo run --example basic

# Concurrent example
cargo run --example concurrent

# Benchmarks
cargo bench
```
