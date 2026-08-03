# PulseMap

**A CPU cache-line hash table with zero-cost eviction.**

> **💡 Use PulseMap anywhere you'd use HashMap but can't afford unbounded memory growth.**

---

## What is PulseMap?

PulseMap is a **bounded, concurrent hash table** where every bucket fits in exactly **one 64-byte CPU cache line**. Unlike `HashMap` which grows forever until you run out of memory, PulseMap has a fixed capacity and **automatically evicts** the least-useful entries when full.

**Key insight:** By packing metadata (state, H2 fingerprint, frequency counter, recency bits) into the same cache line as the data slots, eviction decisions cost **zero additional cache misses**.

## Why PulseMap?

| Problem | HashMap | Redis | PulseMap |
|---------|---------|-------|----------|
| Memory growth | ❌ Unbounded → OOM | ✅ Bounded | ✅ Bounded |
| Lookup latency | ~10ns | ~100μs (network) | **~5ns** |
| Eviction | ❌ None | ✅ LRU | ✅ LFU+LRU hybrid |
| Thread safety | ❌ `Mutex<HashMap>` | ✅ Single-threaded | ✅ Per-bucket locks |
| GC pauses | N/A | N/A | **Zero** |
| Cache efficiency | ❌ Random | N/A | ✅ 1 cache line/bucket |

## Quick Example

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
use std::thread;

// Create a thread-safe map with auto-resize
let map = Arc::new(ConcurrentPulseMap::<String, u64>::with_auto_resize(256));

// Concurrent writes from 4 threads
let handles: Vec<_> = (0..4).map(|t| {
    let m = map.clone();
    thread::spawn(move || {
        for i in 0..1000 {
            m.insert(format!("key_{}", t * 1000 + i), i as u64);
        }
    })
}).collect();

for h in handles { h.join().unwrap(); }

// Read back
assert!(map.get(&"key_0".to_string()).is_some());
println!("Entries: {}, Evictions: {}", map.len(), map.eviction_count());
```

## Features

- **🏗️ Cache-Line Architecture** — 64-byte buckets with 4 slots each
- **⚡ Zero-Cost Eviction** — LFU+LRU metadata embedded in bucket
- **🔒 Thread-Safe** — Per-bucket spinlocks, `&self` API
- **🏗️ 16-Shard Concurrency** — ShardedPulseMap: 2.4–3.1x faster than global lock
- **⏱️ Per-Entry TTL** — Individual expiry per key, or global default
- **📏 Bounded Memory** — Fixed capacity, no unbounded growth
- **🔄 Auto-Resize** — Optional dynamic growth at 75% load
- **🌐 C FFI Bindings** — Use from C, or build your own language bridge
- **🔧 no_std Compatible** — Core data structures work without allocator

## Supported Platforms

| Platform | Status |
|----------|:------:|
| Linux x86_64 | ✅ |
| macOS x86_64 / ARM64 | ✅ |
| Windows x86_64 | ✅ |
| MSRV: Rust 1.70.0 | ✅ |

## License

Dual licensed under [MIT](../LICENSE-MIT) or [Apache-2.0](../LICENSE-APACHE).

Copyright (c) 2026 Deendayal Kumawat.
