# Use Cases

> **💡 Use PulseMap anywhere you'd use HashMap but can't afford unbounded memory growth.**

## DNS Cache

```rust
let dns_cache = ConcurrentPulseMap::<String, String>::new(65536);

// Hot domains stay, cold domains auto-evict
dns_cache.insert("google.com".to_string(), "142.250.80.46".to_string());

// Bounded memory — won't OOM on millions of unique queries
```

**Why PulseMap:** ISPs see millions of unique domains. HashMap grows forever → OOM. PulseMap keeps the hottest records in fixed memory.

## API Rate Limiter

```rust
let rate_limiter = ConcurrentPulseMap::<String, u64>::with_auto_resize(4096);

fn check_rate(ip: &str) -> bool {
    let key = ip.to_string();
    let count = rate_limiter.get(&key).unwrap_or(0);
    if count >= 100 {
        return false;  // rate limited
    }
    rate_limiter.insert(key, count + 1);
    true
}
```

**Why PulseMap:** Thousands of IPs hit your API. PulseMap tracks per-IP counters in bounded memory. Old IPs auto-evict.

## CDN Edge Cache

```rust
let edge_cache = ConcurrentPulseMap::<String, Vec<u8>>::new(16384);

// Serve from cache — ~5ns lookup
if let Some(content) = edge_cache.get(&url) {
    return content;
}

// Cache miss — fetch from origin
let content = fetch_origin(url);
edge_cache.insert(url, content);
```

**Why PulseMap:** Hot content stays in L1 (64-byte cache line). Cold content evicts automatically. No GC pauses.

## Game Asset Cache

```rust
let texture_cache = ConcurrentPulseMap::<String, u64>::new(2048);

// Cache texture IDs — fixed VRAM budget
texture_cache.insert("hero_idle.png".to_string(), gpu_handle);

// When VRAM is full, least-used textures evict
println!("Evictions: {}", texture_cache.eviction_count());
```

**Why PulseMap:** Fixed memory = no frame drops from GC. Eviction metadata is free (embedded in cache line).

## Session Store

```rust
let sessions = ConcurrentPulseMap::<String, String>::with_auto_resize(8192);

// Store session data
sessions.insert(session_id, user_json);

// Auto-evict idle sessions — no cleanup thread needed
```

**Why PulseMap:** Sessions auto-evict when inactive. No background cleanup. No memory leak.

## Log Deduplication

```rust
let seen_logs = ConcurrentPulseMap::<u64, u8>::new(32768);

fn should_log(hash: u64) -> bool {
    if seen_logs.contains_key(&hash) {
        return false;  // duplicate — skip
    }
    seen_logs.insert(hash, 1);
    true
}
```

**Why PulseMap:** Dedup window is bounded. Old hashes evict automatically.

## Database Query Cache

```rust
let query_cache = ConcurrentPulseMap::<String, String>::new(4096);

fn cached_query(sql: &str) -> String {
    let key = sql.to_string();
    if let Some(result) = query_cache.get(&key) {
        return result;  // cache hit — ~5ns
    }
    let result = execute_sql(sql);  // cache miss — ~1ms
    query_cache.insert(key, result.clone());
    result
}
```

**Why PulseMap:** Hot queries stay cached. Cold queries evict. Fixed memory = predictable DB performance.
