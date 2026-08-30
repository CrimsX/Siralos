---
title: "Stage 5.10 Session Skill Consumption Entry Review — Consumption of the Frozen Skill Seam"
label: "wayfinder:decision"
type: entry-review
status: accepted
date: 2026-08-29
ticket: "56"
supersedes: []
---

# Decision 56 — Stage 5.10 Session Skill Consumption Entry Review

**Ticket:** [Ticket 56 — Stage 5.10 Session Skill Consumption Entry Review](../tickets/56-stage5-10-session-skill-consumption-entry-review.md)
**Maps to:** [Siralos Roadmap](../siralos-roadmap.md)

## Question

Decision 52 froze the Skill seam (declarative, digest-bound model
guidance whose evidence binds `authority = none`) but nothing consumes
resolution at a session boundary: no profile `skills` key exists. How
does an applied profile opt in to guidance, and what is the frozen
evidence shape?

## Contract (C1–C6, approved 2026-08-29)

| #   | Clause              | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| --- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the pure session-level skill-consumption decision: `compose_skill_consumption(selection, catalog_state)` → typed outcome (`none` / `bound` / `unknown`) over the frozen 5.6 `resolve_profile_skills`, with digest-bound `SkillConsumptionEvidence` whose payload binds `authority = none`. `siralos-adapters::profile_config` gains the additive `[profile.skills]` array key (mirrors the `plugins` precedent); malformed entries → `ProfileDocumentError` → profile unapplied (5.2 semantics). `siralos-cli` owns the wiring: at startup (applied profile only) the session resolves the selection against the workspace catalog and surfaces the bound guidance in the system-instructions segment (Q2: bounded, deterministic, sorted by name; absent selection/catalog = byte-transparent, R7.5 preserved). |
| C2  | Authority invariant | Guidance only: consumption can never add capability, Tool, or permission; the evidence digest literally binds `authority = none`. Absent selection or absent catalog = byte-transparent session (R7.5 preserved); unknown names are listed truthfully, never silently ignored or invented.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| C3  | Posture             | Zero new fs write paths (the catalog is read through the unchanged 5.6 loader) and zero spawn; the decision is pure over in-memory inputs. No wall clock, nothing silently trusted.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| C4  | Evidence            | Frozen differential subject `composition-skill-consumption` ×4 at v48 over the pure seam: (1) absent selection → `none`, transparent; (2) selection + catalog → `bound`; (3) partial selection with unknown names → bound subset applies and unknown names are listed truthfully; (4) absent catalog → nothing binds. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v47 → v48 (300 → 304 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (300 → 304; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| C6  | Lean guardrails     | No new CLI commands; no skill schema change; no plugin/profile shape change beyond the additive key; no multi-agent machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

## HITL answers (2026-08-29, recorded verbatim)

- **Q1 Scope** — approved: _"Full scope as drafted (Recommended)"_ —
  "Core decision + profile key + session wiring + v48 subject —
  completes the ADR 0036 §6 composition unit."
- **Q2 Surfacing** — approved: _"System-instructions segment
  (Recommended)"_ — "The bound skills' guidance composes into the
  session's system-instructions projection segment — bounded,
  deterministic, sorted by name; absent selection/catalog stays
  byte-transparent (R7.5 preserved)."
- **Q3 Subject matrix** — approved: _"×4 as drafted (Recommended)"_ —
  "(1) absent selection → none, transparent; (2) selection + catalog →
  bound; (3) partial selection with unknown names → bound subset
  applies, unknown listed truthfully; (4) absent catalog → nothing
  binds."
- **Q4 Contract** — approved: _"Approve C1–C6 (Recommended)"_ — "Freezes
  decision 56; implementation authorized for this arc (gates, v48, docs
  atomic)."

**The Stage 5.10 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. Acceptance: gates
green, `composition-skill-consumption` ×4 at required parity at v48, the
guidance-only property unit- and session-proven, `check:rust` green, docs atomic.

## Self-loop verification

- Criterion: the frozen contract is complete and internally consistent
  with decisions 47–55 and ADR 0036's lean composition model.
- Evidence: this document; ticket 56 with the C1–C6 draft; the HITL
  answers above recorded verbatim.
- Verdict: **PASS** — the Stage 5.10 contract is frozen; implementation
  authorized.
