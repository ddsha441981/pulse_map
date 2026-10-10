# Getting Started

```toml
[dependencies]
pulse_map = "0.6"
```

Library MSRV: Rust 1.70. Benchmark/dev dependencies may require newer toolchains.

```rust
use pulse_map::TypedPulseMap;

let mut cache = TypedPulseMap::<u32, u32>::new(16);
cache.insert(7, 70);
assert_eq!(cache.get(&7), Some(70));
assert!(cache.contains_key(&7));
assert!(cache.remove(&7));
```

Raw/typed constructors take **buckets**, rounded up to a power of two (minimum one).
Capacity = buckets × 4. A raw/typed inline map needs **128 B/bucket** for buckets
and TTL storage: 16 buckets = 64 nominal slots = 2 KiB, before allocator overhead.

Concurrent types additionally allocate locks and access tracking. ShardedPulseMap
takes buckets **per shard**; multiply by 16 shards and four slots. Local collisions
can evict before the whole map is full. Headroom helps but is not a retention guarantee.

```rust
use pulse_map::ShardedPulseMap;
let cache = ShardedPulseMap::<u32, u32>::new(64);
assert_eq!(cache.capacity(), 16 * 64 * 4);
cache.insert_ttl(7, 70, u64::MAX); // immune to expiry, still subject to eviction
assert_eq!(cache.peek(&7), Some(70));
```

Raw/typed are fixed-size. Concurrent/sharded offer optional auto-growth above 75%
occupied slots; that mode is not a fixed-memory cache. See the API pages for locking,
TTL and resize semantics.
