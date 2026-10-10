# API Reference

| Type | Storage API | Threading |
|---|---|---|
| `PulseMap` (alias of `PulseMapRaw`) | `&[u8]` keys/values; borrowed lookup result | Send, not Sync |
| `TypedPulseMap<K,V>` | Owned typed insert/result; iter/entry APIs | Single-owner mutation |
| `ConcurrentPulseMap<K,V>` | Owned typed values; methods take `&self` | Internally locked |
| `ShardedPulseMap<K,V>` | Concurrent API across 16 shards | Internally locked per shard |

`PulseKey: Sized` and `PulseValue: Sized` provide associated `Bytes: AsRef<[u8]>`,
`to_bytes(&self)` and `from_bytes(&[u8]) -> Option<Self>`. PulseKey also provides
`with_key_bytes`, borrowing key bytes for reads where possible. It must encode the
same bytes as `to_bytes`.

Both traits: u8/u16/u32/u64/i32/i64/String/`Vec<u8>`. Arrays implement PulseKey only;
bool implements PulseValue only. Custom implementations define their own encoding.

All maps: new, insert, insert_ttl, get, peek, remove, len, is_empty, capacity,
load_factor, eviction_count, set_ttl, get_ttl, current_epoch.

- `contains_key`: typed/concurrent/sharded; raw uses `peek(key).is_some()`.
- `iter`, `entry`, Extend: typed. Raw iteration uses `RawIter::new(&map)`.
- `num_buckets`: raw and Concurrent; sharded capacity aggregates all shards.
- `with_auto_resize`: Concurrent and Sharded only.
- `resize`: Concurrent; `resize_all`: Sharded (growth only).

`len()` means occupied slots, including expired entries. See individual pages and
[TTL](ttl.md) for lazy visibility. All insertion APIs may evict.
