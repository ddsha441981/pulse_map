# TypedPulseMap<K, V>

Type-safe single-threaded map. Works with any type implementing `PulseKey` and `PulseValue`.

## Construction

```rust
use pulse_map::TypedPulseMap;

// String → String cache
let mut cache: TypedPulseMap<String, String> = TypedPulseMap::new(256);

// u64 → u64 counter store
let mut counters: TypedPulseMap<u64, u64> = TypedPulseMap::new(1024);

// With auto-resize
let mut cache: TypedPulseMap<String, Vec<u8>> = TypedPulseMap::with_auto_resize(64);
```

## CRUD Operations

```rust
// Insert
cache.insert("session_abc".to_string(), "user_data_json".to_string());

// Get — returns owned Option<V>
let val: Option<String> = cache.get(&"session_abc".to_string());

// Peek — like get() but doesn't update eviction priority
let val: Option<String> = cache.peek(&"session_abc".to_string());

// Contains
let exists: bool = cache.contains_key(&"session_abc".to_string());

// Remove
let removed: bool = cache.remove(&"session_abc".to_string());
```

## Numeric Keys

```rust
let mut map: TypedPulseMap<u32, u64> = TypedPulseMap::new(256);

map.insert(42, 100);
map.insert(1337, 9001);

assert_eq!(map.get(&42), Some(100));
```

## Stats

```rust
println!("Entries:     {}", cache.len());
println!("Empty:       {}", cache.is_empty());
println!("Capacity:    {}", cache.capacity());
println!("Load Factor: {:.1}%", cache.load_factor() * 100.0);
println!("Evictions:   {}", cache.eviction_count());
println!("Buckets:     {}", cache.num_buckets());
```

## Performance Tips

1. **Use small keys** (≤ 6 bytes) when possible — they stay inline (no heap allocation)
2. **Use small values** (≤ 7 bytes) when possible — same reason
3. **Pre-size correctly** — avoid auto-resize overhead for known workloads
4. **Use `peek()` for read-heavy** paths where you don't want to affect eviction priority
