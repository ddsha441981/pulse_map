# Use Cases

> **💡 Use PulseMap anywhere you'd use HashMap but can't afford unbounded memory growth.**

## DNS Cache

```rust
use pulse_map::ShardedPulseMap;

let dns_cache = ShardedPulseMap::<String, String>::new(65536);

// Hot domains stay, cold domains auto-evict
dns_cache.insert("google.com".to_string(), "142.250.80.46".to_string());

// Bounded memory — won't OOM on millions of unique queries
println!("Evictions: {}", dns_cache.eviction_count());
```

**Why PulseMap:** ISPs see millions of unique domains. HashMap grows forever → OOM. ShardedPulseMap keeps the hottest records in fixed memory, with 6.5–12x better throughput than moka under concurrent load.

---

## API Rate Limiter with TTL

```rust
use pulse_map::ShardedPulseMap;
use std::sync::Arc;

let rate_limiter = Arc::new(ShardedPulseMap::<String, u64>::with_auto_resize(4096));
rate_limiter.set_ttl(100_000); // reset counts after 100K inserts

fn check_rate(limiter: &ShardedPulseMap<String, u64>, ip: &str) -> bool {
    let key = ip.to_string();
    let count = limiter.get(&key).unwrap_or(0);
    if count >= 100 {
        return false;  // rate limited
    }
    limiter.insert(key, count + 1);
    true
}
```

**Why PulseMap:** Per-IP counters in bounded memory. Old IPs auto-evict. Per-entry TTL lets short-burst IPs reset faster.

---

## CDN Edge Cache

```rust
use pulse_map::ShardedPulseMap;

let edge_cache = ShardedPulseMap::<String, Vec<u8>>::new(16384);

// Serve from cache — ~5ns lookup on cache hit
if let Some(content) = edge_cache.get(&url) {
    return content;
}

// Cache miss — fetch from origin
let content = fetch_origin(&url);
edge_cache.insert(url, content);
```

**Why PulseMap:** Hot content stays in L1 (64-byte cache line). Cold content evicts automatically. No GC pauses — critical for sub-millisecond edge latency.

---

## Session Store with Per-Entry TTL

```rust
use pulse_map::ShardedPulseMap;

let sessions = ShardedPulseMap::<String, String>::with_auto_resize(8192);
sessions.set_ttl(500_000); // global default: 500K inserts

// Premium users: longer TTL
sessions.insert_ttl("premium:abc".to_string(), user_json, 2_000_000);

// Regular users: global default
sessions.insert("user:xyz".to_string(), user_json);

// Admin tokens: never expire
sessions.insert_ttl("admin:root".to_string(), token, u32::MAX);
```

**Why PulseMap:** Per-entry TTL means different session policies without needing a separate cache per tier. No background cleanup thread needed.

---

## Game Asset Cache

```rust
use pulse_map::ShardedPulseMap;

let texture_cache = ShardedPulseMap::<String, u64>::new(2048);

// Cache texture GPU handles — fixed VRAM budget
texture_cache.insert("hero_idle.png".to_string(), gpu_handle);

// When full, least-used textures auto-evict
println!("Evictions: {}", texture_cache.eviction_count());
```

**Why PulseMap:** Fixed memory = no frame drops from GC. Eviction metadata embedded in cache line = zero extra cost.

---

## Log Deduplication

```rust
use pulse_map::ConcurrentPulseMap;

let seen_logs = ConcurrentPulseMap::<u64, u8>::new(32768);

fn should_log(seen: &ConcurrentPulseMap<u64, u8>, hash: u64) -> bool {
    if seen.contains_key(&hash) {
        return false;  // duplicate — skip
    }
    seen.insert(hash, 1);
    true
}
```

**Why PulseMap:** Dedup window is bounded. Old hashes auto-evict. Zero allocations during hot path.

---

## Database Query Cache

```rust
use pulse_map::ShardedPulseMap;

let query_cache = ShardedPulseMap::<String, String>::new(4096);

fn cached_query(cache: &ShardedPulseMap<String, String>, sql: &str) -> String {
    let key = sql.to_string();
    if let Some(result) = cache.get(&key) {
        return result;  // cache hit — ~5ns
    }
    let result = execute_sql(sql);  // cache miss — ~1ms
    cache.insert(key, result.clone());
    result
}
```

**Why PulseMap:** Hot queries stay cached. Cold queries evict. 8-thread query dispatchers benefit from ShardedPulseMap's near-zero lock contention.

---

## Choosing the Right Map per Use Case

| Use Case | Recommended | Reason |
|----------|:-----------:|--------|
| DNS cache (multi-core) | `ShardedPulseMap` | High concurrent insert rate |
| Rate limiter (API server) | `ShardedPulseMap` | Per-IP TTL + concurrent access |
| Single-thread parser | `TypedPulseMap` | No locking overhead |
| Game assets | `ShardedPulseMap` | Multi-thread asset streaming |
| FFI / C interop | `PulseMapRaw` | Raw byte API |
