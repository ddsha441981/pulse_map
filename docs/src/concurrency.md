# Concurrency Model

PulseMap offers two concurrent map implementations:

- **ConcurrentPulseMap** — single map with per-bucket spinlocks (good for 1–2 threads)
- **ShardedPulseMap** — 16 independent shards (optimal for 3+ threads, 2.4–3.1x faster)

## Lock Architecture

```
Level 1: RwLock (global)
  ├── Read lock: normal operations (get, insert, remove)
  └── Write lock: resize only (rare, stop-the-world)

Level 2: Per-Bucket Spinlock (AtomicU8)
  └── One lock per bucket — only contention is same-bucket access
```

### Why This Works

- **Lock-free reads:** MetaWord uses `AtomicU64` for lock-free reads, and `get()` pushes to a deferred `AccessBuffer` without acquiring exclusive bucket spinlocks for metadata updates.
- **Different buckets = zero contention.** Two threads accessing different buckets for writes run fully in parallel.
- **Same bucket = brief spinlock.** Bucket write operations are ~10ns, so spin wait is negligible.
- **RwLock read = cheap.** Multiple threads hold read locks simultaneously.
- **Resize = rare.** Only triggered at 75% load with auto-resize enabled.

## Spinlock Implementation

```rust
struct BucketLocks {
    locks: Vec<AtomicU8>,  // 1 byte per bucket
}

fn lock(&self, idx: usize) {
    while self.locks[idx]
        .compare_exchange_weak(0, 1, Acquire, Relaxed)
        .is_err()
    {
        std::hint::spin_loop();  // CPU hint: we're spinning
    }
}

fn unlock(&self, idx: usize) {
    self.locks[idx].store(0, Release);
}
```

**RAII guard** ensures unlock on all exit paths (including panics):

```rust
struct BucketGuard<'a> {
    locks: &'a BucketLocks,
    idx: usize,
}

impl Drop for BucketGuard<'_> {
    fn drop(&mut self) {
        self.locks.unlock(self.idx);
    }
}
```

## &self API

All CRUD methods take `&self`, not `&mut self`:

```rust
// No Mutex needed — just Arc!
let map = Arc::new(ConcurrentPulseMap::<String, String>::new(1024));

// All these are &self calls:
map.insert("key".to_string(), "value".to_string());
map.get(&"key".to_string());
map.remove(&"key".to_string());
map.contains_key(&"key".to_string());
map.len();
```

This is possible because internal mutation is protected by the per-bucket spinlocks + `UnsafeCell`.

## Resize Semantics

Resize uses a **stop-the-world** approach:

```
1. Acquire RwLock::write() — blocks ALL operations
2. Allocate new bucket array (2× size)
3. Rehash all entries from old → new buckets
4. Swap in new state
5. Release write lock — operations resume
```

**Duration:** ~1ms for 10K entries, ~10ms for 100K entries.

**When it happens:**
- Auto-resize: when `load_factor > 0.75` during `insert()`
- Manual: when you call `map.resize(new_num_buckets)`

## Thread Safety Guarantees

**Note on UB Fix:** `PulseMapRaw` is `Send` but NOT `Sync`. The concurrent wrappers handle all the necessary synchronization to safely share the map.

| Operation | Concurrent with | Safe? |
|-----------|----------------|:-----:|
| `get()` | `get()` | ✅ (parallel if different buckets) |
| `get()` | `insert()` | ✅ (serialized per bucket) |
| `insert()` | `insert()` | ✅ (serialized per bucket) |
| `insert()` | `remove()` | ✅ (serialized per bucket) |
| Any | `resize()` | ✅ (blocked until resize completes) |
| `resize()` | `resize()` | ✅ (second caller sees it's done, returns) |

## Memory Ordering

- **Acquire** on lock acquisition (load-after-lock sees latest writes)
- **Release** on lock release (store-before-unlock is visible to next acquirer)
- **Relaxed** for `count` and `eviction_count` (eventual consistency is fine for stats)

## Best Practices

1. **Use `ShardedPulseMap` for 3+ threads** — 2.4–3.1x faster than ConcurrentPulseMap
2. **Use `Arc<ShardedPulseMap>` or `Arc<ConcurrentPulseMap>`** — never `Mutex<PulseMap>`
3. **Pre-size for known workloads** — avoids resize pauses
4. **Use `peek()` for read-heavy paths** — avoids spinlock on eviction metadata update
5. **Monitor `eviction_count()`** — high eviction = undersized map

## ShardedPulseMap (v0.6.1+)

For high-concurrency workloads, `ShardedPulseMap` splits data across **16 independent shards**:

```
ShardedPulseMap:
  ├── Shard 0:  ConcurrentPulseMap (own RwLock + spinlocks)
  ├── Shard 1:  ConcurrentPulseMap (own RwLock + spinlocks)
  ├── ...
  └── Shard 15: ConcurrentPulseMap (own RwLock + spinlocks)

Shard selection: h1 & 0xF (low-bits routing)
```

**Result:** Threads accessing different shards have **zero lock contention**.

```rust
use pulse_map::ShardedPulseMap;
use std::sync::Arc;

let map = Arc::new(ShardedPulseMap::<u32, u32>::new(4096));
// Same API as ConcurrentPulseMap — just faster under contention
```

### resize_all() — Per-Shard Rehash

Unlike ConcurrentPulseMap's stop-the-world resize, `resize_all()` rehashes one shard at a time. Other shards remain fully operational.

See [ShardedPulseMap API](./api-sharded.md) for full details.
