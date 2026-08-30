---
title: "Stage 5.10 Session Skill Consumption Entry Review"
label: "wayfinder:ticket"
type: HITL
status: closed
resolution: "PASS per decision 56 (2026-08-29): C1-C6 approved; implemented as the Stage 5.10 slice."
blockedBy: []
---

# Ticket 56 — Stage 5.10 Session Skill Consumption Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.10 Session Skill Consumption Entry Review](../decisions/56-stage5-10-session-skill-consumption-entry-review.md) (opens after HITL grilling)

## Why now

Decision 52 froze the Skill seam — declarative, digest-bound model
guidance whose evidence literally binds `authority = none` — but
nothing consumes resolution at a session boundary: no profile `skills`
key exists, so an applied profile cannot opt in to guidance. This slice
completes the ADR 0036 §6 composition unit (the last unconsumed Stage 5
seam before a roll-up can close clean).

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the pure session-level skill-consumption decision: `compose_skill_consumption(selection, catalog_state)` → typed outcome (`none` / `bound` / `unknown`) over the frozen 5.6 `resolve_profile_skills`, with digest-bound `SkillConsumptionEvidence` whose payload binds `authority = none`. `siralos-adapters::profile_config` gains the additive `[profile.skills]` array key (mirrors the `plugins` precedent); malformed entries → `ProfileDocumentError` → profile unapplied (5.2 semantics). `siralos-cli` owns the wiring: at startup (applied profile only) the session resolves the selection against the workspace catalog and surfaces the bound guidance. |
| C2  | Authority invariant | Guidance only: consumption can never add capability, Tool, or permission; the evidence digest literally binds `authority = none`. Absent selection or absent catalog = byte-transparent session (R7.5 preserved); unknown names are listed truthfully, never silently ignored or invented.                                                                                                                                                                                                                                                                                                                                                                                                           |
| C3  | Posture             | Zero new fs write paths (the catalog is read through the unchanged 5.6 loader) and zero spawn; the decision is pure over in-memory inputs. No wall clock, nothing silently trusted.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| C4  | Evidence            | Frozen differential subject `composition-skill-consumption` ×4 at v48 over the pure seam: (1) absent selection → `none`, transparent; (2) selection + catalog → `bound`; (3) partial selection with unknown names → bound subset applies and unknown names are listed truthfully; (4) absent catalog → nothing binds. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                                                                                                                                                              |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v47 → v48 (300 → 304 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (300 → 304; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                                                                                                                                          |
| C6  | Lean guardrails     | No new CLI commands; no skill schema change; no plugin/profile shape change beyond the additive key; no multi-agent machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

## Open questions for HITL grilling

1. **Q1 Scope** — composition consumption decision + additive `[profile.skills]` key + session wiring + differential `composition-skill-consumption` ×4 at v48?
2. **Q2 Surfacing** — where does bound guidance reach the model?
3. **Q3 Subject matrix** — the four cases as drafted?
4. **Q4 Approval** — approve C1–C6 as drafted and authorize the implementation slice?
