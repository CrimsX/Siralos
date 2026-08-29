---
title: "Stage 5.1 Profiles Entry Review - Named Declarative Working Configurations over a Narrowing-Only Boundary"
label: "wayfinder:ticket"
type: HITL
status: open
resolution: ""
blockedBy: []
---

# Ticket 47 — Stage 5.1 Profiles Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.1 Profiles Entry Review](../decisions/47-stage5-1-profiles-entry-review.md) (opens after HITL grilling)

## Why now

Stage 4 is Verified (`9566eee`) and the map's Not-yet-specified section is empty. HITL
selected Stage 5.1 — Profiles as the next frontier (2026-08-28): the composition unit
per [ADR 0036](../../../docs/adr/0036-lean-product-composition-and-extension-model.md)
§6 — "a named declarative AI working configuration" whose "lower-authority
configuration may narrow behavior [and] may never broaden Host authority."
Existing seams: `siralos-adapters::config` (R7.4 bounded config parsing) and
`siralos-core::tool` capability/permission evaluation (R7.2).

## Contract draft (C1–C6)

| #   | Clause                         | Draft                                                                                                                                                                                                                                                                                                                                                               |
| --- | ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership                      | The generic Profile model (named record: id/name bounds, permission-policy overlay, bounded selection fields) lands in `siralos-core::composition`; bounded parsing/validation extends `siralos-adapters::config`; the composition surface composes in `siralos-cli`. No Skills/Plugins/lockfile consumption in this slice.                                         |
| C2  | Narrowing-only boundary        | A profile's permission overlay may only narrow Host policy (allow-set intersection); a profile requesting authority the Host does not grant is a typed refusal — a Profile never itself grants authority (ADR 0036 §6 resolution chain: profile request → resolution → Host policy → effective run configuration).                                                  |
| C3  | Determinism, zero-config valid | Profiles are pure declarative data: bounded parse (bytes/entry caps), deterministic validation ordering, no network/spawn/live probing; the absence of a profile resolves to a valid typed default (zero-configuration UX stays first-class).                                                                                                                       |
| C4  | Evidence                       | Frozen differential subject `composition-profile` ×4 at v39: (1) valid minimal profile → resolved record; (2) authority-widening overlay → typed narrowing refusal; (3) bounds/unknown-field violation → typed validation failure; (4) absent profile → valid default resolution. New scenarios covered by the post-freeze expectations mechanism (decision 40 C7). |
| C5  | Corpus mechanics               | Schema stays 3; corpus bumps v38 → v39 (264 → 268 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader assert move together per the established checklist.                                                                                                                                                            |
| C6  | Lean guardrails                | No `siralos.lock` generation, no Skills/Plugin references resolved, no multi-agent machinery, no auto-acquisition, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                   |

## Open questions for HITL grilling

1. **Q1 Slice scope** — parse/validate/resolve model with the narrowing-only overlay, no CLI command yet?
2. **Q2 Subject matrix** — `composition-profile` ×4 as drafted (valid / widening-refused / bounds-violated / absent-default)?
3. **Q3 Posture** — confirm C2+C3: narrowing-only overlays, zero-config valid, no network/spawn, no authority broadening?
4. **Q4 Approval** — approve C1–C6 as drafted?
