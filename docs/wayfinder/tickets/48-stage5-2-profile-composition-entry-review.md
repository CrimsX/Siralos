---
title: "Stage 5.2 Profile Composition Entry Review - Effective Run Configuration over Host Authority"
label: "wayfinder:ticket"
type: HITL
status: open
resolution: ""
blockedBy: []
---

# Ticket 48 — Stage 5.2 Profile Composition Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.2 Profile Composition Entry Review](../decisions/48-stage5-2-profile-composition-entry-review.md) (opens after HITL grilling)

## Why now

Stage 5.1 (`be030e3`) landed the Profile model and narrowing resolution but no
consumer: C1 deferred the CLI composition surface. ADR 0036 §6 freezes the
resolution chain — profile request → resolution → Host policy → **effective run
configuration** — and the last link is missing. Existing seams:
`siralos-core::composition` (5.1), `siralos-adapters::profile_config` (5.1 bounded
TOML parsing), and the `load_plugin_records` workspace-`siralos.toml` reader
pattern (decision 38/39: missing file = empty, malformed = typed refusal,
fail-closed).

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` gains `compose_effective_policy`: host rules + a resolution → effective rules (per capability: the narrowed overlay rule when applied, else the Host rule). `siralos-adapters::profile_config` gains `load_workspace_profile` following the `load_plugin_records` pattern (missing/empty file = no profile; oversize/symlink/malformed = typed failure). `siralos-cli` session composition consumes it. No `siralos.lock`, no Context controls. |
| C2  | Authority invariant | A profile can only narrow. The effective rule for an overlaid capability is min-rank(Host decision, requested); every non-overlaid capability keeps its Host rule; composition can never produce a rule broader than the Host's own. A refused or invalid profile is never applied.                                                                                                                                                                                         |
| C3  | Posture             | A malformed or authority-widening workspace profile does not block session composition and is never silently adopted: it is not applied, and a truthful typed diagnostic records why (Host policy remains the sole authority; ignoring unverified config cannot broaden authority). Zero-configuration and zero-`siralos.toml` stay valid.                                                                                                                                  |
| C4  | Evidence            | Frozen differential subject `composition-effective` ×4 at v40: (1) narrowing profile applied → effective rules reflect the overlay; (2) absent profile → effective equals Host; (3) widening overlay ignored → effective equals Host + diagnostic; (4) malformed document ignored → effective equals Host + diagnostic. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                   |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v39 → v40 (268 → 272 files, inside the 384 cap); all four contract.mjs sites, protocol validator, strict-loader assert move together per the checklist.                                                                                                                                                                                                                                                                                        |
| C6  | Lean guardrails     | No lockfile generation, no Context controls, no Skills/Plugin consumption, no multi-agent machinery, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                         |

## Open questions for HITL grilling

1. **Q1 Scope** — core compose seam + workspace loader + CLI session consumption, no lockfile/Context?
2. **Q2 Subject matrix** — `composition-effective` ×4 as drafted (narrowed / absent / widening-ignored / invalid-ignored)?
3. **Q3 Posture** — confirm C3: a bad profile is not applied and yields a truthful diagnostic with Host authority unchanged (rather than blocking composition)?
4. **Q4 Approval** — approve C1–C6 as drafted?
