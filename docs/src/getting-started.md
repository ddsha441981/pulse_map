# Getting Started

## Installation

Add PulseMap to your `Cargo.toml`:

```toml
[dependencies]
pulse_map = "0.6.1"
```

Or via command line:

```bash
cargo add pulse_map
```

## Minimum Supported Rust Version

PulseMap requires **Rust 1.70.0** or later.

---

## Your First PulseMap

### 1. Single-Threaded — `TypedPulseMap<K, V>`

```rust
use pulse_map::TypedPulseMap;

fn main() {
    // 64 buckets = 256 slot capacity
    let mut map: TypedPulseMap<String, String> = TypedPulseMap::new(64);

    map.insert("name".to_string(), "Deendayal".to_string());
    map.insert("lang".to_string(), "Rust".to_string());

    assert_eq!(map.get(&"name".to_string()), Some("Deendayal".to_string()));
    assert_eq!(map.get(&"missing".to_string()), None);

    map.remove(&"lang".to_string());

    println!("Entries:   {}", map.len());
    println!("Capacity:  {}", map.capacity());
    println!("Evictions: {}", map.eviction_count());
}
```

### 2. Multi-Threaded — `ConcurrentPulseMap<K, V>`

Good for 1–2 threads:

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
use std::thread;

fn main() {
    let map = Arc::new(ConcurrentPulseMap::<String, u64>::with_auto_resize(64));

    let handles: Vec<_> = (0..2).map(|t| {
        let m = map.clone();
        thread::spawn(move || {
            for i in 0..1000 {
                m.insert(format!("t{}_{}", t, i), i as u64);
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }
    println!("Total: {}", map.len());
}
```

### 3. High-Concurrency — `ShardedPulseMap<K, V>` ✅ Recommended

Best for **3+ threads** — 2.4–3.1x faster than ConcurrentPulseMap:

```rust
use pulse_map::ShardedPulseMap;
use std::sync::Arc;
use std::thread;

fn main() {
    // 16 shards × 256 buckets = 16,384 capacity
    let map = Arc::new(ShardedPulseMap::<u32, u64>::new(256));

    let handles: Vec<_> = (0..8).map(|t| {
        let m = map.clone();
        thread::spawn(move || {
            for i in 0..10_000u32 {
                m.insert(t * 10_000 + i, i as u64);
            }
        })
    }).collect();

    for h in handles { h.join().unwrap(); }
    println!("Entries:   {}", map.len());
    println!("Evictions: {}", map.eviction_count());
}
```

### 4. With Per-Entry TTL (v0.6.1+)

```rust
use pulse_map::ShardedPulseMap;

let map = ShardedPulseMap::<String, String>::new(256);
map.set_ttl(1000);  // global: expire after 1000 inserts

// Per-entry overrides
map.insert_ttl("session:abc".to_string(), "data".to_string(), 50);     // short-lived
map.insert_ttl("config:key".to_string(), "value".to_string(), u32::MAX); // never expire
map.insert("normal".to_string(), "val".to_string());                     // uses global 1000
```

---

## Choosing the Right Type

| Type | Threads | TTL | Best For |
|------|:-------:|:---:|----------|
| `PulseMapRaw` | ❌ Single | ✅ | Raw `[u8]` keys, FFI, max perf |
| `TypedPulseMap<K, V>` | ❌ Single | ✅ | Type-safe single-threaded cache |
| `ConcurrentPulseMap<K, V>` | ✅ 1-2T | ✅ | Low-contention concurrent cache |
| `ShardedPulseMap<K, V>` | ✅ **3+T** | ✅ | **High-concurrency production** |

## Capacity Planning

```
actual_buckets = next_power_of_2(num_buckets)
total_capacity = actual_buckets × 4  (4 slots per bucket)

# ShardedPulseMap:
total_capacity = 16 × actual_buckets × 4
```

| `num_buckets` | Actual | Capacity | Memory | ShardedPulseMap |
|:---:|:---:|:---:|:---:|:---:|
| 64 | 64 | 256 | 4 KB | 4 KB × 16 = 64 KB |
| 256 | 256 | 1,024 | 16 KB | 256 KB |
| 1,024 | 1,024 | 4,096 | 64 KB | 1 MB |
| 65,536 | 65,536 | 262,144 | 4 MB | 64 MB |

> **Rule of thumb:** `num_buckets = expected_entries / 3`. The ~75% fill rate balances performance with memory.

## Running Examples

```bash
git clone https://github.com/ddsha441981/pulse_map.git
cd pulse_map

cargo run --example basic
cargo run --example concurrent
cargo bench
```
