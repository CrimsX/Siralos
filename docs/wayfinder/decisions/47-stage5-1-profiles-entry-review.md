# Decision — Stage 5.1 Profiles Entry Review — Named Declarative Working Configurations over a Narrowing-Only Boundary

**Wayfinder ticket:** [Stage 5.1 Profiles Entry Review](../tickets/47-stage5-1-profiles-entry-review.md) · label `wayfinder:ticket` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4 Verified Roll-Up](46-stage4-verified-roll-up.md) (Stage 4 Verified at `9566eee`)
**Decided:** 2026-08-28 (resolver session; HITL grilling over ticket 47's C1–C6 draft and the 4 open frontier questions)
**Status:** **PASS — Stage 5.1 contract frozen; authorized as next implementation slice**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> First Stage 5 slice. Graduates Composition into the map with Profiles — the composition unit per [ADR 0036](../../../docs/adr/0036-lean-product-composition-and-extension-model.md) §6: "a named declarative AI working configuration" whose "lower-authority configuration may narrow behavior [and] may never broaden Host authority."

---

## Summary

HITL confirmed **Stage 5.1 — Profiles**: the generic Profile model lands in `siralos-core::composition` (named record, bounded fields, permission-policy overlay), bounded parsing extends `siralos-adapters::config`, and the composition surface composes in `siralos-cli` — with no CLI command and no `siralos.lock` in this slice (Q1). The security property is narrowing-only (Q3/C2): an overlay entry is legal iff it is not broader than the Host's current rule for that capability (Deny < Ask < Allow); a widening request is a typed refusal, never a silent clamp. Zero-configuration stays valid: an absent profile resolves to a typed default. Frozen differential subject `composition-profile` ×4 at corpus v39 (Q2/C4). No Stage-4 boundary flips.

## 1. HITL answers (2026-08-28)

| #   | Frontier question | Human answer                                                                                          |
| --- | ----------------- | ----------------------------------------------------------------------------------------------------- |
| 1   | Slice scope       | **Model + overlay, no CLI/lockfile**                                                                  |
| 2   | Subject matrix    | **`composition-profile` ×4 as drafted** (valid / widening-refused / bounds-violated / absent-default) |
| 3   | Posture           | **Confirm** C2+C3 — narrowing-only, zero-config valid, pure data                                      |
| 4   | Contract approval | **Approve C1–C6 as drafted**                                                                          |

## 2. Frozen contract (C1–C6, confirmed)

| #   | Clause                   | Contract                                                                                                                                                                                                                                                                                                                                                            |
| --- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership                | Generic Profile model (named record: id/name bounds, permission-policy overlay, bounded selection fields) in `siralos-core::composition`; bounded parsing/validation extends `siralos-adapters::config`; the composition surface composes in `siralos-cli`. No Skills/Plugins/lockfile consumption in this slice.                                                   |
| C2  | Narrowing-only           | An overlay entry is legal iff it is not broader than the Host's current rule for that capability; a widening request is a typed refusal. A Profile never itself grants authority (ADR 0036 §6: profile request → resolution → Host policy → effective run configuration).                                                                                           |
| C3  | Determinism, zero-config | Profiles are pure declarative data: bounded parse (bytes/entry caps), deterministic validation ordering, no network/spawn/live probing; an absent profile resolves to a valid typed default (zero-configuration UX stays first-class).                                                                                                                              |
| C4  | Evidence                 | Frozen differential subject `composition-profile` ×4 at v39: (1) valid minimal profile → resolved record; (2) authority-widening overlay → typed narrowing refusal; (3) bounds/unknown-field violation → typed validation failure; (4) absent profile → valid default resolution. New scenarios covered by the post-freeze expectations mechanism (decision 40 C7). |
| C5  | Corpus mechanics         | Schema stays 3; corpus bumps v38 → v39 (264 → 268 files, inside the 384 cap); all four contract.mjs sites, the protocol validator, and the strict-loader assert move together per the established checklist.                                                                                                                                                        |
| C6  | Lean guardrails          | No `siralos.lock` generation, no Skills/Plugin references resolved, no multi-agent machinery, no auto-acquisition, no approval-authority change (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                   |

**The Stage 5.1 implementation slice is authorized as the next implementation slice** against this frozen contract and the 4 HITL answers above. **Implemented at `be030e3`** (corpus v39/268 files, expectations 29 records, audit 263/263 applicable required; zero spawn paths preserved). Acceptance: gates green, `composition-profile` ×4 at required parity at v39, narrowing-only enforced mechanically with adversarial coverage, `check:rust` green, docs atomic.

## Self-loop verification

| Criterion                                 | Direct evidence                                                                                        | Status |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------ | ------ |
| Grounded in ADR 0036, not new scope       | §6 Profile definition + narrowing clause quoted; §8 portable shape deferred (no lockfile this slice)   | pass   |
| Security property mechanical, adversarial | C4 case 2 is an adversarial widening attempt with a typed refusal; case 3 bounds; unit tests to mirror | pass   |
| Existing seams reused                     | R7.4 `siralos-adapters::config` + R7.2 capability/permission evaluation; no parallel authority system  | pass   |
| Lean guardrails explicit                  | C6; no lockfile, no Skills/Plugins, no acquisition; corpus cap respected (268 ≤ 384)                   | pass   |
