---
title: "Stage 5.2 Profile Composition Entry Review - Effective Run Configuration over Host Authority"
label: "wayfinder:decision"
date: 2026-08-28
status: accepted
ticket: "Stage 5.2 Profile Composition Entry Review"
adr: "ADR 0036"
---
# Decision 48 — Stage 5.2 Profile Composition Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Ticket:** [Stage 5.2 Profile Composition Entry Review](../tickets/48-stage5-2-profile-composition-entry-review.md)

## Question on the table

Stage 5.1 landed the Profile model with no consumer (C1 deferred the CLI surface
and lockfile). Does Stage 5.2 complete the ADR 0036 §6 resolution chain —
profile request → resolution → Host policy → effective run configuration — by
composing the workspace profile into the session's effective policy, and what is
the frozen evidence shape?

## Contract (C1–C6, approved 2026-08-28)

| # | Clause | Frozen decision |
| --- | --- | --- |
| C1 | Ownership | `siralos-core::composition` gains `compose_effective_policy`: host rules + a declared profile → effective rules (per capability: the narrowed overlay rule when applied, else the Host rule; the invariant is re-checked at the composition boundary). `siralos-adapters::profile_config` gains `load_workspace_profile` following the `load_plugin_records` pattern (missing/empty file = no profile; oversize/symlink/malformed = typed invalid state). `siralos-cli` session composition consumes it. No `siralos.lock`, no Context controls. |
| C2 | Authority invariant | A profile can only narrow. The effective rule for an overlaid capability is min-rank(Host decision, requested); every non-overlaid capability keeps its Host rule; composition can never produce a rule broader than the Host's own. A refused or invalid profile is never applied. |
| C3 | Posture | A malformed or authority-widening workspace profile does not block session composition and is never silently adopted: it is not applied, and a truthful typed diagnostic records why (Host policy remains the sole authority; ignoring unverified config cannot broaden authority). Zero-configuration and zero-`siralos.toml` stay valid. |
| C4 | Evidence | Frozen differential subject `composition-effective` ×4 at v40: (1) narrowing profile applied → effective rules reflect the overlay; (2) absent profile → effective equals Host; (3) widening overlay ignored → effective equals Host + diagnostic; (4) malformed document ignored → effective equals Host + diagnostic. Expectations mechanism covers the new scenarios (decision 40 C7). |
| C5 | Corpus mechanics | Schema stays 3; corpus bumps v39 → v40 (268 → 272 files, inside the 384 cap); all four contract.mjs sites, protocol validator, strict-loader assert move together per the checklist. |
| C6 | Lean guardrails | No lockfile generation, no Context controls, no Skills/Plugin consumption, no multi-agent machinery, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete. |

## HITL answers (2026-08-28, recorded verbatim)

- **Q1 Slice scope** — approved: *"Compose seam + loader + CLI (Recommended)"* —
  "Completes the ADR 0036 sec 6 chain end to end; everything else (lockfile,
  Context) waits for later slices."
- **Q2 Subject matrix** — approved: *"composition-effective x4 as drafted
  (Recommended)"* — "Each case pins the authority invariant: effective rules
  never broader than Host, bad profiles never applied."
- **Q3 Posture** — approved: *"Confirm (Recommended)"* — "Ignoring unverified
  config cannot broaden authority; the session proceeds on pure Host policy with
  a recorded diagnostic."
- **Q4 Contract** — approved: *"Approve C1-C6 as drafted (Recommended)"* —
  "Freeze decision 48 and proceed to implement the compose seam, workspace
  loader, CLI wiring, and v40 reconciliation end to end."

## Resolution

**The Stage 5.2 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. Acceptance: gates
green, `composition-effective` ×4 at required parity at v40, the authority
invariant re-checked at the composition boundary with adversarial coverage,
`check:rust` green, docs atomic.

## Self-loop verification

Ledger (criterion → evidence → verdict) recorded at record-complete in the map
and AGENTS.md; loop budget one pass + two repairs.
