# Core Concepts

## Bucket layout

```text
Bucket = 64 bytes, alignment 64
  MetaWord: 8 bytes
    states:     4 × 2 bits
    H2 hashes:  4 × 7 bits
    priorities: 4 × 7 bits
  Slots: 4 × 14 bytes
```

Inline: key ≤6 bytes **and** value ≤7 bytes. Larger keys or values store a slab
index and extended fingerprint in the slot. `u32/u32` is inline; `u64/u64` is slab.
Small typed numeric conversions use stack arrays, but slab storage still allocates.

WyHash with fixed seed 0 hashes the key bytes. Low bits select the bucket; high
seven bits (57–63) form H2. Full key comparison follows a fingerprint match, so
fingerprint collisions do not mean key equality. This fixed hash is not a HashDoS
resistance guarantee for adversarial inputs.

## Local eviction

Each bucket has four slots without chaining. An insert into a full bucket replaces
its minimum-priority slot, even if a neighbouring bucket is empty. Priority is
`(frequency << 3) | recency`: frequency dominates, recency breaks ties. See
[Eviction](eviction.md) for aging and cold-start behaviour.

TTL records are outside the bucket: four × 16 bytes per bucket. Thus the fixed
raw/typed allocation is 128 B/bucket, not 64. Slab storage grows with payload sizes.

## Occupancy vs live lookup

Expiry is lazy: `get`/`peek` hide expired entries. `len`, `is_empty`, `load_factor`
and iteration describe physically occupied slots, including expired unreclaimed
ones. Iterating is not equivalent to repeatedly calling `get` on unexpired entries.
