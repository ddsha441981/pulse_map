# Eviction Strategy

PulseMap uses a **hybrid LFU+LRU eviction** policy that requires **zero additional cache misses** — all eviction metadata is embedded in the 8-byte MetaWord of each bucket.

## How Eviction Works

When all 4 slots in a bucket are full and a new entry hashes to that bucket:

1. **Calculate eviction score** for each slot
2. **Evict the slot with the lowest score**
3. **Insert the new entry** in the freed slot

### Eviction Score Formula

```
score(slot) = lfu_count(slot) + recency(slot) × 2
```

- **LFU count** (4 bits, range 0-15): How many times this entry was accessed
- **Recency** (3 bits, range 0-7): How recently this entry was accessed relative to siblings

The slot with the **minimum score** is evicted.

## MetaWord Layout (8 bytes)

```
Bit Layout (64 bits):
┌──────────────────────────────────────────────────────────────┐
│ Slot 3          │ Slot 2          │ Slot 1          │ Slot 0 │
├─────────────────┼─────────────────┼─────────────────┼────────┤
│ st│h2   │freq│rec│ st│h2   │freq│rec│ st│h2   │freq│rec│st│h2..│
│ 2b│7b   │4b  │3b │ 2b│7b   │4b  │3b │ 2b│7b   │4b  │3b │2b│7b. │
└──────────────────────────────────────────────────────────────┘

st  = Slot State (2 bits): Empty(0), Full(1), Deleted(2), Tombstone(3)
h2   = H2 Fingerprint (7 bits): Fast hash match filter
freq = Frequency Counter (4 bits): Access count (0-15)
rec  = LRU Recency (3 bits): Relative age (0=oldest, 7=newest)
```

## Eviction Behavior

### Frequency Dominates

Frequently accessed entries survive eviction even if they haven't been accessed recently:

```
Slot 0: freq=15, recency=0 → score = 15 + 0 = 15  (survives!)
Slot 1: freq=1,  recency=7 → score = 1 + 14 = 15  (tied)
Slot 2: freq=0,  recency=1 → score = 0 + 2 = 2    (EVICTED)
Slot 3: freq=5,  recency=4 → score = 5 + 8 = 13   (survives!)
```

### Cold Start

New entries start with `freq=0, recency=7` (newest). They must earn frequency to survive.

### Frequency Saturation

LFU counter saturates at 15 (4 bits). This prevents long-lived entries from becoming permanently sticky — a recently-inserted entry with moderate access can still compete.

## Eviction Statistics

```rust
let map = ConcurrentPulseMap::<String, String>::new(64);

// Fill beyond capacity
for i in 0..1000 {
    map.insert(format!("key_{}", i), format!("val_{}", i));
}

println!("Evictions: {}", map.eviction_count());
// Will show evictions once capacity (256) is exceeded
```

## Comparison with Other Policies

| Policy | Hit Rate | Overhead | Cache Misses |
|--------|:--------:|:--------:|:------------:|
| **PulseMap (LFU+LRU)** | ★★★★ | 7 bits/slot | **0 extra** |
| LRU (linked list) | ★★★ | 16 bytes/entry | 2-3 |
| LFU (heap) | ★★★★ | 8+ bytes/entry | 3-4 |
| FIFO | ★★ | 0 | 0 |
| Random | ★ | 0 | 0 |

PulseMap achieves **near-LFU hit rates** with **FIFO-level overhead**.

## Tuning

PulseMap's eviction is **not configurable** by design. The 4-bit LFU + 3-bit LRU hybrid was chosen after extensive benchmarking as the optimal tradeoff for 4-slot buckets.

If you need different eviction behavior:
- **More capacity instead of better eviction** → Use auto-resize: `with_auto_resize(n)`
- **No eviction at all** → Use auto-resize with large initial size
- **TTL-based expiration** → Use `set_ttl(n)` (global) or `insert_ttl(k, v, n)` (per-entry)
- **Permanent entries** → Use `insert_ttl(key, val, u32::MAX)` — never expire
