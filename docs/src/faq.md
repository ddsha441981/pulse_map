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

### How does it compare to DashMap?

PulseMap is ~1.3-1.5× faster for concurrent workloads because:
- Per-bucket spinlocks (1 byte) vs DashMap's shard-level locks
- Cache-line-aligned buckets vs cache-unfriendly nodes

---

## Memory

### Does PulseMap leak memory?

**No** (since v0.5.0). All bindings are audited:
- **Rust:** Automatic `Drop` chains through SlabPool
- **Python:** PyO3 auto-drop on GC
- **Java:** `Cleaner` safety net + `AutoCloseable`
- **Node.js:** napi-rs auto-drop on GC
- **C:** User must call `pulse_map_free()` (documented)

### How much memory does PulseMap use?

```
Memory = num_buckets × 64 bytes + slab_overhead
```

For inline-only workloads (small KV pairs): exactly `num_buckets × 64` bytes.

### Can I use PulseMap in no_std?

Yes! Disable the `std` feature:
```toml
pulse_map = { version = "0.5", default-features = false }
```

Core data structures (`MetaWord`, `Slot`, `Bucket`) work without allocator.

---

## Concurrency

### Is PulseMap thread-safe?

`ConcurrentPulseMap` is fully thread-safe. `TypedPulseMap` and `PulseMapRaw` are single-threaded.

### Can I use PulseMap with async/await?

Yes! `ConcurrentPulseMap` methods are non-blocking (spinlock, not mutex):

```rust
async fn handler(cache: &ConcurrentPulseMap<String, String>) {
    // Safe to call from async context — won't block the executor
    cache.insert("key".to_string(), "val".to_string());
}
```

### What happens during resize?

Resize acquires an exclusive write lock (stop-the-world). All reads and writes block until rehashing completes. Duration: ~1ms per 10K entries.

---

## FFI

### Which Java version do I need?

Java **22 or later**. PulseMap Java bindings use the Panama FFM API (Foreign Function & Memory), which became stable in Java 22.

### Do I need to install Rust to use the Python/Node.js bindings?

For **pre-built wheels** (pip install / npm install): No.
For **building from source**: Yes, you need Rust + Cargo.

### Is the C API thread-safe?

Yes! The C API wraps `ConcurrentPulseMap` internally. You can call `pulse_map_insert()` from multiple threads simultaneously.
