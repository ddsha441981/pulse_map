# ShardedPulseMap

Sixteen independent ConcurrentPulseMaps. Constructor argument is buckets **per shard**.
Each shard has its own locks, slab, access buffer, count and insertion epoch.

```rust
use pulse_map::ShardedPulseMap;
let map = ShardedPulseMap::<u32,u32>::new(16);
assert_eq!(map.capacity(), 16 * 16 * 4);
map.insert_ttl(1, 10, u64::MAX);
map.resize_all(32);
assert_eq!(map.peek(&1), Some(10));
```

## Routing

```text
shard = (hash >> 14) & 15
bucket_hash = (hash & 0x3fff) | ((hash >> 4) & !0x3fff)
bucket = bucket_hash & (buckets_per_shard - 1)
```

The shard's four bits are removed before bucket indexing, preserving low-14-bit
distribution while allowing larger shards to use independent hash bits. All CRUD
and rehash operations use this same derivation. H2 uses the original hash's high
seven bits. Older versions failed to remove shard bits at larger capacities.

`resize_all` grows one shard at a time. Other shards remain usable; operations on
the shard being migrated block. It is not a globally atomic snapshot operation.

TTL counts inserts into the entry's own shard. `current_epoch()` is the maximum
shard epoch, not total map-wide inserts. `len()` sums occupied slots; all aggregated
stats are approximate snapshots under concurrent mutations.
