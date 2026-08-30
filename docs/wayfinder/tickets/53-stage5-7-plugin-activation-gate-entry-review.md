---
title: "Stage 5.7 Session Plugin Activation Gate Entry Review"
label: "wayfinder:ticket"
type: HITL
status: closed
resolution: "PASS per decision 53 (2026-08-29): C1-C6 approved; implemented as the Stage 5.7 slice."
blockedBy: []
---

# Ticket 53 — Stage 5.7 Session Plugin Activation Gate Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.7 Session Plugin Activation Gate Entry Review](../decisions/53-stage5-7-plugin-activation-gate-entry-review.md) (opens after HITL grilling)

## Why now

Decision 51 froze the profile→plugin narrowing (activated = enabled ∩
selected) but deferred session consumption. The consumer now exists: the
interactive session's `/domains-activate` (decisions 38/39). This slice
completes the loop — the frozen filter gates real activation, and the
per-id gate becomes a differential subject. Skill binding waits until a
skill consumption surface exists (no profile `skills` key yet).

## Contract draft (C1–C6)

| #   | Clause               | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| --- | -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership            | `siralos-core::composition` owns the pure per-id gate: `decide_plugin_activation(enabled, selected, requested)` → typed outcome (`activated` / `refused-filtered` / `refused-not-enabled`) + digest-bound `PluginActivationEvidence` + deterministic rendering. `siralos-cli` owns the wiring: the session holds the applied profile's plugin selection at startup (only when the 5.2 composition actually applied) and `/domains-activate` consults the gate before install/enable/activate. |
| C2  | Authority precedence | Host authority is checked first: a requested id the Host has not enabled is refused regardless of any profile (a profile can never enable). The profile filter then refuses ids outside the selection before any install/enable/activate side effect. Refusals carry truthful, report-safe diagnostics naming the id and the reason.                                                                                                                                                          |
| C3  | Posture              | Zero new fs write paths and zero spawn; the gate is a pure decision over in-memory sets. Absent profile selection = gate transparent (session behaves exactly as before). Invalid/refused profiles contribute no selection (5.2 semantics).                                                                                                                                                                                                                                                   |
| C4  | Evidence             | Frozen differential subject `composition-plugin-activation` ×4 at v45 over the pure seam: (1) absent selection → enabled id activates; (2) narrowed → selected id activates; (3) narrowed → enabled-but-unselected id refused (`refused-filtered`); (4) Host precedence → un-enabled id refused (`refused-not-enabled`) even when selected. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                 |
| C5  | Corpus mechanics     | Schema stays 3; corpus bumps v44 → v45 (288 → 292 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (288 → 292; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                   |
| C6  | Lean guardrails      | No new CLI commands (the gate rides the existing `/domains-activate`); no profile schema change; no skill consumption surface; measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                          |

## Open questions for HITL grilling

1. **Q1 Scope** — core per-id gate + session wiring into `/domains-activate` + differential `composition-plugin-activation` ×4 at v45?
2. **Q2 Subject matrix** — the four cases as drafted (transparent / allowed / filtered / Host precedence)?
3. **Q3 Semantics** — confirm C2/C3: Host-authority check first, profile filter refuses before side effects, gate transparent without an applied profile selection?
4. **Q4 Approval** — approve C1–C6 as drafted and authorize the implementation slice?
