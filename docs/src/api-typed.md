# TypedPulseMap<K,V>

A fixed-size wrapper around raw storage using PulseKey/PulseValue encodings.
Lookups deserialize owned values. No `Index` or reference-to-V API is provided.

```rust
use pulse_map::TypedPulseMap;
let mut map = TypedPulseMap::<u32, u64>::new(16);
map.insert(42, 100);
assert_eq!(map.get(&42), Some(100));
assert!(map.contains_key(&42));
map.extend([(1, 10), (2, 20)]);
assert!(map.iter().any(|(k,v)| k == 42 && v == 100));
assert!(map.remove(&42));
```

This example's u64 value uses slab storage; choose u32/u32 for inline mode. Keys
must fit 6 bytes and values 7 bytes to avoid slab allocation. Strings/Vec values
also allocate when deserialized.

Stats: len, is_empty, capacity, load_factor, eviction_count. There is no typed
`num_buckets` or `with_auto_resize`. `iter()` includes expired occupied entries and
skips values that fail custom deserialization. `contains_key()` tests raw presence
with expiry, without decoding V.

`From<HashMap>` chooses a bucket count then inserts; collisions can evict entries
during conversion. It is not a lossless HashMap conversion guarantee.
