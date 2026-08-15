# Synthetic conformance domain (Stage 3R R6)

This directory holds the deterministic synthetic Domain that proves the
production Host/Domain boundary (ADR 0034). It is test/conformance
material only: it is NOT a shipped product Domain, and it contains no
product-specific semantics.

## Layout

- `guest/` — standalone Cargo crate building the conformance component
  (`siralos-domain-conformance-guest`) for `wasm32-wasip2` against the
  production WIT (`crates/siralos-adapters/wit/domain-abi.wit`). The
  guest exports `domain-api` (bind, query, request-effect) and imports
  `host-effects`; its `query` accepts deterministic markers:
  `trap` (guest fault), `loop` (unbounded execution), `denied`
  (guest-initiated denied effect), and any other text (deterministic
  semantic query: token count and byte length).
- `guest-incompatible/` — fixture guest built against a 1.1.0 WIT with
  an added interface member; the production 1.0.0 host must refuse to
  instantiate it (unknown/incompatible ABI fails explicitly).
- `fixtures/` — the exact checked-in component bytes consumed by the
  production conformance tests
  (`crates/siralos-adapters/tests/domain_conformance.rs`), which assert
  their exact SHA-256 digests.

## Rebuilding the components

Prerequisites: the pinned Rust toolchain with the `wasm32-wasip2`
target, `wasm-tools`, and `wit-bindgen` availability as documented in
the assurance workflow. Rebuild and refresh the fixtures with:

```text
npm run build:domain-conformance
```

or manually:

```text
cargo build --manifest-path tests/domain-conformance/guest/Cargo.toml
  --release --target wasm32-wasip2 --locked
cp tests/domain-conformance/guest/target/wasm32-wasip2/release/\
  siralos_domain_conformance_guest.wasm \
  tests/domain-conformance/fixtures/conformance-domain.component.wasm

cargo build --manifest-path tests/domain-conformance/guest-incompatible/Cargo.toml
  --release --target wasm32-wasip2 --locked
cp tests/domain-conformance/guest-incompatible/target/wasm32-wasip2/release/\
  siralos_domain_conformance_guest_incompatible.wasm \
  tests/domain-conformance/fixtures/incompatible-domain.component.wasm
```

The build script prints the new SHA-256 digests; when the bytes change,
update the digest constants in the conformance test file in the same
change. The standalone guest lockfiles are audited with the shared
dependency policy exactly like the retained experiments.

## What the conformance tests prove

`cargo test -p siralos-adapters --test domain_conformance` proves, on
the real production boundary: exact-byte digest identity, load only
when ABI-compatible (versioned export name checked before
instantiation), wrong/stale digest rejection before any semantic work,
deterministic semantic queries, the permitted mediated workspace read
and the denied process-exec request (grant-checked, bounded), the
absence of any ambient filesystem/network/process import (only the
world interfaces plus the minimal wasm32-wasip2 std plumbing), trap
containment with typed fault outcomes and session stop, fuel-bounded
termination of pathological guests, input/output/host-call bounds,
cancellation, and session-scoped activation binding.
