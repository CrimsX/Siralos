---
title: "Stage 5.7 Session Plugin Activation Gate Entry Review"
label: "wayfinder:decision"
date: 2026-08-29
status: accepted
ticket: "Stage 5.7 Session Plugin Activation Gate Entry Review"
adr: "ADR 0036"
---

# Decision 53 — Stage 5.7 Session Plugin Activation Gate Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Ticket:** [Stage 5.7 Session Plugin Activation Gate Entry Review](../tickets/53-stage5-7-plugin-activation-gate-entry-review.md)

## Question on the table

How does decision 51's frozen profile→plugin narrowing become real session
behavior, and what is the frozen per-id gate and evidence shape?

## Contract (C1–C6, approved 2026-08-29)

| #   | Clause               | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| --- | -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership            | `siralos-core::composition` owns the pure per-id gate: `decide_plugin_activation(enabled, selected, requested)` → typed outcome (`activated` / `refused-filtered` / `refused-not-enabled`) + digest-bound `PluginActivationEvidence` + deterministic rendering. `siralos-cli` owns the wiring: the session holds the applied profile's plugin selection at startup (only when the 5.2 composition actually applied) and `/domains-activate` consults the gate before install/enable/activate. |
| C2  | Authority precedence | Host authority is checked first: a requested id the Host has not enabled is refused regardless of any profile (a profile can never enable). The profile filter then refuses ids outside the selection before any install/enable/activate side effect. Refusals carry truthful, report-safe diagnostics naming the id and the reason.                                                                                                                                                          |
| C3  | Posture              | Zero new fs write paths and zero spawn; the gate is a pure decision over in-memory sets. Absent profile selection = gate transparent (session behaves exactly as before). Invalid/refused profiles contribute no selection (5.2 semantics).                                                                                                                                                                                                                                                   |
| C4  | Evidence             | Frozen differential subject `composition-plugin-activation` ×4 at v45 over the pure seam: (1) absent selection → enabled id activates; (2) narrowed → selected id activates; (3) narrowed → enabled-but-unselected id refused (`refused-filtered`); (4) Host precedence → un-enabled id refused (`refused-not-enabled`) even when selected. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                 |
| C5  | Corpus mechanics     | Schema stays 3; corpus bumps v44 → v45 (288 → 292 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (288 → 292; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                   |
| C6  | Lean guardrails      | No new CLI commands (the gate rides the existing `/domains-activate`); no profile schema change; no skill consumption surface; measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                          |

## HITL answers (2026-08-29, recorded verbatim)

- **Q1 Slice scope** — approved: _"Core gate + CLI wiring + v45 subject
  (Recommended)"_ — "The slice's point is consumption - the frozen 5.5
  filter gates the real activate command."
- **Q2 Subject matrix** — approved: _"composition-plugin-activation x4 as
  drafted (Recommended)"_ — "Covers gate transparency, the narrowed allow,
  the filtered refusal, and Host-authority precedence over any profile."
- **Q3 Semantics** — approved: _"Confirm (Recommended)"_ — "Host authority
  dominates the profile at the per-id gate; refusals name the id and
  reason."
- **Q4 Contract** — approved: _"Approve C1-C6 as drafted (Recommended)"_ —
  "Freeze decision 53 and proceed to implement the gate, wiring, and v45
  reconciliation end to end."

## Resolution

**The Stage 5.7 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. Acceptance: gates
green, `composition-plugin-activation` ×4 at required parity at v45, the
authority-precedence property unit-proven, `check:rust` green, docs atomic.

**Implemented at `926ac71`** (2026-08-29): `siralos-core::composition` owns
`decide_plugin_activation` with Host-authority precedence and digest-bound
`PluginActivationEvidence`; `siralos-cli` holds the applied profile's selection
and `/domains-activate` refuses filtered ids before any side effect; corpus
v45/292 files, expectations 53 records, audit 287/287 applicable required;
docs at the follow-up docs commit.

## Self-loop verification

Ledger (criterion → evidence → verdict) recorded at record-complete in the map
and AGENTS.md; loop budget one pass + two repairs.
