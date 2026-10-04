# Performance & Benchmarks

The authoritative current methodology and raw evidence live in
[`evaluation/`](https://github.com/ddsha441981/pulse_map/tree/staging/v0.6.6/evaluation).
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
