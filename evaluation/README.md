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
python3 capture.py before --smoke
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

## Reporting rules

Record commit, `rustc -Vv`, CPU/OS, flags, lockfile and commands with raw CSV output.
Run on an otherwise idle machine; compare paired baseline/candidate trials under the
same harness, features and profile. Report all seeds and spread, not one best sample.
Repeat timing experiments if the difference is within observed noise. No universal
"fastest" conclusion follows from these workloads. Competitor hashes are their
defaults; these are end-to-end library comparisons, not isolated algorithm timings.

`--smoke` is for correctness/CI. CI never gates on wall-clock speed or RSS. Real MCU
performance needs physical hardware; this executable is a host benchmark.

The separate root Criterion suite is historical; its fairness fixes/results are
tracked in v0.6.6 T05. Do not mix old timing tables with this harness's results.
