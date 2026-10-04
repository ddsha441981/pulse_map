# v0.6.6 local ticket log

Base/main: `c3e358a190f2453f9693b183b1309b8739c27b4d`.
Branch: `staging/v0.6.6`. All commits are local.

## T00 — baseline and evaluation infrastructure

Completed in `c877fc4`. Added independent workspace, pinned published baseline and
competitors, shared seeded traces, equal nominal capacity, independent op selection,
u32/u64 payload comparisons and fresh-process memory. Four methodology/adapter tests,
Clippy, all-scenario smoke and full three-trial capture passed. Core all-features,
no_std, fmt and Clippy passed. Raw baseline: `results/before/`.

## T01 — empty-key resize

Seven regressions fail on unmodified v0.6.5: String, Vec/slab value, zero-length array,
finite TTL, never-expire TTL, auto-resize and sharded resize. Removed the empty-key
skip: occupancy is established by Full state, not key length.
All seven pass after the fix, including Miri (22 seconds). Full all-features/no_std
tests and Clippy passed; formatting checked. No policy change was required.

## T02 — shard routing across growth

Added a shard-only bucket-index compaction that removes routing bits 14..17.
Existing maps at <=16,384 buckets/shard keep their distribution; growth uses the
next independent hash bit. All lookup/write/rehash paths use the same derivation.
The boundary regression failed before the fix and passes afterwards:

| Buckets/shard | Before residents | After residents (1M distinct inputs) |
|---:|---:|---:|
| 8,192 | 514,096 | 514,096 |
| 16,384 | 821,839 | 821,839 |
| 32,768 | 821,839 | 967,051 |

Synthetic routing/H2 independence through 32-bit masks, auto-growth TTL, inline/slab
resident preservation and concurrent growth checks passed. Small tests passed Miri;
the large allocation probe is ignored only under Miri. All-features (60 unit +
10 integration + 11 doc), formatting and Clippy passed.

## T03 — access-buffer ownership across wrap

Both 64-slot and production 4,096-slot forced-overlap regressions failed before
(duplicate events 65 and 4097) and pass after sequence-tagged reservation/publication.
One CAS attempt per push/pop; contention/full/unpublished head skips instead of
waiting. Slots are released before callbacks, preventing reuse races and callback
panic poisoning. Removed the inaccurate formal lock-free claim. Payload now uses
usize with two slot bits, eliminating silent 24-bit bucket-index truncation.
Resize discards old-layout access hints under the exclusive map lock.

Checks: 66 unit + 10 integration + 11 doc tests, Clippy/fmt, 6 Loom models (including
two-slot generation reuse), 5 small buffer Miri tests all passed. Production-sized
Miri case is covered by its small deterministic equivalent.

Full paired before/after evidence: `results/after-correctness/`. Across all 21
single-threaded trace/seed combinations, each candidate map has exactly its matching
baseline's hit counts and residents. Correctness has a measurable cost: slots now
hold two usize atomics (16 B on x86_64, previously 4 B). Buffer allocation increases
by 48 KiB per Concurrent map / 768 KiB across 16 shards. Typed/no_std memory is
unchanged. Sharded u32 throughput was lower in this session (e.g. four-thread
candidate 9.397–9.841 vs baseline 10.418–12.143 Mops/s); noisy data is retained in
full. This is a correctness release, not a universal speed/memory improvement.

## T04 — public documentation and executable guide

README, mdBook and public rustdoc now agree on occupied vs unexpired counts,
iterator inclusion, strict `age > ttl`, per-shard epochs, raw/concurrent reclamation,
inline/slab allocation and actual read locks. Removed nonexistent API examples,
u32 TTL sentinels, non-atomic rate-limiter examples, unsupported timing rankings,
and claims that host/QEMU timing establishes physical-MCU performance. Bindings
are correctly linked to their independently versioned C/Python/Java/Node project.

`src/guide_doctests.rs` includes the actual README and every guide page for rustdoc
testing. All 25 executable examples pass, including lazy TTL occupancy assertions.
Strict all-features rustdoc and `mdbook build` pass without warnings. Historical
brain index dated locally; new result tables follow in T05.

## T05 — fair workloads and complete paired evidence

Expanded the in-repo suite with four-slot/realistic adaptation, explicit semantics,
large-shard boundaries, host routing/sensor traces, and corrected host contention
accounting. Captures include source/lockfile SHA256 and build environment. The
all-trial report verifies **63 hit-rate + 32 adaptation + 6 host embedded paired
rows** match in hits/residents. Full eight-scenario capture: `results/t05-full/`.

Corrected root examples' rounded capacities; Criterion large bounded caches now
all use 262,144 slots and print actual residents. Fresh-process memory example
reports both denominators. Marked non-isolated RSS and descriptive timing spread
as exploratory, not reliable rankings. CI gets an eight-scenario paired smoke job.

Checks: evaluation tests (5), Clippy, all-scenario smoke and full capture; root
all-target/all-feature Clippy; memory smoke; seven Criterion concurrent/sharded
smoke cases. `report.py` verified deterministic parity and generated all-trial
tables; README/guide reference them. The original separate eval checkout/results
remain historical inputs; the corrected, reproducible suite is here in-repo.

Strengths/costs: large-shard residents 821,839 → 967,051; scan resistance and inline
typed memory preserved. No frequency aging: the four-slot hot-set shift still has
0/400 new hits, and realistic phase changes trail LRU. The sequence buffer has
extra per-slot ownership/publication work and 48 KiB/map fixed allocation cost.
The repeat capture confirms lower 4T u64 sharded throughput (9.56–9.75 vs
10.52–11.03 Mops/s); some other rows vary widely. Both capture sessions are kept;
correctness fixes are accepted with disclosed overhead, with queue optimization
left for measured follow-up rather than claiming a universal improvement.
