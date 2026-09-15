---
title: "Supply chain: the licensing decision and the cargo-vet re-evaluation"
label: "wayfinder:ticket"
status: closed
date: "2026-09-12"
supersedes: []
---

# Supply chain: the licensing decision and the cargo-vet re-evaluation

Found by the ticket 131 sweep: two supply-chain decisions have a decision point but
no owner, and one of them has already passed it.

## 1. Licensing (the concrete one)

`docs/development/SUPPLY_CHAIN.md` says "the licensing decision is deferred to Stage
6 release planning". Stage 6 is Verified (decisions 58-59) and **the crates still
carry no license field** in their `Cargo.toml`. The deferral's decision point passed
unowned, so this is the one item in the sweep with an external consequence: an
unpublished crate with no license is unusable by anyone who takes it.

**Acceptance:** a decision that names the license (or explicitly keeps the
workspace unpublished and says why), applied to every workspace member that would
be published, with the license text present and `Cargo.toml` metadata consistent.

## 2. `cargo-vet` re-evaluation

Same file: "`cargo-vet` is not adopted at this stage". Nothing records when the
re-evaluation happens or what would trigger it. With a small dependency set and a
pinned toolchain the honest options are to adopt it, or to state the re-evaluation
trigger (e.g. "revisit when the dependency tree grows or a crate is published").

**Acceptance:** adoption, or an explicitly triggered re-evaluation written into
`SUPPLY_CHAIN.md` — either way the deferral stops being indefinite.

## Out of scope

- Any change to the dependency set beyond what a license decision requires.
- Publishing anything: this ticket decides and documents, it does not release.

## Resolution (2026-09-12)

Decision [175](../decisions/175-licensing-and-cargo-vet-decided.md) records both
halves; `docs/development/SUPPLY_CHAIN.md` carries them.

**Licensing.** The deferral ("deferred to Stage 6 release planning") is replaced by
a decision that can act: every workspace member sets `publish = false` and the
architecture gate FAILS a crate that does not
(`scripts/check-rust-architecture.mjs`: "private crates must set publish = false"),
so the workspace is private and unpublished **by policy, mechanically enforced** —
and a private crate needs no license field, because there is no distribution for a
license to govern. The consequence is now stated instead of implied: the repository
is public but **grants no license for reuse**. The license decision is
PRECONDITIONED ON PUBLISHING, and its trigger is written down (first publish
attempt, a shipped distribution artifact, or an explicit intent to allow reuse),
with `MIT OR Apache-2.0` named as the default to reach for when it fires.

**cargo-vet.** Adoption is still declined, but the indefinite "re-evaluated as the
graph grows and again at Stage 6" is replaced by four explicit triggers: the
distinct third-party direct dependency count passing 20 (12 today, across 19 direct
entries in three crates), a workspace crate being published, a supply-chain
incident touching the graph, or SBOM/provenance tooling being adopted.

Both halves are verifiable from the repository: the ratchet line in the
architecture check, and the numbers above (recounted rather than recalled).
