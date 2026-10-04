# Concurrency Model

`ConcurrentPulseMap` shares a map with `&self` operations; `ShardedPulseMap` routes
operations to 16 independent concurrent maps. Both require `std`.

## Locks

1. RwLock read guard for normal operations; exclusive write guard for resize.
2. Per-bucket spinlock for **get, peek, insert and remove** on that bucket.
3. Epochs mutex for TTL metadata, including hit lookups with global TTL disabled.
4. Slab mutex for large entries. Inline get avoids this mutex, but not the others.

Different buckets can still contend on shared metadata/pool locks. Sharding narrows
that contention to a shard. Same-key operations still route to the same shard/bucket.
Lock waits and allocation mean these synchronous operations can block an async
executor; no universal non-blocking/ISR-latency guarantee is provided.

## Deferred priority updates

Get records bucket/slot access events rather than updating priority inline. Inserts
drain up to 64 events before taking their own target bucket lock. Each callback
takes one target bucket lock and releases it before the next event. Guards are not
nested. The queue releases its slot before calling out.

The bounded MPMC buffer uses per-slot sequence numbers for publication/reuse and
one reservation CAS attempt per push/pop. A full buffer, contention, or an unpublished
head can cause tracking to be skipped. A stalled producer can delay consumption:
try operations do not wait, but this is not a formally lock-free FIFO. Sequence
tagging prevents duplicate delivery; lossy tracking remains an approximation.

Resize holds the exclusive map lock, clears queued old-layout events, migrates keys,
values and TTL, and swaps the arrays. Policy history resets during rehash. Sharded
resize processes one shard at a time; other shards can proceed.

Raw/typed maps have no shared concurrent writer API. Raw is Send, not Sync; use a
concurrent map or application-level synchronization for shared mutation.
