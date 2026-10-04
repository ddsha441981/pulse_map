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
