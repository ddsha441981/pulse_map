# FFI & Language Bindings

PulseMap provides native bindings for 4 languages via the [`pulse_map_bindings`](https://github.com/ddsha441981/pulse_map_bindings) repository.

## Architecture

```
                    ┌──────────────────────┐
                    │  pulse_map (Rust)     │ ← crates.io
                    │  ConcurrentPulseMap   │
                    └──────────┬───────────┘
                               │
          ┌────────────────────┼────────────────────┐
          │                    │                     │
    ┌─────┴─────┐      ┌──────┴──────┐       ┌─────┴──────┐
    │ pulse_map  │      │ pulse_map   │       │ pulse_map  │
    │   _ffi     │      │   _py       │       │   _java    │
    │  (cdylib)  │      │  (PyO3)     │       │  (cdylib)  │
    └─────┬──────┘      └──────┬──────┘       └─────┬──────┘
          │                    │                     │
    ┌─────┴──────┐      ┌──────┴──────┐       ┌─────┴──────┐
    │    C/C++   │      │   Python    │       │  Java 22+  │
    │  .so/.dll  │      │   wheel     │       │  Panama    │
    └────────────┘      └─────────────┘       └────────────┘
```

## C FFI

Uses **opaque handle pattern** (`PulseMapHandle*`) for memory-safe C interop.

```c
#include "pulse_map.h"

PulseMapHandle* map = pulse_map_new(1024);
pulse_map_insert(map, key, key_len, val, val_len);

uint8_t buf[4096];
int32_t len = pulse_map_get(map, key, key_len, buf, sizeof(buf));

pulse_map_free(map);  // MUST call — no GC!
```

**Build:** Produces `.so` (Linux), `.dylib` (macOS), `.dll` (Windows) + `.a`/`.lib` static.

## Python (PyO3)

Direct Rust struct exposed as Python class. **GC-safe** — PyO3 handles Drop automatically.

```python
from pulse_map_py import PulseMap

cache = PulseMap(1024)
cache["hello"] = "world"       # __setitem__
print(cache["hello"])          # __getitem__
print("hello" in cache)        # __contains__
del cache["hello"]             # __delitem__
print(len(cache))              # __len__
```

**Build:** `maturin develop` → installs into virtualenv.

## Java 22+ (Panama FFM)

Uses **Foreign Function & Memory API** (no JNI). `AutoCloseable` + `Cleaner` for memory safety.

```java
try (var cache = new PulseMap(1024)) {
    cache.put("hello", "world");
    String val = cache.get("hello");  // "world"
}  // auto-close → frees native memory
// Even if close() is missed, Cleaner GC will free it
```

**Requirements:** Java 22+, `--enable-native-access=ALL-UNNAMED`.

## Node.js (napi-rs)

Native addon via napi-rs. **GC-safe** — V8 handles Drop automatically.

```javascript
const { PulseMap } = require('pulse-map');

const cache = new PulseMap(1024);
cache.set('hello', 'world');
console.log(cache.get('hello'));  // 'world'
console.log(cache.size);         // 1
```

**Build:** `npx napi build --release` → produces `.node` file.

## Memory Safety by Language

| Language | Who frees? | Leak protection |
|----------|-----------|:-:|
| C | User calls `pulse_map_free()` | ❌ Manual |
| Python | PyO3 Drop on GC | ✅ Automatic |
| Java | `close()` + `Cleaner` fallback | ✅ Double safety |
| Node.js | napi-rs Drop on GC | ✅ Automatic |

## Null Safety

| Language | null key | null value | Behavior |
|----------|:--------:|:----------:|----------|
| C | ✅ No-op | ✅ No-op | All functions check `ptr.is_null()` |
| Python | ✅ TypeError | ✅ TypeError | PyO3 rejects `None` as `&str` |
| Java | ✅ NullPointerException | ✅ NullPointerException | Explicit checks |
| Node.js | ✅ Error | ✅ Error | napi-rs rejects `null`/`undefined` |
