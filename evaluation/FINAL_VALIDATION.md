# v0.6.6 — local validation, 2026-10-04

Branch: `staging/v0.6.6`. Prepared locally, unpublished. Linux x86_64;
stable rustc 1.95.0, nightly 1.97.0-nightly (52b6e2c20), MSRV rustc 1.70.0.

## Results

| Check | Result |
|---|---|
| Nightly and stable `cargo test --all-features --locked` | Each: 66 unit + 10 integration + 25 doc tests, no ignores |
| `cargo test --no-default-features --locked` | 38 unit + 7 doc tests |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Pass |
| Strict all-features rustdoc; `mdbook build` | Pass, no warnings |
| Loom `loom_meta` + `loom_access_buffer` | 6 models pass |
| Miri no-default-features | 38 unit + 7 doc tests pass |
| Miri std `--lib` | 62 pass, 4 documented size-related skips |
| Miri resize/shard-growth integrations | 9 pass, 1 large-allocation probe skipped |
| MSRV 1.70 std/no_std check and std build | Pass; expected old-Cargo `lints` manifest warning |
| Eight no_std targets | All check successfully (matrix below) |
| QEMU Cortex-M0 + Cortex-M3 | Functional, allocation and footprint probes pass |
| Pinned evaluation | 5 tests, Clippy, eight-scenario smoke and full paired captures pass |
| Criterion concurrent/sharded smoke | 7 cases pass; equal nominal capacities printed |
| Package verification/dry-run | 70 files; compiled successfully; no upload |
| Packaged README/guide doctests | All 25 pass from unpacked .crate source |

Full std Miri initially exceeded a 20-minute timeout. Two large unit workloads now
use smaller data sizes under `cfg(miri)` with the same code paths/assertions; the
four-thread read/write test additionally checks every returned value. Production
test sizes remain unchanged. The completed std Miri rerun took 120.46 s; targeted
integration suites took 21.99 s and 94.73 s. Existing oversized skips remain
explicit, with ordinary-test coverage and small Miri equivalents.

## Reproduction commands

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
cargo +stable test --all-features --locked
cargo test --no-default-features --locked
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features --locked
RUSTFLAGS='--cfg loom' cargo test -p pulse_map --test loom_meta --test loom_access_buffer --locked
cargo +nightly miri test -p pulse_map --no-default-features
cargo +nightly miri test -p pulse_map --lib
cargo +nightly miri test -p pulse_map --test resize_regressions --test shard_growth
cargo +1.70 check -p pulse_map
cargo +1.70 check -p pulse_map --no-default-features
cargo +1.70 build -p pulse_map
cargo package --list --allow-dirty
cargo publish --dry-run -p pulse_map --allow-dirty
cargo test --manifest-path target/package/pulse_map-0.6.6/Cargo.toml --doc --all-features --locked
```

`--allow-dirty` was needed while the release files were awaiting their local commit;
packaged contents were inspected. No IDE, evaluation, ignored plans, fuzz or QEMU
workspace files were packaged. Guide sources required by doctests were included.
Root Cargo.lock is intentionally ignored; evaluation/Cargo.lock is versioned and
now resolves **path v0.6.6** alongside **registry v0.6.5**.

## Embedded matrix and measured footprint

Checked without default features: thumbv7m-none-eabi, thumbv7em-none-eabihf,
thumbv8m.main-none-eabi, riscv32imac-unknown-none-elf, wasm32-unknown-unknown,
aarch64-unknown-none. Also thumbv6m-none-eabi and riscv32imc-unknown-none-elf with
`--features critical-section`.

From `qemu-test/`, ran release functional binary and all three footprint binaries
on thumbv7m and thumbv6m (latter `--features m0`), then `./size.sh <target>`.

| Measurement | PulseMap | LRU |
|---|---:|---:|
| Heap for 64 resident inline u32/u32 entries | 2,048 B | 4,520 B |
| Construction/fill allocations | 2 | 68 |
| M3 text over no-cache baseline | 9,864 B | 7,744 B |
| M0 text over no-cache baseline | 7,376 B | 7,208 B |

Inline probe: zero allocations over 256 insert/get/remove operations after
construction. Slab u64/u64 probe: 14 allocations over six inserts. The full
functional program's five maps/payloads use 4,592 heap bytes. QEMU does not supply
physical-MCU timing evidence.

## Final paired evaluation

[Final versioned report](results/release-final/report.md), with source hashes,
dirty patch and resolved dependencies, rechecks all eight scenarios after the
version bump. This is a **full** run (metadata `smoke=false`, three seeds, 2M ops),
originally invoked with label `release-smoke`; the directory was renamed to
`release-final` for accuracy without altering captured CSV/metadata contents.

All 63 hit-rate, 32 adaptation and six host-routing/sensor paired rows still match
the baseline in hits/residents. Large-shard residency remains 967,051 vs 821,839.
Typed inline RSS remains 40.04 B/resident. The cost is reproducible: final u32 4T
sharded throughput 11.87–12.19 vs 13.97–14.21 Mops/s, and sharded u32 RSS
60.35 vs 46.41 B/resident. Older captures are retained; documentation's T05 tables
refer to their explicitly linked session. No blanket speed/memory win is claimed.

## Release status

Local preparation is complete. Remote Linux/macOS/Windows CI has not been run for
these unpushed commits, and registry v0.6.6 cannot be smoke-tested until publication.
No issue/PR/tag/push/publication was created. Main remains at
`c3e358a190f2453f9693b183b1309b8739c27b4d`; the pre-existing IDE change is unstaged.
