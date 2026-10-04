# ConcurrentPulseMap

Thread-safe owned typed operations with `&self`. Share using Arc; get/peek take
internal locks and copy value bytes before deserialization.

```rust
use pulse_map::ConcurrentPulseMap;
use std::sync::Arc;
let map = Arc::new(ConcurrentPulseMap::<String, u32>::new(16));
let writer = map.clone();
std::thread::spawn(move || writer.insert(String::new(), 42)).join().unwrap();
map.resize(32);
assert_eq!(map.get(&String::new()), Some(42));
assert!(map.remove(&String::new()));
```

New takes bucket count. `with_auto_resize(n)` enables growth above 75% occupied
slots. Local collision eviction can still happen before that threshold.

Manual `resize(n)` rounds to a power of two, only grows, and blocks all operations
while migrating resident entries, including empty keys and their TTL metadata.
Buffered old-layout access hints and eviction priorities are reset during resize.

`len()`/`eviction_count()` read atomic counters. `capacity()`/`num_buckets()` acquire
the map read lock. Counts across concurrent operations are not a consistent snapshot.

Get+insert is **not an atomic read-modify-write**. Concurrent counters/rate limiters
need additional application synchronization or a purpose-built atomic-update API.
