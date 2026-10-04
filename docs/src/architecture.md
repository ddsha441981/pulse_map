# Architecture & Internals

| File | Responsibility |
|---|---|
| `lib.rs` | Raw alias, typed wrapper, serialization traits, entry API |
| `raw.rs` | Fixed-size byte cache, TTL, local eviction |
| `sync.rs` | Independently implemented concurrent operations, locks, growth |
| `sharded.rs` | 16-way routing and per-shard map ownership |
| `engine/meta.rs` | AtomicU64 packed state/H2/priority |
| `engine/access_buffer.rs` | Bounded, sequence-tagged lossy deferred access queue |
| `engine/slot.rs`, `bucket.rs` | 14-byte slot, 64-byte aligned four-slot bucket |
| `engine/hash.rs` | WyHash and fingerprint decomposition |
| `engine/slab.rs` | Heap payload ownership and free-list index reuse |
| `iter.rs`, `traits.rs` | Iterators, Debug/Display/Extend/`From<HashMap>` |

## Bucket

```rust
use pulse_map::{Bucket, MetaWord, Slot};
assert_eq!(core::mem::size_of::<MetaWord>(), 8);
assert_eq!(core::mem::size_of::<Slot>(), 14);
assert_eq!(core::mem::size_of::<Bucket>(), 64);
assert_eq!(core::mem::align_of::<Bucket>(), 64);
```

Metadata selects fingerprint candidates before full key comparison. Inline slots
own small bytes; slab slots contain an index and extended fingerprint. Freed slab
indices are reusable; payload allocation/deallocation is still required for new
slab entries, and vectors can retain their high-water capacity.

## Concurrent paths

Normal CRUD takes an RwLock read guard then a bucket lock. TTL checking takes the
epochs mutex. Get's inline path avoids the pool lock; peek/remove can take it even
for inline candidates. Slab reads need the pool lock. Owned value bytes are copied
before deserialization. See [Concurrency](concurrency.md) for lock ordering.

Insert drains up to 64 deferred accesses before taking its own target bucket lock.
Publication/reuse sequence tags protect the ring slot until its consumer has copied
the event. A callback runs after queue ownership is released.

## Resize

Only concurrent maps grow. Under the exclusive map guard, discard buffered access
hints, extract every Full entry (including empty keys and expired occupied slots),
rehash into new bucket/slab/TTL arrays, and swap. TTL data is preserved; priorities
reset. Requests do not shrink capacity. Sharded routing removes shard bits before
bucket indexing, including during rehash.
