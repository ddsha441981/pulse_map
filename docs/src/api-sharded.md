# ShardedPulseMap

> Added in **v0.6.1**

A 16-shard concurrent map built on top of `ConcurrentPulseMap`. Each shard is a fully independent `ConcurrentPulseMap` — near-zero cross-thread contention.

## Why Sharded?

`ConcurrentPulseMap` uses a single `RwLock` + per-bucket spinlocks. Under high thread counts, the `RwLock` becomes a bottleneck. `ShardedPulseMap` eliminates this by splitting data across 16 shards — each with its own `RwLock`.

**Result: 2.4–3.1x faster** than `ConcurrentPulseMap` on 4-thread workloads.

## Construction

```rust
use pulse_map::ShardedPulseMap;
use std::sync::Arc;

// 16 shards × 4096 buckets each = 262,144 total capacity
let map = Arc::new(ShardedPulseMap::<u32, u32>::new(4096));

// Auto-resize: each shard auto-grows at 75% load
let map = Arc::new(ShardedPulseMap::<String, String>::with_auto_resize(256));
```

## Thread-Safe Operations

Same API as `ConcurrentPulseMap` — all methods take `&self`:

```rust
use std::thread;

let map = Arc::new(ShardedPulseMap::<u32, u32>::new(4096));

// 8 threads inserting concurrently
let handles: Vec<_> = (0..8).map(|t| {
    let m = map.clone();
    thread::spawn(move || {
        for i in 0..10_000 {
            m.insert(t * 10_000 + i, i);
        }
    })
}).collect();
for h in handles { h.join().unwrap(); }

println!("Total entries: {}", map.len());
```

## API

```rust
// CRUD — routed to shard by key hash
map.insert(key, value);
map.insert_ttl(key, value, ttl);  // per-entry TTL (v0.6.1+)
map.get(&key);                     // Option<V>
map.peek(&key);                    // no priority update
map.remove(&key);                  // bool
map.contains_key(&key);            // bool

// TTL — applied to all shards
map.set_ttl(500u64);
map.get_ttl();                     // → 500u64
map.current_epoch();               // max across shards (u64)

// Stats — aggregated across all shards
map.len();
map.capacity();
map.load_factor();
map.eviction_count();

// Resize — per-shard, no stop-the-world
map.resize_all(new_buckets_per_shard);
```

## Shard Selection

Shards are selected using the **low 4 bits** of the wyhash:

```
shard_index = h1 & 15    // bits [3:0] → 0..15
bucket_index = (h1 >> 4) & mask  // upper bits → bucket within shard
```

This ensures shard selection is **independent** from bucket selection — no correlation, no hot-spotting.

## resize_all() — No Stop-the-World

Unlike `ConcurrentPulseMap::resize()` which blocks ALL operations, `resize_all()` rehashes **one shard at a time**. Other shards remain fully operational during the resize.

```rust
// Only shard N is blocked while being rehashed
// Shards 0..N-1 and N+1..15 continue serving requests
map.resize_all(8192);
```

## When to Use

| Scenario | Best Map |
|----------|----------|
| Single-threaded | `TypedPulseMap` |
| 1–2 threads | `ConcurrentPulseMap` |
| **3+ threads** | **`ShardedPulseMap`** ✅ |
| High-contention workloads | **`ShardedPulseMap`** ✅ |

## Performance

| Benchmark (4T, 100K ops) | ShardedPulseMap | ConcurrentPulseMap | Speedup |
|--------------------------|:--------------:|:-----------------:|:-------:|
| INSERT | **8.8 ms** | 20.2 ms | **2.3x** |
| LOOKUP | **9.0 ms** | 35.0 ms | **3.9x** |
| MIXED | **15.9 ms** | 46.6 ms | **2.9x** |
