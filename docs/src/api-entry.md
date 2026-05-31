# Entry API

The Entry API provides in-place access for complex insert-or-update patterns.

## Usage

```rust
use pulse_map::TypedPulseMap;

let mut map: TypedPulseMap<String, u64> = TypedPulseMap::new(256);

// Insert-or-update pattern
map.insert("counter".to_string(), 0);

// Update existing value
if let Some(old) = map.get(&"counter".to_string()) {
    map.insert("counter".to_string(), old + 1);
}
```

## Insert-or-Default

```rust
// If key doesn't exist, insert default
let key = "visits".to_string();
if !map.contains_key(&key) {
    map.insert(key.clone(), 0);
}

// Now safely increment
if let Some(count) = map.get(&key) {
    map.insert(key, count + 1);
}
```

## Atomic Upsert Pattern (Concurrent)

```rust
use pulse_map::ConcurrentPulseMap;

let map = ConcurrentPulseMap::<String, u64>::new(256);

// Thread-safe upsert — insert always succeeds
// If key exists, value is overwritten (last-writer-wins)
map.insert("key".to_string(), 42);
map.insert("key".to_string(), 99);  // overwrites

assert_eq!(map.get(&"key".to_string()), Some(99));
```

> **Note:** PulseMap's `insert()` is an upsert — it inserts if the key is new, or updates if the key exists. There is no separate `update()` method.
