# TTL — Automatic Expiry

> Global TTL added in **v0.6.0** · Per-entry TTL added in **v0.6.1**

PulseMap supports **insertion-epoch TTL** — entries automatically expire after a fixed number of insertions. No background thread, no timer, zero overhead when disabled.

---

## How It Works

Every insert bumps a monotonic `current_epoch: u32` counter. Each slot stores the epoch at which its entry was inserted. On every `get()` or `peek()`:

```
age = current_epoch - slot_epoch
if age > effective_ttl → entry is expired → return None
```

This is a **single wrapping subtraction + comparison** — effectively free on modern CPUs.

---

## Quick Start — Global TTL

```rust
use pulse_map::PulseMap;

let mut cache = PulseMap::new(1024);

// Set TTL: entries expire after 500 insertions
cache.set_ttl(500);

cache.insert(b"session:abc", b"user_data");  // epoch 1

// High-traffic server: 500 more inserts
for i in 0u32..501 {
    cache.insert(&i.to_le_bytes(), b"traffic");
}

// session:abc was inserted at epoch 1
// current_epoch is now 502, age = 501 > ttl = 500
assert_eq!(cache.get(b"session:abc"), None);  // expired ✓
```

---

## Per-Entry TTL (v0.6.1+)

Individual entries can have their own TTL, overriding the global default:

```rust
use pulse_map::PulseMap;

let mut cache = PulseMap::new(1024);
cache.set_ttl(500); // global default

// Per-entry overrides
cache.insert_ttl(b"session", b"data", 50);      // expires after 50 inserts
cache.insert_ttl(b"config", b"val", u32::MAX);  // never expires
cache.insert(b"normal", b"val");                 // uses global TTL = 500
```

### TTL Parameter Semantics

| `ttl` value | Behavior |
|:-----------:|----------|
| `0` | Use global default (`set_ttl()`) |
| `1..u32::MAX-1` | Expire after N insertions |
| `u32::MAX` | **Never expire** — entry lives forever |

### Available On All Map Types

```rust
// PulseMap (raw bytes)
map.insert_ttl(b"key", b"val", 100);

// TypedPulseMap<K, V>
map.insert_ttl(42u32, 100u64, 50);

// ConcurrentPulseMap<K, V> (thread-safe)
map.insert_ttl(42u32, 100u64, 50);

// ShardedPulseMap<K, V> (16-shard)
map.insert_ttl(42u32, 100u64, 50);
```

### Per-Entry Overrides Global

```rust
let mut map = PulseMap::new(64);
map.set_ttl(100); // global: 100 inserts

// Per-entry TTL = 2 (overrides global 100)
map.insert_ttl(b"short", b"val", 2);

// After 3 more inserts → short expired (age 3 > ttl 2)
// Even though global TTL is 100
```

---

## API

### PulseMap (raw `&[u8]`)

```rust
map.set_ttl(n: u32)              // global TTL (0 = disabled)
map.get_ttl() -> u32             // current global TTL
map.current_epoch() -> u32       // total insertions
map.insert(key, value)           // uses global TTL
map.insert_ttl(key, value, ttl)  // per-entry TTL override
```

### TypedPulseMap\<K, V\>

```rust
map.set_ttl(500u32);
map.insert_ttl(key, value, 50);  // per-entry TTL
map.get_ttl()                    // → 500
map.current_epoch()              // → total inserts
```

---

## TTL = 0 → Disabled (Default)

By default, `default_ttl = 0`. The expiry check returns `false` immediately when both global and per-entry TTL are 0 — **zero overhead**.

**Backward compatible** — existing code works without any changes.

---

## Update Refreshes Epoch

Re-inserting the same key resets its epoch and TTL:

```rust
cache.set_ttl(3);

cache.insert_ttl(b"key", b"v1", 3);  // epoch 1, TTL=3
cache.insert(b"a", b"x");             // epoch 2
cache.insert(b"b", b"y");             // epoch 3

// Re-insert refreshes both epoch AND TTL
cache.insert_ttl(b"key", b"v2", 3);  // epoch 4, TTL=3 (refreshed!)

cache.insert(b"c", b"z");             // epoch 5 → key age = 1 (alive)
assert_eq!(cache.get(b"key"), Some(&b"v2"[..]));
```

This is useful for **session stores** — each access or heartbeat refreshes the TTL.

---

## Lazy Eviction

Expired slots are **not eagerly removed**. They are reclaimed lazily:

1. `get()` / `peek()` — returns `None` for expired entries (no cleanup)
2. `insert()` — when searching for a free slot, expired slots are treated as available

This means **no background thread, no periodic scan, no latency spikes**.

---

## Choosing a TTL Value

TTL is measured in **insertions**, not wall-clock time. To convert:

```
ttl_epochs = expected_inserts_per_second × desired_ttl_seconds

Example:
  Server: 10,000 inserts/sec
  Desired TTL: 60 seconds
  → set_ttl(600_000)
```

This makes TTL workload-proportional — a busier server expires entries faster.

---

## Comparison with Redis TTL

| Feature | Redis TTL | PulseMap TTL |
|---------|:---------:|:------------:|
| Time unit | Seconds / milliseconds | Insertions |
| Per-entry TTL | ✅ Yes | ✅ Yes (v0.6.1+) |
| Background expiry | ✅ Yes | ❌ Lazy only |
| Refresh on access | Manual (`EXPIRE` cmd) | Re-insert |
| Network hop | ~100μs | 0 (in-process) |
| Memory bound | No | ✅ Fixed |

> PulseMap TTL is ideal for **in-process** hot caches. Use Redis when you need cross-process TTL or millisecond precision.

---

## Implementation Details

```rust
// PulseMapRaw fields (raw.rs) — v0.6.1
#[derive(Clone, Copy, Default)]
pub(crate) struct SlotTTL {
    epoch: u32,  // insertion epoch
    ttl: u32,    // 0 = use default, u32::MAX = never
}

slots_ttl: Vec<SlotTTL>,  // one per slot
current_epoch: u32,        // global counter
default_ttl: u32,          // set via set_ttl()

// On every insert:
self.current_epoch = self.current_epoch.wrapping_add(1);
self.slots_ttl[idx] = SlotTTL { epoch: self.current_epoch, ttl };

// On get():
fn is_expired(&self, bucket_idx: usize, slot_idx: u8) -> bool {
    let entry = self.slots_ttl[bucket_idx * 4 + slot_idx as usize];
    let effective_ttl = if entry.ttl == 0 { self.default_ttl } else { entry.ttl };
    if effective_ttl == 0 || effective_ttl == u32::MAX { return false; }
    self.current_epoch.wrapping_sub(entry.epoch) > effective_ttl
}
```

Memory overhead: `num_buckets × 4 × 8 bytes` (one `SlotTTL` per slot).
For 1024 buckets: `1024 × 4 × 8 = 32 KB` — negligible.
