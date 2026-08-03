# FAQ

## General

### Is PulseMap a HashMap replacement?

**No.** PulseMap is a **bounded cache** with automatic eviction. Use it when:
- You need fixed memory usage
- You can tolerate entries being evicted
- You want in-process caching without Redis

Use `HashMap` when you need to keep every entry forever.

### What happens when PulseMap is full?

The **least-useful entry** in the target bucket is evicted (LFU+LRU hybrid). This is automatic and costs zero additional cache misses. Check `eviction_count()` to monitor.

### Can I turn off eviction?

Not directly, but you can minimize it:
1. Use `with_auto_resize(n)` — the map doubles when 75% full
2. Start with a large initial size
3. Monitor `eviction_count()` — if it's 0, you're fine

### What's the maximum key/value size?

- **Inline mode:** key ≤ 6 bytes, value ≤ 7 bytes (fastest, zero allocation)
- **Slab mode:** unlimited size (heap allocated)

Both modes are transparent — PulseMap automatically chooses the optimal storage.

---

## Performance

### Why is PulseMap faster than HashMap?

Three reasons:
1. **Cache efficiency:** 1 cache line per lookup (vs 2-3 for HashMap)
2. **No pointer chasing:** Inline mode stores data directly in the bucket
3. **H2 fingerprint:** 99.2% of non-matches rejected without key comparison

### When is PulseMap slower?

- **Iteration** — PulseMap doesn't maintain insertion order
- **Very large values** — Slab allocation adds overhead
- **99%+ fill rate** — Every insert causes an eviction

### How does it compare to moka?

**Single-thread:** moka is significantly slower (161ms vs 6.1ms for 100K inserts). moka uses background maintenance threads and heavy synchronization.

**Multi-thread (4T):** ShardedPulseMap is **6.5–12x faster** than moka across all concurrent workloads.

moka's strength is its W-TinyLFU eviction policy (better hit rates on skewed workloads). PulseMap wins on raw throughput.

---

## Memory

### Does PulseMap leak memory?

**No** (since v0.6.0). Slab entries are returned to a free list on eviction/removal.
- **Rust:** `Drop` chains through SlabPool + free list
- **C FFI:** User must call `pulse_map_free()` (documented)

### How much memory does PulseMap use?

```
Memory = num_buckets × 64 bytes + slab_overhead
```

For inline-only workloads (small KV pairs): exactly `num_buckets × 64` bytes.

### Can I use PulseMap in no_std?

Yes! Disable the `std` feature:
```toml
pulse_map = { version = "0.6", default-features = false }
```

Core data structures (`MetaWord`, `Slot`, `Bucket`) work without allocator.

---

## Concurrency

### Is PulseMap thread-safe?

- `ConcurrentPulseMap` — fully thread-safe, single-lock architecture
- `ShardedPulseMap` — fully thread-safe, 16-shard architecture (recommended for 3+ threads)
- `TypedPulseMap` and `PulseMapRaw` — single-threaded only

### Can I set different TTLs for different keys?

**Yes!** Since v0.6.1, use `insert_ttl(key, value, ttl)`:

```rust
cache.set_ttl(500);                              // global default
cache.insert_ttl(b"session", b"data", 50);       // expires after 50
cache.insert_ttl(b"config", b"val", u32::MAX);   // never expires
```

### Can I use PulseMap with async/await?

Yes! `ConcurrentPulseMap` and `ShardedPulseMap` methods are non-blocking (spinlock, not mutex):

```rust
async fn handler(cache: &ShardedPulseMap<String, String>) {
    // Safe to call from async context — won't block the executor
    cache.insert("key".to_string(), "val".to_string());
}
```

### What happens during resize?

- **ConcurrentPulseMap:** Stop-the-world (exclusive write lock). ~1ms per 10K entries.
- **ShardedPulseMap:** `resize_all()` rehashes one shard at a time — other shards remain operational.

---

## FFI

### Is the C API thread-safe?

Yes! The C API wraps `ConcurrentPulseMap` internally. You can call `pulse_map_insert()` from multiple threads simultaneously.
