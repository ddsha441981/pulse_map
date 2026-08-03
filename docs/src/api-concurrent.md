# ConcurrentPulseMap

Thread-safe map for production concurrent workloads. All methods take `&self` — no `Mutex` wrapping needed.

## Construction

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;

// Fixed-size
let map = Arc::new(ConcurrentPulseMap::<String, String>::new(1024));

// Auto-resize (doubles at 75% load)
let map = Arc::new(ConcurrentPulseMap::<String, u64>::with_auto_resize(256));
```

## Thread-Safe Operations

All methods take `&self` — safe to call from multiple threads simultaneously:

```rust
use std::thread;

let map = Arc::new(ConcurrentPulseMap::<u32, u32>::with_auto_resize(64));

// Spawn writers
let handles: Vec<_> = (0..8).map(|t| {
    let m = map.clone();
    thread::spawn(move || {
        for i in 0..10_000 {
            m.insert(t * 10_000 + i, i);
        }
    })
}).collect();

for h in handles { h.join().unwrap(); }

// Read from any thread — no lock needed
println!("Entries: {}", map.len());
```

## API

```rust
// Insert (thread-safe, no &mut needed)
map.insert("key".to_string(), "value".to_string());

// Insert with per-entry TTL (v0.6.1+)
map.insert_ttl("key".to_string(), "value".to_string(), 100);  // expires after 100 inserts
map.insert_ttl("key".to_string(), "value".to_string(), u32::MAX);  // never expires

// Get (updates eviction priority atomically)
let val: Option<String> = map.get(&"key".to_string());

// Peek (no priority update — pure read)
let val: Option<String> = map.peek(&"key".to_string());

// Remove
let existed: bool = map.remove(&"key".to_string());

// Contains
let exists: bool = map.contains_key(&"key".to_string());
```

> **Tip:** For 3+ threads, use [`ShardedPulseMap`](./api-sharded.md) — 2.4–3.1x faster under contention.

## Manual Resize

```rust
// Force resize to 2048 buckets (8192 capacity)
map.resize(2048);
```

> ⚠️ **Resize is stop-the-world** — acquires exclusive write lock, blocking all operations until rehashing completes. This is brief (~1ms for 10K entries) but causes a latency spike.

## Stats (Lock-Free)

```rust
map.len()             // AtomicUsize — no lock
map.capacity()        // Acquires read lock (cheap)
map.load_factor()     // Derived from len/capacity
map.eviction_count()  // AtomicUsize — no lock
map.num_buckets()     // Acquires read lock
```

## Locking Model

```
Read operations (get, peek, contains, stats):
  └── RwLock::read() + per-bucket spinlock

Write operations (insert, remove):
  └── RwLock::read() + per-bucket spinlock

Resize:
  └── RwLock::write() (exclusive — blocks everything)
```

**Key insight:** Normal reads and writes only acquire a **read lock** on the RwLock, so they run concurrently. The per-bucket spinlock serializes access to the same bucket only.

## Production Pattern

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;

// Shared application cache
struct AppState {
    cache: ConcurrentPulseMap<String, String>,
}

impl AppState {
    fn new() -> Self {
        Self {
            cache: ConcurrentPulseMap::with_auto_resize(4096),
        }
    }
}

// Use from any handler — no mutex needed
fn handle_request(state: &AppState, key: &str) -> Option<String> {
    state.cache.get(&key.to_string())
}
```
