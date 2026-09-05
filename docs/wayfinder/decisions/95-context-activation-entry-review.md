---
title: "The Context System Activation Entry Review"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "68"
supersedes: []
---

# 95 — The Context System Activation Entry Review

Governing plan 68 · entry review [79](79-context-management-foundations.md) · Map.

> **User-directed 2026-08-31 (session HITL): C1–C6 approved as drafted, following the externally-discussed optimization plan (decisions 93/94 record its resolutions). The decision 84 precondition gate is consumed by this review and authorized as the next implementation arc; nothing activates without the slices below.**

## 2. Approved criteria (C1–C6) — as presented to and approved by the user 2026-08-31

- C1 Scope: B1 bounded read-only graph reconstruction over existing digest/staleness seams; B2 node-taxonomy review as its own HITL slice (which node kinds, per-kind bounds); B3 session wiring behind an additive [profile.context] key, default OFF (absent key = byte-transparent; narrowing-only); B4 the decision 91 ring and counters as the live audit trail.
- C2 Safety: no persistence (frozen clause c); tools stay read-only; demand events and search scores are host-observed only; the decision 84 precondition gate is consumed by this review and nothing else; the benchmark flows are untouched (the decision 93 byte-identity guard extends to all activation work).
- C3 Adversarial invariants REQUIRED per slice as pass/fail fixtures with no promise to measure later: spam-pinning resistance (sustained garbage demand cannot breach the 1024 pinned-HOT quota, cannot promote stale nodes, cannot grow the tick ring beyond 64 records); churn boundedness (oscillating demand cannot exceed one disposition per node per tick and cannot violate the 4096 unique-digest budget in any assembled output); neighbor-stub honesty under fanout (an adversarial high-fanout hot node still assembles at most 8 stubs, node_id ascending, ids+digests only, stub bytes counted in the budget with deterministic over-budget demotion). Activation does not proceed on a failed invariant.
- C4 Evidence: every slice corpus-pinned with audit all-applicable-required and expectation records updated in place; the context-benchmark record byte-identical throughout.
- C5 Sequence: B1 -> B2 -> B3 -> B4, each with its own decision record; the node taxonomy (B2) is an authority decision and gets its own review before any session wiring consumes it.
- C6 Out of scope: frozen clause (d) and (e) exceptions, persistence of any kind, default-on activation, neighbor content prefetch (deferred until a live loop can measure it), and any model-injectable demand or score surface.

## 3. Criteria → evidence

| Criterion                 | Authorization source                                                                                                                                                                                                 | Status |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| C1 Scope                  | Presented draft approved by the user 2026-08-31 (B1–B4 as scoped); the externally-discussed optimization plan recorded in [93](93-score-ordered-retention.md) / [94](94-sensitivity-sweep-rule.md)                   | pass   |
| C2 Safety                 | Presented draft approved by the user 2026-08-31 (no persistence, read-only tools, host-observed demand); [93](93-score-ordered-retention.md) byte-identity guard extends to all activation work                      | pass   |
| C3 Adversarial invariants | Presented draft approved by the user 2026-08-31; adversarial invariants from the external discussion round 2 (spam-pinning resistance, churn boundedness, neighbor-stub honesty under fanout as pass/fail fixtures)  | pass   |
| C4 Evidence               | Presented draft approved by the user 2026-08-31; corpus-pinned audit and expectation update discipline from the externally-discussed plan ([93](93-score-ordered-retention.md) / [94](94-sensitivity-sweep-rule.md)) | pass   |
| C5 Sequence               | Presented draft approved by the user 2026-08-31 (B1 -> B2 -> B3 -> B4, B2 as authority HITL); externally-discussed plan sequencing                                                                                   | pass   |
| C6 Out of scope           | Presented draft approved by the user 2026-08-31 (frozen clause d/e, persistence, default-on, prefetch, model-injectable score); externally-discussed plan out-of-scope pin                                           | pass   |

## 4. Result

Entry review PASS: B1 (bounded read-only workspace scan) is authorized as the first activation slice; B2's taxonomy review returns to HITL before B3 wires anything; the adversarial invariants gate every slice; the fail-closed posture is untouched.
