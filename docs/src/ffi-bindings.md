# FFI — C Bindings

PulseMap exposes a stable C ABI via the `pulse_map_ffi` crate. This is the only supported language binding — other languages should call through this C layer.

## Architecture

```
                    ┌──────────────────────┐
                    │  pulse_map (Rust)     │ ← crates.io
                    │  ConcurrentPulseMap   │
                    └──────────┬───────────┘
                               │  Rust FFI (#[no_mangle])
                    ┌──────────┴───────────┐
                    │  pulse_map_ffi        │
                    │  libpulse_map.so/.dll │
                    └──────────┬───────────┘
                               │  C header (pulse_map.h)
                    ┌──────────┴───────────┐
                    │  C / C++ consumers    │
                    └───────────────────────┘
```

## Build

```bash
# Build the shared library
cd pulse_map_ffi
cargo build --release

# Output:
#   target/release/libpulse_map.so      (Linux)
#   target/release/libpulse_map.dylib   (macOS)
#   target/release/pulse_map.dll        (Windows)
#   target/release/libpulse_map.a       (static)
```

## C API

Uses an **opaque handle pattern** (`PulseMapHandle*`) — the Rust struct is never exposed directly to C.

```c
#include "pulse_map.h"

int main(void) {
    // Create — 1024 buckets = 4096 slot capacity
    PulseMapHandle* map = pulse_map_new(1024);
    if (!map) return 1;  // allocation failed

    // Insert
    const uint8_t key[] = "session:abc";
    const uint8_t val[] = "user_data";
    pulse_map_insert(map, key, sizeof(key)-1, val, sizeof(val)-1);

    // Get
    uint8_t buf[4096];
    int32_t len = pulse_map_get(map, key, sizeof(key)-1, buf, sizeof(buf));
    if (len >= 0) {
        printf("Found: %.*s\n", len, buf);
    }

    // Remove
    int removed = pulse_map_remove(map, key, sizeof(key)-1);

    // Stats
    printf("Entries:   %zu\n", pulse_map_len(map));
    printf("Evictions: %zu\n", pulse_map_eviction_count(map));

    // Free — MUST call, no GC!
    pulse_map_free(map);
    return 0;
}
```

## Full API Reference

```c
// Lifecycle
PulseMapHandle* pulse_map_new(size_t num_buckets);
void            pulse_map_free(PulseMapHandle* map);

// CRUD
void    pulse_map_insert(PulseMapHandle* map,
                         const uint8_t* key, size_t key_len,
                         const uint8_t* val, size_t val_len);

int32_t pulse_map_get(PulseMapHandle* map,
                      const uint8_t* key, size_t key_len,
                      uint8_t* out_buf, size_t out_len);
// Returns: bytes written (≥0) on hit, -1 on miss, -2 if out_buf too small

int     pulse_map_remove(PulseMapHandle* map,
                         const uint8_t* key, size_t key_len);
// Returns: 1 if removed, 0 if not found

int     pulse_map_contains(PulseMapHandle* map,
                            const uint8_t* key, size_t key_len);

// Stats
size_t  pulse_map_len(const PulseMapHandle* map);
size_t  pulse_map_capacity(const PulseMapHandle* map);
size_t  pulse_map_eviction_count(const PulseMapHandle* map);

// TTL
void     pulse_map_set_ttl(PulseMapHandle* map, uint32_t ttl_epochs);
uint32_t pulse_map_get_ttl(const PulseMapHandle* map);
uint32_t pulse_map_current_epoch(const PulseMapHandle* map);
```

## Null Safety

All functions check for null pointers before dereferencing:

```c
// Safe — pulse_map_free() is a no-op on NULL
pulse_map_free(NULL);

// Safe — pulse_map_insert() checks map != NULL
pulse_map_insert(NULL, key, key_len, val, val_len);  // no-op

// Safe — pulse_map_get() returns -1 on NULL map
int32_t len = pulse_map_get(NULL, key, key_len, buf, sizeof(buf));  // -1
```

## Memory Model

| Question | Answer |
|----------|--------|
| Who allocates? | `pulse_map_new()` — heap via Rust allocator |
| Who frees? | **You** — call `pulse_map_free()` |
| Thread-safe? | ✅ Yes — wraps `ConcurrentPulseMap` |
| GC? | ❌ No — manual lifetime management |

> **Critical:** Always call `pulse_map_free()` when done. Forgetting it leaks the entire map including slab pool.

## Linking

```makefile
# Makefile example
CFLAGS  = -I./pulse_map_ffi/include
LDFLAGS = -L./target/release -lpulse_map -Wl,-rpath,./target/release

your_app: main.c
	$(CC) $(CFLAGS) -o $@ $< $(LDFLAGS)
```
