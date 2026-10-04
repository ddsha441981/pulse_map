# Reproducible cache evaluation

Adapted from the author's independent `pulsemap-eval` (seeded Zipf, uniform, scan,
cache-aside loops, fresh-process RSS). This workspace keeps the published **0.6.5**
baseline and local candidate in the **same optimized executable**, along with pinned
LRU, QuickCache and Moka dependencies. `Cargo.lock` is versioned here.

## Run (working directory: `evaluation/`)

```bash
cargo test --locked
cargo run --release --locked -- all --smoke
cargo run --release --locked -- hitrate
cargo run --release --locked -- throughput
cargo run --release --locked -- memory
cargo metadata --locked --format-version 1 --filter-platform x86_64-unknown-linux-gnu
python3 capture.py my-unique-label --smoke
```

The first two commands test methodology/adapters and exercise every scenario. They
do not establish a performance ranking. Full runs use 65,536 nominal entries,
2,000,000 operations, seeds 42–44 and fresh instances per trial. Order rotates by
trial. All constructors receive the same **actual nominal capacity** (power of two);
four-way local eviction still means different numbers of resident entries.

- Hit-rate: u64/u64, identical pre-generated trace, fill on miss, cold start. Print
  hits, operations and final residents, not just time. Include changing-hot-set
  phases; scan resistance alone does not demonstrate adaptation.
- Throughput: u32/u32 and u64/u64 separately, 1/4/8 threads, prewarm with the first
  capacity operations, independent seeded 5%-probability write decisions. Print the
  **actual write count** and read hits. Misses do not fill in this scenario. Timing
  starts after workers are ready; includes start-barrier release and completion/join.
- Memory: Linux RSS delta in a **fresh subprocess per cache and type**. Include
  empty RSS, filled RSS, residents, bytes/nominal slot and bytes/resident. Equal
  payload widths across competitors. RSS is allocator/OS-dependent, not a byte-exact
  allocation measure. Small-capacity smoke RSS is especially noisy.
- Moka: run pending maintenance before a timed prewarmed run and before reading
  final occupancy. Hit-rate loop allows its normal deferred maintenance.
- LRU: native single-threaded for hit-rate; `Mutex<LruCache>` for threaded scenarios.
  Typed PulseMap is excluded from threaded results rather than silently adding a lock.
- `adapt`: warmed four-slot reproduction and 1,024/65,536-slot working-set shifts,
  followed by a 3×capacity cold scan and ten hot-set passes. Same sequences per map.
- `semantics`: strict TTL boundary, occupied/iterator inclusion, reversible lazy
  visibility and never-expire sentinel, asserted on baseline and candidate.
- `boundary`: 1M u32 keys with 8,192/16,384/32,768 buckets per shard, counts plus
  value checks on every surviving key. Smoke uses smaller allocation sizes.
- `embedded`: host u32/u32 routing and sensor traces, three shared seeds. This is
  not bare-metal execution; the separate qemu-test/ owns MCU instruction/allocation checks.
- `contention`: host writer/reader simulation, one always-resident hot key for equal
  hit work. LRU guard is dropped before timestamp/sample recording. Reports hits,
  read counts and samples while the writer is active, not just an unqualified p99.

## Reporting rules

Record commit, `rustc -Vv`, CPU/OS, flags, lockfile and commands with raw CSV output.
Run on an otherwise idle machine; compare paired baseline/candidate trials under the
same harness, features and profile. Report all seeds and spread, not one best sample.
Repeat timing experiments if the difference is within observed noise. No universal
"fastest" conclusion follows from these workloads. Competitor hashes are their
defaults; these are end-to-end library comparisons, not isolated algorithm timings.

`--smoke` is for correctness/CI. CI never gates on wall-clock speed or RSS. Real MCU
performance needs physical hardware; this executable is a host benchmark.

The root Criterion suite now equalizes nominal capacity (262,144 for large cases,
1,024 for eviction) and prints resident counts. Its timings include spawn/join,
some raw/string/iterator cases are separate references, and HashMap is unbounded.
Old root examples are exploratory; this pinned suite supplies published comparisons.

## Current evidence

- [Full T05 report](results/t05-full/report.md), [raw CSV/provenance](results/t05-full/).
- [Before](results/before/) and [first correctness capture](results/after-correctness/)
  are preserved, including slower candidate measurements.
- `python3 report.py results/t05-full` regenerates all-trial tables and verifies
  paired deterministic hit/resident parity. It does not gate noisy timing or RSS.

This in-repo suite supersedes stale pre-drain output in the separate author's eval
checkout. Its original findings remain valid for their version/workload; “beats LRU
on every skew” is not supported (Zipf 1.30 and large hot+scan can favour LRU).
The corrected host simulation applies the same guard/sample accounting to both
PulseMap versions and Mutex<LRU>. Original files/results are retained as history.
