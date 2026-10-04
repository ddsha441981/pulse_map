# Language Bindings

C, Python (PyO3), Java (Panama FFM) and Node.js bindings are maintained in the
separate [pulse_map_bindings](https://github.com/ddsha441981/pulse_map_bindings)
repository. This Rust crate contains no C ABI or language-binding artifacts.

Use that repository's versioned README, headers and examples for:

- exact library/package names and supported runtime versions;
- return codes, buffer sizes, TTL integer widths and thread-safety contracts;
- handle ownership/freeing and platform build commands.

Bindings have independent version/dependency resolution. Preparing Rust v0.6.6
does not establish that a released binding already links it. Check the resolved
core dependency when reproducing behaviour. Older guide examples describing C as
the only binding and a u32 TTL ABI are historical, not the current Rust API.
