# PulseMap

PulseMap is a fixed-capacity Rust cache with bucket-local LFU+LRU eviction. Every
64-byte aligned bucket holds four slots and their packed eviction metadata.

The eviction decision uses metadata in the bucket already fetched for lookup.
This avoids an extra metadata cache-line fetch; it does not make operations free.
TTL, slab storage and concurrency tracking require additional memory accesses.

Choose it when entries are disposable and compact keys/values make inline storage
useful. Use an ordinary map when all entries must be retained. Evaluate scan
resistance, hot-set changes, memory and concurrency separately.

Start with [Getting Started](getting-started.md), then [Core Concepts](core-concepts.md)
and [Benchmarks](benchmarks.md). Embedded support and its measured limits are in
[Embedded & no_std](embedded-no-std.md).
