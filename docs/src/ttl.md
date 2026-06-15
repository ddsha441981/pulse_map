# TTL — Automatic Expiry

> Added in **v0.6.0**

PulseMap supports **insertion-epoch TTL** — entries automatically expire after a fixed number of insertions. No background thread, no timer, zero overhead when disabled.

---

## How It Works

Every insert bumps a monotonic `current_epoch: u32` counter. Each slot stores the epoch at which its entry was inserted (`slot_epoch`). On every `get()` or `peek()`, if TTL is enabled:

```
age = current_epoch - slot_epoch
if age > ttl_epochs → entry is expired → return None
```

This is a **single wrapping subtraction + comparison** — effectively free on modern CPUs.

---

## Quick Start

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

## API

### PulseMap (raw `&[u8]`)

```rust
map.set_ttl(n: u32)      // set TTL in insertion epochs (0 = disabled)
map.get_ttl() -> u32     // query current TTL setting
map.current_epoch() -> u32  // total insertions so far
```

### TypedPulseMap<K, V>

```rust
map.set_ttl(500u32);
map.get_ttl()       // → 500
map.current_epoch() // → total inserts
```

---

## TTL = 0 → Disabled (Default)

By default, `ttl_epochs = 0`. The expiry check is:

```rust
if self.ttl_epochs == 0 {
    return false;  // short-circuit: zero overhead
}
```

**Backward compatible** — existing code works without any changes.

---

## Update Refreshes Epoch

Re-inserting the same key resets its epoch to `current_epoch`:

```rust
cache.set_ttl(3);

cache.insert(b"key", b"v1");  // epoch 1
cache.insert(b"a",   b"x");   // epoch 2
cache.insert(b"b",   b"y");   // epoch 3

// key's age is now 2, about to expire...
// Re-insert before it expires:
cache.insert(b"key", b"v2");  // epoch 4 — refreshed!

cache.insert(b"c", b"z");     // epoch 5 → key age = 1 (alive)
assert_eq!(cache.get(b"key"), Some(&b"v2"[..]));
```

This is useful for **session stores** — each access or heartbeat refreshes the TTL.

---

## Lazy Eviction

Expired slots are **not eagerly removed**. They are reclaimed lazily:

1. `get()` / `peek()` — returns `None` for expired entries (no cleanup)
2. `insert()` — when searching for a free slot, expired slots are treated as available

This means **no background thread, no periodic scan, no latency spikes**.

```
Expired slot detected during insert → reuse it immediately
No extra memory cost, no timer interrupt
```

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

This makes TTL workload-proportional — a busier server expires entries faster, which is often the desired behavior for rate limiters and caches.

---

## TypedPulseMap Example

```rust
use pulse_map::TypedPulseMap;

let mut sessions = TypedPulseMap::<String, String>::new(1024);
sessions.set_ttl(10_000); // expire after 10K inserts

sessions.insert("user:42".to_string(), "token_abc".to_string());

// ... much later, after 10K+ inserts ...

assert_eq!(sessions.get(&"user:42".to_string()), None); // expired
```

---

## Epoch Counter Overflow

`current_epoch` is a `u32` — max value is `4,294,967,295` (4.3 billion inserts).

At 1 million inserts/sec, it wraps after ~71 minutes. The expiry check uses **wrapping subtraction** (`wrapping_sub`) so wrap-around is handled correctly without panicking.

---

## Comparison with Redis TTL

| Feature | Redis TTL | PulseMap TTL |
|---------|:---------:|:------------:|
| Time unit | Seconds / milliseconds | Insertions |
| Background expiry | ✅ Yes | ❌ Lazy only |
| Refresh on access | Manual (`EXPIRE` cmd) | Re-insert |
| Network hop | ~100μs | 0 (in-process) |
| Memory bound | No | ✅ Fixed |
| No GC pauses | ❌ No | ✅ Yes |

> PulseMap TTL is ideal for **in-process** hot caches. Use Redis when you need cross-process TTL or millisecond precision.

---

## Implementation Details

```rust
// PulseMapRaw fields (raw.rs)
epochs: Vec<u32>,      // one per slot: epochs[bucket_idx * 4 + slot_idx]
current_epoch: u32,    // global counter, wrapping
ttl_epochs: u32,       // 0 = disabled

// On every insert:
self.current_epoch = self.current_epoch.wrapping_add(1);
self.epochs[bucket_idx * 4 + slot_idx] = self.current_epoch;

// On get():
fn is_expired(&self, bucket_idx: usize, slot_idx: u8) -> bool {
    if self.ttl_epochs == 0 { return false; }
    let slot_epoch = self.epochs[bucket_idx * 4 + slot_idx as usize];
    self.current_epoch.wrapping_sub(slot_epoch) > self.ttl_epochs
}
```

Memory overhead: `num_buckets × 4 × 4 bytes` (one `u32` per slot).
For 1024 buckets: `1024 × 4 × 4 = 16 KB` — negligible.
