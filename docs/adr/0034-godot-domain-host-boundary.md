---
id: ADR-0034
status: accepted
domains: [architecture, security]
paths:
  [
    experiments/domain-abi/**,
    docs/development/PROTOCOL_VERSIONING.md,
    crates/siralos-adapters/wit/**,
    crates/siralos-adapters/src/domain/**,
    crates/siralos-core/src/domain/**,
    tests/domain-conformance/**,
  ]
supersedes: []
---

# ADR 0034 — Godot Domain Host Boundary

- Status: accepted (pre-Stage-4 assurance — domain host ABI decision
  spike, contract Parts 16–19)
- Date: current milestone
- Related: ADR 0032 (Rust migration), ADR 0033 (differential harness),
  ADR 0001 (modular monolith)

## Problem

Before Stage 4 runtime integration, the packaging boundary between the
Siralos host and the optional Godot domain must be decided. The domain
must not own filesystem or process authority merely because the chosen
mechanism could technically allow it; all host effects remain mediated
by Siralos.

## Evaluated candidates

- **Candidate A — WebAssembly Component Model + WIT** host/domain API.
- **Candidate B — separate domain process + versioned IPC**.

Both were prototyped minimally and measured on the same host (see
`experiments/domain-abi/README.md` for full numbers and reproduction).

## Measured evidence (same host, release builds)

| Dimension              | A (WIT/wasmtime)                                 | B (IPC)                               |
| ---------------------- | ------------------------------------------------ | ------------------------------------- |
| Startup to first call  | 13.8 ms                                          | 3.75 ms                               |
| Steady-state call      | 372 ns                                           | 28.8 µs                               |
| 1 MiB payload          | 1.34 ms                                          | 4.55 ms                               |
| Capability enforcement | structural (linker grants only declared imports) | conventional (host mediates requests) |
| Type safety            | compile-time WIT interface                       | runtime-validated schema              |
| Crash containment      | runtime trap, in-process                         | OS process boundary                   |
| Distribution           | one cross-platform artifact, digestable          | per-platform native binary            |

## Decision

**Candidate A — the WebAssembly Component Model with a versioned WIT
world** is selected as the initial Godot domain host boundary.

Rationale, on evidence rather than elegance:

- Per-call and per-payload overhead are one to two orders of magnitude
  lower, and domain semantic work is call-heavy.
- Capability mediation is _structural_: the component imports exactly
  what the linker grants — the "domain must not own filesystem/process
  authority" rule is enforced by the runtime boundary, not by
  convention. The prototype demonstrates host-mediated workspace reads
  and policy-denied process capabilities.
- A component is a single cross-platform artifact whose bytes are the
  package identity (digest-bound), which simplifies package
  verification and binds an active task to the exact package
  version/digest.
- The 13.8 ms startup cost is a per-session cost and is dwarfed by
  task-scale work; it does not affect per-operation latency.
- The wasm toolchain (wasm-tools, wit-bindgen, wasm32-wasip2) was
  exercised end to end on the development host and on the stable
  toolchain, so the boundary is not a nightly-only construction.

## Security consequences

- The domain executes in the wasm sandbox; traps are contained by the
  runtime and never escape into host memory.
- Capability mediation is structural: the WIT world declares exactly
  the imports the domain may use, and the host linker grants them. The
  prototype grants no filesystem, network, or process capability to the
  domain; WASI is provided minimally (stdout plumbing only in the
  prototype) and the production host will grant the minimal set.
- Package identity is bound per operation (`bind`); a stale or
  mismatched package identity is rejected before any semantic work.
- The Godot engine itself remains a separate dependency, never inside
  the domain component.

## Portability consequences

- wasmtime supports all Tier-1 platforms (Windows, Linux, macOS);
  components are portable across them, removing per-platform domain
  builds.
- The wasm toolchain adds a build-time dependency for domain packages
  only; the host core does not require it.

## Compatibility/versioning strategy

- The WIT world is versioned (`package siralos:domain-abi@1.0.0`);
  version-incompatible components fail instantiation (never a silent
  downgrade). Protocol versioning follows
  `docs/development/PROTOCOL_VERSIONING.md` (unknown versions fail
  closed; hard-incompatible changes bump the world version).

## Capability model

- The world declares the domain's capabilities; the host linker grants
  them; the host mediates every effect. Workspace access is granted per
  request with bounds; process/runtime capabilities are denied by
  policy (prototype) and will be mediated by the Stage 4 execution
  boundary.

## Package identity model

- Each component is identified by package id + package digest (SHA-256
  of the component bytes). A task binds to the exact digest at
  activation; anything else is stale and rejected.

## Cancellation behavior

- Cancellation of a long-running domain call is a host-runtime
  requirement (async wasmtime calls + handle dropping); the prototypes
  record the requirement, and the production boundary must implement
  bounded-wait cancellation. In-process traps make cancellation
  immediate once the runtime supports dropping the call.

## Known limitations

- Domain code runs sandboxed; workloads needing heavy native
  computation may see a wasm slowdown (measured 1 MiB payload at
  1.34 ms — acceptable for semantic analysis workloads).
- The wasm toolchain is more complex than a plain cargo build; domain
  package build pipelines must pin wasm-tools/wit-bindgen versions.
- Stage 4 execution boundaries are not implemented yet; the decision
  fixes the boundary, not the runtime behavior.

## Condition that would justify revisiting

- If the wasm toolchain becomes a portability or maintenance blocker on
  a Tier-1 platform, or measured domain workloads regress materially
  (more than 2× native-equivalent latency), the decision is revisited
  with Candidate B (IPC) as the fallback; the IPC prototype is retained
  for that evaluation.
