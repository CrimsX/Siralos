---
title: "Licensing and cargo-vet: Unpublished by Policy, With Triggers That Fire"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "132"
supersedes: []
---

# Licensing and cargo-vet: Unpublished by Policy, With Triggers That Fire

Ticket [132](../tickets/132-supply-chain-decisions.md) ·
[SUPPLY_CHAIN.md](../../development/SUPPLY_CHAIN.md) · [Map](../siralos-roadmap.md)

## 1. The problem

`SUPPLY_CHAIN.md` deferred the licensing decision "to Stage 6 release planning".
Stage 6 is Verified, the crates carry no license field, and nothing owned the
decision point. The same file declined `cargo-vet` "at this stage" with a
re-evaluation clause ("as the migrated product graph grows and again at Stage 6")
that names no trigger and has already passed.

## 2. What the repository actually is

- Every workspace member sets `publish = false`, and that is **mechanically
  enforced**: `scripts/check-rust-architecture.mjs` fails a crate without it
  ("private crates must set publish = false"), so the unpublished state cannot
  drift silently.
- There is no `LICENSE` file and no `license` field anywhere in the product
  workspace (the `.agents/skills/*` fixtures are third-party files).
- The dependency graph is locked and `cargo-deny`-checked (crates.io only, license
  allowlist, wildcard and Git sources denied).

## 3. The decision

**Licensing.** A private, unpublished crate needs no license field: there is no
distribution for a license to govern. So the deferral is replaced by a decision it
can act on — **the license decision is preconditioned on publishing**, and
publishing is preconditioned on removing the ratchet. Stated plainly in
`SUPPLY_CHAIN.md`, because it has a real consequence: the repository is public but
**grants no license for reuse**. That is the current state, not an oversight.

The trigger for naming a license is any of: the first publish attempt, a shipped
distribution artifact, or an explicit intent to let someone else reuse the code.
When it fires the default is `MIT OR Apache-2.0` (the Rust ecosystem norm) applied
to all three members with the license text committed — a small change, not a
redesign.

**`cargo-vet`.** Still not adopted, but the indefinite clause is replaced by four
explicit triggers: the distinct third-party direct dependency count passing 20 (it
is 12 today, across 19 direct entries in three crates), a workspace crate being
published, a supply-chain incident touching the graph, or SBOM/provenance tooling
being adopted.

## 4. Evidence

- The ratchet: `scripts/check-rust-architecture.mjs` (private crates must set
  `publish = false`), run by `npm run check:rust` inside `npm run check`.
- The counts: recounted for this decision from the three `Cargo.toml` files
  (adapters 7, cli 11, core 1 = 19 entries; 12 distinct third-party crates).
- The text: `docs/development/SUPPLY_CHAIN.md` now carries both dispositions and
  points here.

## 5. What this decision deliberately does not do

It does not choose a license. Choosing one for a project that does not distribute
would be a legal product decision with no current effect, and the owner has not
asked for reuse terms. What it does is remove the ambiguity: the state is now
stated, the trigger is named, and the default is ready — so the eventual decision
is one line rather than an open question.
