# v0.6.5 review baseline

Reviewed 2026-10-04, source `c3e358a190f2453f9693b183b1309b8739c27b4d`.
Published 0.6.5 checksum:
`3ccf7f4306c2979b327bab9437089f8293412700598872742ffca00952487471`.
Published and local `src/` were identical before fixes. The original independent
evaluation completed without assertion failures. Full methodology: `README.md`.

| Existing independent scenario | Baseline observation |
|---|---|
| Typed/Concurrent S2 Zipf 0.99 | 73.64% / 73.64% |
| Typed/Concurrent S2 Zipf 0.70 | 33.42% / 33.42% |
| Typed/Concurrent S2 hot+scan | 76.14% / 76.14% |
| 1,000 warm hot keys followed by 200,000 cold keys | 1,000 survivors (LRU: 0) |
| Repeated scan | 22.14% hits (LRU: 0.00%) |
| Sensor trace | 81.60% hits (LRU: 79.85%) |

These figures describe the original sibling evaluation traces; the in-repo harness
reports its own geometry/seeds and runs the published baseline alongside the candidate.

Confirmed edge-case repros, to become regression tests ticket by ticket:

1. `ConcurrentPulseMap<String,u32>::new(4)`, insert `"" -> 42`, `resize(8)`:
   `Some(42)` becomes `None` with no TTL/eviction pressure.
2. One million distinct u32 inputs in ShardedPulseMap: 8,192 / 16,384 / 32,768
   buckets per shard retain 514,096 / 821,839 / 821,839 entries. The final growth
   overlaps shard hash bits and does not improve retention.
3. AccessBuffer (4,096 slots, 64-event drains): pause consumer A in its first
   callback, refill 64 released slots, pause A in second callback, let consumer B
   drain through wrap, finish A. Event `(4097,0)` pushed once is delivered twice.
   Loss is allowed; duplicate delivery violates the component contract. No value
   corruption was demonstrated by this repro.

Baseline controls passed: ordinary-key resize with TTL migration, sequential buffer
wrap, and lazy get/peek expiry. Epoch TTL and ordinary bucket-local eviction are
documented policies, not newly discovered defects.
