# Entry API

TypedPulseMap provides occupied/vacant entries through an exclusive mutable borrow.
Unlike std HashMap's entry API, `or_insert`/`or_insert_with` return unit, not `&mut V`.

```rust
use pulse_map::TypedPulseMap;
let mut map = TypedPulseMap::<u32,u32>::new(16);
map.entry(1).or_insert(10);
map.entry(1).and_modify(|v| *v += 1).or_insert(0);
assert_eq!(map.get(&1), Some(11));
map.entry(2).or_insert_with(|| 20);
assert_eq!(map.get(&2), Some(20));
```

OccupiedEntry exposes get/key/insert/remove; VacantEntry exposes key/insert.
Writing a modified value re-inserts it, refreshing its epoch and using the default
TTL. Entry lookup treats expired values as vacant.

Concurrent/sharded maps offer upsert through insert, but no atomic read-modify-write
entry API. A separate get followed by insert can lose concurrent updates.
