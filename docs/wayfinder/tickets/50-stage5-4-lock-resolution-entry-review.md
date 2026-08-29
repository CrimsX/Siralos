---
title: "Stage 5.4 siralos.lock Resolution Entry Review - Machine-Generated Portable Identities"
label: "wayfinder:ticket"
type: HITL
status: open
resolution: ""
blockedBy: []
---

# Ticket 50 — Stage 5.4 siralos.lock Resolution Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.4 siralos.lock Resolution Entry Review](../decisions/50-stage5-4-lock-resolution-entry-review.md) (opens after HITL grilling)

## Why now

ADR 0036 §8-9 pair the human-authored `siralos.toml` with a machine-generated
`siralos.lock` of resolved portable identities. Stage 5 now has declarations to
resolve: the profile (5.1/5.2 digest-bound evidence) and the workspace plugin
records (decision 38/39: id/path/digest). The atomic workspace-write pattern
already exists and is mechanically enforceable (`write_record_document`: unique
temp file, lstat regular-file check, rename over target, symlink refusal), so
the lock write can follow the one proven pattern instead of inventing a new
mutation primitive.

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition::lock` owns the lock model: `LockIdentity` (profile digest, sorted plugin records, workspace declarations digest) + `create_workspace_lock` + deterministic canonical serialization. `siralos-adapters::profile_config` (or a sibling `lockfile` module) owns `load_workspace_lock` / `write_workspace_lock` via the established atomic-write pattern, and `verify_workspace_lock` (current / stale / missing). No new write primitives. |
| C2  | Authority invariant | The lock is derived data: it binds digests of declarations the workspace already holds and grants nothing. Regeneration is deterministic and idempotent (same declarations → byte-identical lock). A lock can never include an identity the workspace did not declare, and verification compares the recomputed lock digest, not file bytes alone.                                                                                                                  |
| C3  | Posture             | Missing lock = typed missing (generation is always available). Stale lock = truthful stale report with expected/actual lock digests; nothing is silently rewritten. The lock write uses the same lstat-verified regular-file + rename pattern as the plugin record writer (symlink at target replaced by rename, never followed; any failure removes the temp file).                                                                                                |
| C4  | Evidence            | Frozen differential subject `composition-lock` ×4 at v42 over the pure resolution seam: (1) profile-only workspace → lock with profile identity; (2) workspace with plugin records → sorted identities included; (3) verify current → match; (4) verify stale → expected/actual digests. Expectations mechanism covers the new scenarios (decision 40 C7); the fs write/load paths are covered by adapters unit tests.                                              |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v41 → v42 (276 → 280 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (now 276 → 280 — see the map Notes reminder: run `cargo test -p siralos-cli --lib --all-features` before commit) move together per the checklist.                                                                                                                                                 |
| C6  | Lean guardrails     | No acquisition/installation machinery, no network, no multi-agent machinery, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                         |

## Open questions for HITL grilling

1. **Q1 Scope** — core lock model + adapters load/write/verify with the atomic-write pattern, differential over the pure resolution seam?
2. **Q2 Subject matrix** — `composition-lock` ×4 as drafted?
3. **Q3 Posture** — confirm C3: missing lock is a typed state (not an error), stale lock is reported and never silently rewritten, write follows the plugin-record atomic pattern?
4. **Q4 Approval** — approve C1–C6 as drafted?
