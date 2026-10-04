# Insertion-Epoch TTL

TTL counts insertions, including updates. It does not count seconds or reads.
Entries expire when `current_epoch.wrapping_sub(insertion_epoch) > effective_ttl`.
At age equal to TTL they remain valid. Shards count their own inserts independently.

| Setting | Meaning |
|---|---|
| `set_ttl(0)` | Disable global expiry (default) |
| `insert_ttl(k,v,0)` | Use the current global default |
| `insert_ttl(k,v,N)` | Override with N insertion epochs |
| `insert_ttl(k,v,u64::MAX)` | Never expire; can still be evicted |

```rust
use pulse_map::TypedPulseMap;
let mut cache = TypedPulseMap::<u32, u32>::new(16);
cache.set_ttl(100);
cache.insert_ttl(1, 10, 1);
cache.insert(2, 20);
assert_eq!(cache.peek(&1), Some(10)); // age == ttl
cache.insert(2, 21);
assert_eq!(cache.get(&1), None);     // age > ttl
assert_eq!(cache.len(), 2);          // occupied, not unexpired count
assert!(cache.iter().any(|(k, _)| k == 1));
```

Reinsert refreshes epoch and replaces the TTL override. `get` does neither.
Changing global TTL changes the effective TTL of entries with override zero.
Expiry is a lookup decision, not a permanent deletion; disabling/extending TTL can
make an unreclaimed entry visible again. `remove` physically removes even expired
entries and reports whether an occupied matching key was found.

Expired slots are not eagerly removed. Raw insertion can reuse them; concurrent
insertion does not currently prefer them over policy victims. `len()` includes
expired occupied entries; `iter()` includes them too. No wall-clock session/DNS
lifetime can be guaranteed by converting an estimated request rate into epochs.

TTL metadata occupies 16 B/slot even when expiry is disabled. Concurrent hits take
the epochs mutex before checking effective TTL, so disabled TTL is not zero overhead.
