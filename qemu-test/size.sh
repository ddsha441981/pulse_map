#!/usr/bin/env sh
# Flash/RAM footprint: pulse_map vs lru, same workload, same target, same flags.
#
# `text` is the only discriminating column: `bss` is dominated by the 8 KiB static
# heap that all three probes share, and `footprint_none` is the floor contributed
# by cortex-m-rt, semihosting, the panic handler and the allocator. Subtract it to
# get what each cache actually costs in flash.
#
# Host binutils `size` reads ARM ELF, so no arm-none-eabi toolchain is needed.
#
#   ./size.sh                                        # thumbv6m + m0 (Cortex-M0)
#   ./size.sh thumbv7m-none-eabi                     # Cortex-M3, no critical-section
set -eu

# Present on every GitHub ubuntu runner, but a missing binutils should not fail a
# job whose only output is documentation.
command -v size >/dev/null || { echo "binutils size not found; skipping footprint"; exit 0; }

target="${1:-thumbv6m-none-eabi}"
[ $# -gt 0 ] && shift
case "$target" in
thumbv6m-none-eabi) set -- --features m0 "$@" ;;
esac

cargo build --release --target "$target" --bins "$@" >/dev/null

sect() { size "target/$target/release/$1" | awk 'NR==2{print $1}'; }
base=$(sect footprint_none)

printf '\n%s (baseline text = %s B)\n' "$target" "$base"
printf '%-12s %10s %12s\n' cache text 'over baseline'
for b in footprint_pulse footprint_lru; do
    t=$(sect "$b")
    printf '%-12s %10s %12s\n' "${b#footprint_}" "$t" "$((t - base))"
done
echo
