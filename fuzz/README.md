# Fuzz Testing for pulse_map

This directory contains [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) harnesses
for `pulse_map`. Fuzz testing catches edge cases in the unsafe CAS / slab allocation code
that unit tests miss.

> **Windows users** — `cargo-fuzz` uses libFuzzer which requires Clang and does **not**
> run natively on Windows. Use **WSL2** (recommended) or Docker. See the section below.

---

## 🪟 Windows Setup via WSL2 (Recommended)

WSL2 gives you a real Linux environment inside Windows with full filesystem access to your
Windows drive. All commands below are run **inside the WSL2 terminal**.

### Step 1 — Install WSL2 + Ubuntu

Open **PowerShell as Administrator** and run:

```powershell
wsl --install
```

This installs WSL2 and Ubuntu automatically. **Restart your PC** when prompted.

After restart, open **Ubuntu** from the Start menu. It will finish setup and ask you to
create a Linux username and password.

### Step 2 — Install Rust inside WSL2

In the Ubuntu terminal:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Accept defaults (press 1 then Enter)
source ~/.cargo/env
```

Install the nightly toolchain (required by cargo-fuzz):

```bash
rustup toolchain install nightly
rustup override set nightly   # sets nightly for the current directory
```

### Step 3 — Install cargo-fuzz

```bash
cargo install cargo-fuzz
```

### Step 4 — Navigate to your project

Your Windows `D:\` drive is mounted at `/mnt/d` inside WSL2:

```bash
cd /mnt/d/clone/pulse/pulse_map
```

### Step 5 — Run the fuzz target

```bash
# Run indefinitely (Ctrl-C to stop)
cargo fuzz run fuzz_sequences

# Run for 60 seconds
cargo fuzz run fuzz_sequences -- -max_total_time=60
```

### Step 6 — (Optional) Install clang for AddressSanitizer

```bash
sudo apt update && sudo apt install -y clang llvm
```

This lets cargo-fuzz use ASAN to catch memory bugs that would otherwise be silent.

---

## 🐳 Alternative: Docker (no WSL2 required)

If you prefer Docker Desktop for Windows:

```powershell
# From pulse_map/ root in PowerShell
docker run --rm -it `
  -v "${PWD}:/workspace" `
  -w /workspace `
  rust:latest bash -c "
    rustup toolchain install nightly &&
    rustup override set nightly &&
    cargo install cargo-fuzz &&
    cargo fuzz run fuzz_sequences -- -max_total_time=60
  "
```

---

## Prerequisites (Linux / macOS)

## Targets

| Target | Description |
|---|---|
| `fuzz_sequences` | Fuzzes random `insert` / `get` / `remove` / `peek` / `insert_ttl` / epoch-advance sequences over a `PulseMap`. Checks correctness invariants after every operation. |

## Running

```sh
# From the pulse_map/ root directory:

# Run indefinitely (Ctrl-C to stop)
cargo fuzz run fuzz_sequences

# Run for a fixed duration (60 seconds)
cargo fuzz run fuzz_sequences -- -max_total_time=60

# Run with address sanitizer (recommended for CI)
cargo fuzz run fuzz_sequences -- -max_total_time=120 -rss_limit_mb=2048

# List all available fuzz targets
cargo fuzz list
```

## Corpus

libFuzzer automatically grows a corpus in `fuzz/corpus/fuzz_sequences/`. You can seed it
with hand-crafted inputs:

```sh
mkdir -p fuzz/corpus/fuzz_sequences
# Each file is a raw byte sequence interpreted as an operation stream
echo -ne '\x00\x03key\x05value' > fuzz/corpus/fuzz_sequences/seed_insert
```

## Reproducing a Crash

When cargo-fuzz finds a crash it saves the input to `fuzz/artifacts/fuzz_sequences/`.
Reproduce it with:

```sh
cargo fuzz run fuzz_sequences fuzz/artifacts/fuzz_sequences/<crash-file>
```

## What's Checked

The harness verifies the following invariants after every operation:

- `get()` after `remove()` **always** returns `None`
- `get()` after `insert(key, value)` returns `Some(value)` (when no eviction occurred)
- `peek()` and `get()` agree on key presence
- `len() ≤ capacity()` at all times
- `load_factor()` stays in `[0.0, 1.0]`
- No panics, no UB (caught by AddressSanitizer / libFuzzer)

## CI Integration

Add to your GitHub Actions workflow (`.github/workflows/fuzz.yml`):

```yaml
name: Fuzz
on:
  schedule:
    - cron: '0 2 * * *'   # nightly at 02:00 UTC
  workflow_dispatch:

jobs:
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - run: cargo install cargo-fuzz
      - run: cargo fuzz run fuzz_sequences -- -max_total_time=300
        working-directory: pulse_map
```
