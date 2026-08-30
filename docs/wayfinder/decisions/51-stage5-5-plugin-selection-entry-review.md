---
title: "Stage 5.5 Plugin Selection by Profiles Entry Review - Narrowing-Only Domain Activation"
label: "wayfinder:decision"
date: 2026-08-28
status: accepted
ticket: "Stage 5.5 Plugin Selection by Profiles Entry Review"
adr: "ADR 0036"
---

# Decision 51 — Stage 5.5 Plugin Selection by Profiles Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Ticket:** [Stage 5.5 Plugin Selection by Profiles Entry Review](../tickets/51-stage5-5-plugin-selection-entry-review.md)

## Question on the table

Does Stage 5.5 complete the Plugins clause of ADR 0036 §6 by letting a
Profile narrow which Host-enabled plugins a session activates, with the
intersection property making widening impossible by construction, and what
is the frozen evidence shape?

## Contract (C1–C6, approved 2026-08-28)

| #   | Clause              | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the selection model: `ProfileRecord.plugins: Option<Vec<String>>` + `select_profile_plugins(enabled, selected)` with the mechanical intersection property (activated = enabled ∩ selected; the intersection can only shrink, so widening is impossible by construction) + digest-bound `PluginSelectionEvidence`. `siralos-adapters::profile_config` parses the `plugins` key (bounded, unique). Session application to live activation waits for its own slice. |
| C2  | Authority invariant | Selection filters, never grants: an id the Host has not enabled cannot be activated by naming it in a profile. Selected ids outside the enabled set produce truthful diagnostics and stay inactive. Enabled-set membership is Host authority (decision 39), never profile authority.                                                                                                                                                                                                              |
| C3  | Posture             | Zero spawn, zero fs in the core seam. Absent selection = typed unfiltered (all enabled stay). Empty selection = typed narrowed-to-none. Existing profile/lock evidence payloads are untouched (additive slice; old digests stable).                                                                                                                                                                                                                                                               |
| C4  | Evidence            | Frozen differential subject `composition-plugin-selection` ×4 at v43: (1) absent selection → unfiltered; (2) selection narrows → intersection activates; (3) selection names unknown ids → diagnostics, nothing broadened; (4) empty selection → activated plugins=0. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                           |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v42 → v43 (280 → 284 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (280 → 284; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                       |
| C6  | Lean guardrails     | No acquisition/installation machinery, no network, no multi-agent machinery, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                       |

## HITL answers (2026-08-28, recorded verbatim)

- **Q1 Slice scope** — approved: _"Core + adapters, session deferred
  (Recommended)"_ — "Consistent with 5.3: consumption surfaces wait; the
  selection semantics freeze and hold differential evidence first."
- **Q2 Subject matrix** — approved: _"composition-plugin-selection x4 as
  drafted (Recommended)"_ — "Covers the filter property end to end including
  the truthful diagnostics case."
- **Q3 Semantics** — approved: _"Confirm (Recommended)"_ — "The profile
  filters Host-enabled plugins; it can never enable, install, or activate
  anything."
- **Q4 Contract** — approved: _"Approve C1-C6 as drafted (Recommended)"_ —
  "Freeze decision 51 and proceed to implement the selection model, parsing,
  and v43 reconciliation end to end."

## Resolution

**The Stage 5.5 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. **Implemented at
`5e1b3e0`** (corpus v43/284 files, expectations 45 records, audit 279/279
applicable required; the intersection property unit-proven). Acceptance: gates
green, `composition-plugin-selection` ×4 at required parity at v43, the
intersection property unit-proven, `check:rust` green, docs atomic.

## Self-loop verification

Ledger (criterion → evidence → verdict) recorded at record-complete in the map
and AGENTS.md; loop budget one pass + two repairs.
