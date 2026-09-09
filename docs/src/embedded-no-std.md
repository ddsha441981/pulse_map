# Embedded & `no_std`

PulseMap runs on bare-metal microcontrollers — not just `cargo check`, but
actually executed on emulated ARM hardware via QEMU.

## Quick Start

```toml
# Most targets (Cortex-M3+, RISC-V with A extension, WASM)
pulse_map = { version = "0.6", default-features = false }

# Cortex-M0 / RP2040 / ESP32-C3 (no hardware atomic CAS)
pulse_map = { version = "0.6", default-features = false, features = ["critical-section"] }
```

## What You Get in `no_std`

With `default-features = false`, PulseMap needs only `alloc` (a heap allocator).
You get:

- `PulseMapRaw` — insert / get / remove / peek / TTL
- `TypedPulseMap<K, V>` — type-safe wrapper
- `MetaWord` — 8-byte packed LFU+LRU metadata
- `Bucket` — 64-byte cache-line-aligned storage

You do **not** get `ConcurrentPulseMap` or `ShardedPulseMap` (they need `std`
for `RwLock` and threading).

## Verified Targets

Every push runs `cargo check` in CI for all 8 targets. Two of them also
**execute** on QEMU (marked with 🏃):

| Target | Chips | Extra Feature | Tested |
|---|---|---|---|
| `thumbv7m-none-eabi` | Cortex-M3 | — | 🏃 QEMU |
| `thumbv7em-none-eabihf` | Cortex-M4F / M7F (STM32F4) | — | ✅ build |
| `thumbv8m.main-none-eabi` | Cortex-M33 (nRF9160) | — | ✅ build |
| `thumbv6m-none-eabi` | Cortex-M0 / M0+ (RP2040) | `critical-section` | 🏃 QEMU |
| `riscv32imac-unknown-none-elf` | RISC-V with A extension | — | ✅ build |
| `riscv32imc-unknown-none-elf` | ESP32-C3 | `critical-section` | ✅ build |
| `aarch64-unknown-none` | 64-bit bare metal | — | ✅ build |
| `wasm32-unknown-unknown` | WebAssembly | — | ✅ build |

## Why `critical-section`?

PulseMap's `MetaWord` is an `AtomicU64`. On chips without hardware atomic CAS
(Cortex-M0, RISC-V without the A extension), `portable-atomic` cannot build a
64-bit atomic at all. The `critical-section` feature solves this by masking
interrupts around the update.

The critical-section **implementation** comes from your HAL, not from PulseMap:

```rust
// Cortex-M0 / RP2040 — add to your binary crate
cortex-m = { version = "0.7", features = ["critical-section-single-core"] }

// ESP32-C3
esp-hal = { version = "...", features = ["critical-section"] }
```

## Memory Footprint

On a Cortex-M with 16 KiB RAM, PulseMap fits comfortably:

| Config | Buckets | Slots | Heap Used |
|---|---|---|---|
| 16 buckets | 16 | 64 | **2,048 bytes** |
| 4 buckets | 4 | 16 | **512 bytes** |

Each bucket is exactly 128 bytes of heap (64B bucket + 64B slot TTL metadata).
A 64-slot cache in 2 KiB of RAM is what makes PulseMap practical on
parts where Moka and QuickCache cannot even compile.

## Running the QEMU Tests Yourself

```bash
cd qemu-test

# Cortex-M3 (portable-atomic spinlock fallback)
cargo run --release --target thumbv7m-none-eabi

# Cortex-M0 (critical-section, interrupt masking)
cargo run --release --target thumbv6m-none-eabi --features m0
```

Requires `qemu-system-arm` installed (`apt install qemu-system-arm`).

The test inserts, gets, evicts, checks TTL, and removes — all on real ARM
instructions, not a compile check. Output:

```
qemu-test: 16 buckets, 64 nominal slots, 3072 heap bytes used
qemu-test: all checks passed
```

The 3,072 covers **all three maps the test creates** — the 16-bucket main
map (2,048 B) plus two 4-bucket maps for the TTL and remove checks
(512 B each): 24 buckets × 128 B. A lone 16-bucket map is 2 KiB, matching
the table above.

## PulseMap vs LRU on Embedded

In `no_std`, the only other option is `lru`. Here's why PulseMap wins:

| | PulseMap | lru |
|---|---|---|
| Memory per entry | **34 B** (at scale) | 68 B |
| Eviction policy | LFU + LRU (smart) | Pure LRU |
| Cache-line aligned | ✅ 64-byte buckets | ❌ pointer chasing |
| Hit rate (Zipfian) | **96.73%** | 95.83% |
| Allocation pattern | Upfront, predictable | Per-insert, fragmented |

On memory-constrained MCUs, PulseMap stores **2x more entries** in the same
RAM, with a better eviction policy.
