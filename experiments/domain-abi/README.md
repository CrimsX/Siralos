# Domain ABI decision spike (assurance contract Parts 16–19)

Two minimal prototypes were built, exercised, and measured on the same
host (Windows 11, x86_64-pc-windows-gnu, release builds):

| Measurement                  | WIT/Component (measured with wasmtime 31)            | IPC (JSON-lines subprocess)          |
| ---------------------------- | ---------------------------------------------------- | ------------------------------------ |
| Startup to first call        | 13.8 ms (component load + instantiate)               | 3.75 ms (process spawn + handshake)  |
| Steady-state call            | 372 ns/call                                          | 28.8 µs/call                         |
| Host-mediated workspace read | 5.5 µs                                               | 37.3 µs                              |
| Process capability request   | 500 ns (denied)                                      | 39.9 µs (denied)                     |
| Large payload (1 MiB)        | 1.34 ms                                              | 4.55 ms                              |
| Crash containment            | runtime trap, contained in-process                   | OS process boundary                  |
| Capability mediation         | structural: linker grants exactly the imports        | convention: host mediates requests   |
| Type safety                  | compile-time WIT interface                           | runtime-validated serde schema       |
| Version incompatibility      | component/world version mismatch fails instantiation | handshake rejection (tested)         |
| Package identity binding     | per-op bind (tested)                                 | per-op identity in messages (tested) |
| Malformed input              | rejected at bindgen boundary                         | typed protocol errors (tested)       |
| Distribution                 | single cross-platform artifact, digestable           | per-platform native binary           |
| Toolchain                    | wasm-tools + wit-bindgen + wasm32-wasip2             | plain cargo                          |

Both prototypes demonstrate the authority rule: the domain holds no
filesystem or process authority; every host effect is mediated. The
IPC prototype additionally exercises protocol mismatch, child crash,
and channel-close behavior; bounded-wait for a hung child is recorded
as a host-runtime requirement (the prototype's blocking read has no
timeout).

The retained WIT host now pins Wasmtime 47.0.3 with a minimal feature
set. The table preserves the original decision-spike measurements from
Wasmtime 31; it does not present them as fresh measurements of the
patched retained runtime.

## How to reproduce

```text
# IPC
cd experiments/domain-abi/ipc
cargo test --locked
cargo build --release --bins --locked && cargo run --release --locked --bin host

# WIT
cd experiments/domain-abi/wit/guest
cargo build --release --target wasm32-wasip2 --locked
cp target/wasm32-wasip2/release/domain_abi_wit_guest.wasm ../guest.component.wasm
cd ../host && cargo run --release --locked -- ../guest.component.wasm
```

The scheduled assurance workflow reproduces both prototypes on Linux
with locked dependency resolution and warnings denied, executes their
conformance paths, and audits each standalone lockfile with the shared
dependency policy. See ADR 0034 for the decision and its revisit
conditions.
