# Use Cases

## Compact memoization

Use a typed cache when recomputing a missing result is acceptable. u32/u32 stays
inline and does not allocate during insert/get/remove after map construction.

```rust
use pulse_map::TypedPulseMap;
fn squared(cache: &mut TypedPulseMap<u32,u32>, key: u32) -> u32 {
    if let Some(value) = cache.get(&key) { return value; }
    let value = key.saturating_mul(key);
    cache.insert(key, value);
    value
}
let mut cache = TypedPulseMap::new(16);
assert_eq!(squared(&mut cache, 7), 49);
```

## Shared cache-aside data

```rust
use pulse_map::ShardedPulseMap;
fn cached_query(
    cache: &ShardedPulseMap<String,String>, key: String,
    fetch: impl FnOnce(&str) -> String,
) -> String {
    if let Some(value) = cache.get(&key) { return value; }
    let value = fetch(&key);
    cache.insert(key, value.clone());
    value
}
let cache = ShardedPulseMap::new(16);
assert_eq!(cached_query(&cache, "query".into(), |_| "result".into()), "result");
```

Concurrent misses may call fetch more than once. This does not provide request
coalescing, an atomic read-modify-write, or a wall-clock freshness guarantee.
String values use the slab and owned return values allocate.

## Embedded sample cache

Small numeric sensor IDs and derived values are a good inline fit. Size the bucket
and TTL allocations together and test collision pressure. See the embedded guide
for QEMU allocation/footprint measurements; host timings do not establish ISR bounds.

## Boundaries to account for

- DNS/session lifetimes need actual time-based validation outside epoch TTL.
- A concurrent get/increment/insert rate limiter can lose updates, refreshes TTL
  on each insert, and may forget counts through eviction.
- Caching a numeric GPU handle does not release its external resource on eviction.
- Deduplication is best-effort unless your application provides atomic coordination.
- Fixed entry count does not cap bytes for arbitrarily large content.

These are cache semantics: choose application-level coordination and ownership
according to the data being cached.
