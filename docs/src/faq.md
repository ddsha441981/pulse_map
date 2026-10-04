# FAQ

### Is this a HashMap replacement?

It is a bounded **entry-count cache**, not a lossless map. A full four-slot bucket
evicts locally before global capacity is reached. Slab payload sizes can grow memory
usage even with fixed entry count. Optional auto-resize on concurrent maps removes
the fixed-capacity bound.

### Is every operation one cache miss or allocation-free?

No. Bucket metadata is co-located with slots, but TTL and slab reads touch additional
storage. Raw/typed inline operations avoid allocation after construction. Concurrent
get/peek currently copy bytes into a temporary vector, even on inline hits.

### Is it faster than LRU, QuickCache, Moka or HashMap?

Results depend on payload, occupancy, skew, hit rate and threads. HashMap has no
eviction, so it is not an equivalent bounded-cache competitor. See the versioned
[benchmarks](benchmarks.md) and compare actual resident counts as well as throughput.

### Does expiry remove entries immediately?

No. Get/peek hide expired entries, while len and iter still count/include their
occupied slots. TTL counts inserts, not wall-clock time. `u64::MAX` disables expiry
for an entry but does not protect it from eviction.

### Can I call it from async code?

Yes, as a synchronous operation that can block on locks and allocation. Reads use
RwLock, bucket spinlock, and shared metadata/pool mutexes. A get followed by insert
is not an atomic counter update. See [Concurrency](concurrency.md).

### Does sharding remove same-key contention?

No. A key always maps to one shard and bucket. Sharding spreads unrelated keys;
hot single-key traffic still contends. Resizing blocks the affected shard.

### How much memory should I allocate?

Raw/typed: 128 B/bucket for bucket + TTL records, plus payload slab/allocator costs.
Concurrent/sharded add locks and an access queue per map/shard. Inline key ≤6 bytes
AND value ≤7 bytes. See [Embedded](embedded-no-std.md) and the measured RSS tables.

### Are no_std and bindings supported?

Raw/typed maps support no_std with alloc and a suitable portable-atomic fallback.
C/Python/Java/Node bindings are maintained in a separate repository with their own
versions and lifetime/threading contracts: [Bindings](ffi-bindings.md).
