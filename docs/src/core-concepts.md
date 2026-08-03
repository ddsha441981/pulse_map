# Core Concepts

## The Cache-Line Problem

Modern CPUs don't read memory byte-by-byte. They load **64-byte cache lines** at a time. A traditional hash table stores metadata (hash, state) separately from data (key, value), causing **2+ cache misses per lookup**:

```
Traditional HashMap:
  1. Load metadata → cache miss #1
  2. Follow pointer to key → cache miss #2
  3. Follow pointer to value → cache miss #3
```

PulseMap solves this by packing **everything into one 64-byte cache line**:

```
PulseMap Bucket (64 bytes):
  ┌─────────────────────────────────────────────────┐
  │ MetaWord (8 bytes)                              │
  │  ├── 4× Slot state (2 bits each)                │
  │  ├── 4× H2 fingerprint (7 bits each)            │
  │  └── 4× Priority (7 bits each: freq[4]+rec[3])  │
  ├─────────────────────────────────────────────────┤
  │ Slot 0 (14 bytes) — header(1) + payload(13)    │
  │ Slot 1 (14 bytes) — header(1) + payload(13)    │
  │ Slot 2 (14 bytes) — header(1) + payload(13)    │
  │ Slot 3 (14 bytes) — header(1) + payload(13)    │
  └─────────────────────────────────────────────────┘
  Total: 8 + (4 × 14) = 64 bytes = 1 cache line ✅
```

**Result:** One cache miss per lookup, including eviction decision.

## Slot Storage Modes

Each slot has two modes, determined by the header byte's MSB:

### Inline Mode (mode=0)

For **small** key-value pairs (key ≤ 6 bytes, value ≤ 7 bytes):

```
data[0]:    header byte
              bit 7:   mode (0 = inline)
              bits 6-4: key_len (0-6)
              bits 3-1: val_len (0-7)
              bit 0:   reserved
data[1..7]:  key bytes (up to 6)
data[7..14]: value bytes (up to 7)
```

**Zero-allocation.** Everything lives inside the 14-byte `data` array.

### Slab Mode (mode=1)

For **large** key-value pairs (key > 6 bytes OR value > 7 bytes):

```
data[0]:     header byte
               bit 7:   mode (1 = slab)
               bits 6-0: ext_fp_hi (7-bit extended fingerprint)
data[1..5]:  ext_fp (32-bit extended fingerprint, LE)
data[5]:     flags (reserved)
data[6..14]: slab_ptr (u64 pointer to SlabEntry, LE)
```

The actual key+value data is stored in the **SlabPool** arena allocator.

## Hash Function

PulseMap uses **WyHash** (one of the fastest non-cryptographic hash functions):

```
Input: key bytes
  │
  ▼
WyHash64(key) → 64-bit hash
  │
  ├── H1 (upper 32 bits) → bucket index
  ├── H2 (7 bits) → fingerprint for fast rejection
  ├── ext_fp_hi (7 bits) → extended fingerprint (slab mode)
  └── ext_fp (32 bits) → full extended fingerprint (slab mode)
```

**H2 matching** provides a fast first-pass filter: 99.2% of non-matching slots are rejected without examining the actual key.

## Eviction: LFU + LRU Hybrid

When all 4 slots in a bucket are full and a new entry must be inserted, PulseMap **evicts** the least-useful entry. The eviction decision uses embedded metadata:

### LFU Counter (4 bits per slot)

Tracks **access frequency** (0-15). Incremented on every `get()` or `insert()` hit. Saturates at 15.

### LRU Recency (3 bits per slot)

Tracks **relative recency** among the 4 slots. Set to max (7) on access, other slots decay by 1.

### Eviction Score

```
score = lfu_count + (recency × 2)
evict = slot with minimum score
```

**Zero additional cache misses** — all metadata is in the same 8-byte MetaWord already loaded for the H2 check.

## Memory Layout

```
ConcurrentPulseMap
  ├── RwLock<MapInner>
  │     ├── Vec<UnsafeCell<Bucket>>   ← 64 bytes each, cache-aligned
  │     ├── BucketLocks               ← 1 AtomicU8 per bucket
  │     ├── Mutex<SlabPool>           ← arena for large KV pairs
  │     ├── Mutex<Vec<SlotTTL>>       ← per-entry TTL metadata (v0.6.1+)
  │     ├── num_buckets: usize
  │     └── bucket_mask: usize       ← num_buckets - 1 (power of 2)
  ├── count: AtomicUsize             ← number of entries
  ├── eviction_count: AtomicUsize    ← total evictions
  ├── auto_resize: bool
  └── resize_threshold: f64          ← default 0.75

ShardedPulseMap (v0.6.1+)
  └── shards: [ConcurrentPulseMap; 16]
        ├── Shard 0:  independent RwLock + buckets + slab
        ├── Shard 1:  independent RwLock + buckets + slab
        ├── ...
        └── Shard 15: independent RwLock + buckets + slab
      Shard = h1 >> 60 (top 4 bits of hash)
```
