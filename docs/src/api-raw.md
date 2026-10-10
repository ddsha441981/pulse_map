# PulseMapRaw

`PulseMap` is its backwards-compatible alias. Keys/values are byte slices copied
into inline/slab storage. Get/peek borrow value bytes from the map.

```rust
use pulse_map::{PulseMap, RawIter};
let mut map = PulseMap::new(16);
map.insert(b"hello", b"world");
assert_eq!(map.get(b"hello"), Some(&b"world"[..]));
assert!(map.peek(b"hello").is_some());
assert_eq!(RawIter::new(&map).count(), 1);
assert!(map.remove(b"hello"));
assert!(map.is_empty());
```

Fixed-size; no auto-resize or raw `contains_key` method. Empty keys and values are
valid. Get updates priority, peek does not. Neither refreshes TTL.

`set_ttl`/`insert_ttl` use u64 insertion epochs. `len`/iteration include expired
occupied slots; `get`/`peek` hide them. Raw insertion can reclaim expired slots.
See [TTL](ttl.md) and [Core Concepts](core-concepts.md).
