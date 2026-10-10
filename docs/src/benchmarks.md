# Performance & Benchmarks

The authoritative current methodology and raw evidence live in
[`evaluation/`](https://github.com/ddsha441981/pulse_map/tree/main/evaluation).
It compares published v0.6.5 and the local correctness candidate in one optimized
executable with pinned LRU, QuickCache and Moka dependencies.

## Comparing fairly

- Equal actual nominal capacity; bucket rounding is explicit.
- Like-for-like u32/u32 (PulseMap inline) or u64/u64 (slab).
- Shared seeded traces and operation choices, with independent read/write RNG.
- Native single-thread LRU separately labelled from `Mutex<LruCache>`.
- Fresh-process RSS per map; report both bytes/slot and bytes/resident.
- Repeat timing trials and report spread. CPU scheduling and map policies vary.

These are host measurements. They are not physical-MCU timing results. TTL-disabled
concurrent hits still take the epochs mutex, and slab/value-copy costs remain.

## Historical results

Older v0.6.2 figures and cache-miss/ranking claims in this guide are superseded.
Some old comparisons used a 10,000-entry competitor against PulseMap rounded to
16,384 slots; nominal memory denominators also hid occupancy differences. Historical
outputs remain in repository history and labelled artifacts, not as current rankings.

## Reproduce

```bash
cargo test --manifest-path evaluation/Cargo.toml --locked
python3 evaluation/capture.py my-unique-label
cargo bench --bench benchmark -- 'conc_|sharded_'
```

Capture labels must be new. See evaluation/README.md for workload and RSS limitations.

## v0.6.6 correctness candidate

[Full paired report](https://github.com/ddsha441981/pulse_map/blob/main/evaluation/results/t05-full/report.md)
includes three-seed means/ranges and all raw observations. At 65,536 slots and 2M
cache-aside operations, candidate TypedPulseMap hit rate was 73.26% on Zipf .99,
23.05% on a repeated 3×capacity scan, and 90.58% on changing hot sets; LRU measured
71.69%, 0%, and 97.54% respectively. Scan resistance and adaptation are distinct.

All 63 paired deterministic hit-rate rows match v0.6.5. The separate large-shard
probe retains 967,051 of 1M keys vs 821,839 before the routing fix. The buffer fix
costs 48 KiB/Concurrent map on x86_64 (768 KiB over 16 shards). Measured u32 sharded
RSS was 61.75 B/resident vs 48.12 before; QuickCache was 50.12. Inline typed remained
40.04 B/resident. RSS includes allocator/page effects; the queue allocation increase
is structural.

Four-thread u64 sharded throughput was 9.56–9.75 Mops/s vs 10.52–11.03 for v0.6.5.
This is a measured correctness/performance cost, not a speedup release. Several
other timing rows show substantial host variance; no universal ranking follows.
