# Architecture & Internals

## Layer Architecture

```
Layer 5: sharded.rs     → ShardedPulseMap (16 × ConcurrentPulseMap, shard-per-key)
Layer 4: sync.rs        → ConcurrentPulseMap (thread-safe wrapper)
Layer 3: lib.rs         → TypedPulseMap<K,V>, PulseMap (user API)
Layer 2: raw.rs         → PulseMapRaw (hash table logic, per-entry TTL)
Layer 1: engine/        → Building blocks
           ├── meta.rs  → MetaWord (8-byte AtomicU64 eviction metadata)
           ├── access_buffer.rs → AccessBuffer (lock-free lossy ring buffer for deferred LRU/LFU)
           ├── slot.rs  → Slot (14-byte inline/slab storage)
           ├── bucket.rs→ Bucket (64-byte cache line unit)
           ├── hash.rs  → WyHash + HashResult decomposition
           └── slab.rs  → SlabPool arena allocator
```

## File Map

| File | Lines | Purpose |
|------|:-----:|---------|
| `lib.rs` | ~100 | Public API, type aliases, trait defs |
| `raw.rs` | ~330 | Core insert/get/remove/TTL/per-entry-TTL logic |
| `sync.rs` | ~600 | ConcurrentPulseMap + per-bucket spinlocks |
| `sharded.rs` | ~270 | ShardedPulseMap — 16-shard concurrent map |
| `engine/meta.rs` | ~150 | MetaWord: AtomicU64, H2, state, LFU, LRU bit packing |
| `engine/access_buffer.rs` | ~100 | AccessBuffer: lock-free lossy ring buffer for deferred LRU/LFU |
| `engine/slot.rs` | ~120 | Slot: inline/slab dual-mode storage |
| `engine/bucket.rs` | ~50 | Bucket: MetaWord + 4 Slots = 64 bytes |
| `engine/hash.rs` | ~40 | WyHash → H1, H2, ext_fp decomposition |
| `engine/slab.rs` | ~85 | Arena allocator for large KV pairs |
| `traits.rs` | ~100 | PulseKey + PulseValue trait impls |
| `iter.rs` | ~50 | RawIter, TypedIter |
| `simd.rs` | ~30 | Optional SIMD H2 matching (x86_64) |

## Data Flow: Insert

```
insert("hello", "world")
  │
  ├── 1. Serialize: key.to_bytes() → [104,101,108,108,111]
  ├── 2. Hash: WyHash64 → {h1, h2, ext_fp_hi, ext_fp}
  ├── 3. Bucket: idx = h1 & bucket_mask
  ├── 4. Lock: spinlock[idx].acquire()
  ├── 5. Match: meta.match_mask(h2) → bitmask of matching slots
  │     ├── Hit? → Update value in-place, on_access()
  │     └── Miss? → Continue to step 6
  ├── 6. Find slot:
  │     ├── Free slot? → Use it
  │     └── All full? → Evict lowest-score slot
  ├── 7. Store:
  │     ├── key≤6B && val≤7B → Inline mode
  │     └── Otherwise → Slab mode (arena alloc)
  ├── 8. Update meta: state=Full, h2, on_insert()
  └── 9. Unlock: spinlock[idx].release()
```

## Data Flow: Get

```
get("hello")
  │
  ├── 1. Serialize + Hash → {h1, h2}
  ├── 2. Bucket: idx = h1 & bucket_mask
  ├── 3. Lock: spinlock[idx].acquire()
  ├── 4. H2 filter: meta.match_mask(h2) → bitmask
  │     (rejects ~99.2% of non-matches without key comparison)
  ├── 5. For each matching slot:
  │     ├── Compare full key bytes
  │     ├── Match? → Push to AccessBuffer, return value
  │     └── No match? → Continue
  ├── 6. No match found → return None
  └── 7. Unlock: spinlock[idx].release()
```

## Bucket Alignment

```rust
#[repr(C, align(64))]  // Force 64-byte alignment
pub struct Bucket {
    pub meta: MetaWord,    // 8 bytes
    pub slots: [Slot; 4],  // 4 × 14 = 56 bytes
}                          // Total: 64 bytes = 1 cache line

// Compile-time assertion
const _: () = assert!(std::mem::size_of::<Bucket>() == 64);
```

## SlabPool Arena

For entries too large for inline storage (key > 6B or value > 7B):

```
SlabPool {
    entries: Vec<Box<SlabEntry>>
}

SlabEntry {
    key_len: u32,
    val_len: u32,
    data: *const u8,   // → heap allocation [key_bytes | value_bytes]
    layout: Layout,
}
```

**Design trade-off:** Individual slab entries are managed via a free-list reuse allocator. This is optimal for cache workloads where memory can be efficiently reused before the pool is periodically reset via resize.

## Resize Strategy

```
Auto-resize triggers when: load_factor > 0.75

1. new_buckets = current_buckets × 2
2. Allocate new Vec<Bucket> + new SlabPool
3. For each old bucket, for each Full slot:
   a. Extract key/value bytes
   b. Rehash into new bucket array
   c. Re-allocate slab if needed
4. Swap old state → new state (atomic via RwLock)
5. Old state dropped (old SlabPool freed)
```
