# Contributing to PulseMap

Thank you for your interest in contributing to PulseMap! 🎉

## Getting Started

```bash
git clone https://github.com/ddsha441981/pulse_map.git
cd pulse_map
cargo build
cargo test
```

## Development Workflow

### 1. Fork & Branch
```bash
git checkout -b feature/your-feature
```

### 2. Code Standards

- **Format**: Run `cargo fmt` before committing
- **Lint**: Run `cargo clippy -- -D warnings` — zero warnings policy
- **Test**: Run `cargo test` — all tests must pass
- **Docs**: Run `cargo doc --no-deps` — no doc warnings

### 3. Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):
```
feat: add TTL-based expiration
fix: resolve race condition in concurrent resize
perf: optimize H2 matching with SIMD prefetch
docs: update benchmark tables
```

### 4. Pull Request

- Describe what changed and why
- Reference any related issues
- Ensure CI passes (clippy, fmt, test, doc)

## Architecture

```
Layer 4: sync.rs    → ConcurrentPulseMap (thread-safe, per-bucket locks)
Layer 3: lib.rs     → User API (TypedPulseMap<K,V>, PulseMap, Entry API)
Layer 2: raw.rs     → Hash table logic (insert/get/remove/evict)
Layer 1: core/      → Building blocks (MetaWord, Slot, Bucket, hash)
```

### Key Design Rules

1. **Every bucket = exactly 64 bytes** (1 CPU cache line)
2. **Eviction is zero-cost** — embedded LFU+LRU metadata in MetaWord
3. **No heap allocation in hot path** — inline slots for small KV pairs
4. **Thread safety via `&self`** — no `&mut self` for CRUD, enables `Arc<ConcurrentPulseMap>`

## Testing

```bash
# All tests
cargo test

# Specific feature
cargo test --no-default-features     # no_std
cargo test --features simd           # SIMD (x86_64)

# Benchmarks
cargo bench
```

## Feature Flags

| Flag | Default | Description |
|------|:-------:|-------------|
| `std` | ✅ | Standard library (ConcurrentPulseMap) |
| `simd` | ❌ | SIMD H2 matching (x86_64 only) |

## Reporting Issues

- **Bug**: Include Rust version, OS, and minimal reproduction
- **Performance**: Include benchmark results with `cargo bench`
- **Feature**: Describe the use case, not just the solution

## Code of Conduct

Be respectful, constructive, and inclusive. We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct).

## License

By contributing, you agree that your contributions will be licensed under MIT OR Apache-2.0.

---

**Questions?** Open an issue or reach out to [@ddsha441981](https://github.com/ddsha441981).
