# Concurrency Model

PulseMap uses a **two-level locking** scheme designed for maximum concurrent throughput.

## Lock Architecture

```
Level 1: RwLock (global)
  ├── Read lock: normal operations (get, insert, remove)
  └── Write lock: resize only (rare, stop-the-world)

Level 2: Per-Bucket Spinlock (AtomicU8)
  └── One lock per bucket — only contention is same-bucket access
```

### Why This Works

- **Different buckets = zero contention.** Two threads accessing different buckets run fully in parallel.
- **Same bucket = brief spinlock.** Bucket operations are ~10ns, so spin wait is negligible.
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

1. **Use `Arc<ConcurrentPulseMap>`** — never `Mutex<PulseMap>`
2. **Pre-size for known workloads** — avoids resize pauses
3. **Use `peek()` for read-heavy paths** — avoids spinlock on eviction metadata update
4. **Monitor `eviction_count()`** — high eviction = undersized map
