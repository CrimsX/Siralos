---
title: "Stage 5.6 Skills Entry Review - Declarative Digest-Bound Model Guidance"
label: "wayfinder:decision"
date: 2026-08-28
status: accepted
ticket: "Stage 5.6 Skills Entry Review"
adr: "ADR 0036"
---

# Decision 52 — Stage 5.6 Skills Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Ticket:** [Stage 5.6 Skills Entry Review](../tickets/52-stage5-6-skills-entry-review.md)

## Question on the table

Does Stage 5.6 land the ADR 0036 §22 Skill — reusable declarative guidance
for model reasoning with **no authority** — as a bounded, digest-bound,
order-independent catalog with opt-in selection, and what is the frozen
evidence shape?

## Contract (C1–C6, approved 2026-08-28)

| #   | Clause              | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::skills` owns the model: `SkillDefinition` (bounded name, bounded content, content digest over the artifact-digest primitive), a bounded `SkillCatalog` (unique names, deterministic order), and `resolve_profile_skills(catalog, selected)` — selection binds a subset of the declared catalog (intersection; unknown names diagnosed, never bound) + digest-bound `SkillResolutionEvidence`. `siralos-adapters` owns `load_workspace_skills` reading `.siralos/skills/*.md` (lstat-verified regular files, byte-bounded, deterministic listing). No writes, no registry. |
| C2  | Authority invariant | A Skill is guidance only: resolution yields digest-bound references and never grants capability, filesystem, network, process, credentials, or host mutation — the evidence states the no-authority property mechanically ("Skill != Capability"). Selection filters the declared catalog; it cannot introduce a skill the workspace does not declare.                                                                                                                                                                                                                                   |
| C3  | Posture             | Zero spawn; the only fs is bounded read-only catalog loading. Absent selection = typed none (skills are opt-in — nothing binds by default). Catalog resolution is deterministic and order-independent (same declarations in any order → same resolution digest).                                                                                                                                                                                                                                                                                                                         |
| C4  | Evidence            | Frozen differential subject `composition-skills` ×4 at v44 over the pure seam: (1) absent selection → none bound; (2) selection binds declared skills → digest-bound references; (3) selection names unknown skills → diagnostics, bound = intersection; (4) order independence → shuffled catalog declaration yields the identical resolution digest. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                 |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v43 → v44 (284 → 288 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (284 → 288; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                              |
| C6  | Lean guardrails     | No Skill Creator, no registry/marketplace/dependency resolver/package server, no /evolve, no network, no multi-agent machinery (ADR 0036 §23); no profile schema change in this slice (profile-selected wiring waits); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                             |

## HITL answers (2026-08-28, recorded verbatim)

- **Q1 Slice scope** — approved: _"Core + adapters loader, profile wiring
  deferred (Recommended)"_ — "The skill model, catalog bounds, and selection
  semantics freeze and hold evidence first; profile schema extension waits."
- **Q2 Subject matrix** — approved: _"composition-skills x4 as drafted
  (Recommended)"_ — "Covers opt-in default, digest-bound binding, truthful
  diagnostics, and deterministic resolution."
- **Q3 Semantics** — approved: _"Confirm (Recommended)"_ — "A skill can never
  grant filesystem, network, process, credentials, or host mutation -
  binding only references declared content."
- **Q4 Contract** — approved: _"Approve C1-C6 as drafted (Recommended)"_ —
  "Freeze decision 52 and proceed to implement the skill model, catalog
  loader, and v44 reconciliation end to end."

## Resolution

**The Stage 5.6 implementation slice is authorized as the next implementation slice**
against this frozen contract and the 4 HITL answers above. **Implemented at
`fcf61c5`** (corpus v44/288 files, expectations 49 records, audit 283/283
applicable required; order-independence unit- and corpus-proven). Acceptance:
gates green, `composition-skills` ×4 at required parity at v44,
order-independence unit-proven, `check:rust` green, docs atomic.

## Self-loop verification

Ledger (criterion → evidence → verdict) recorded at record-complete in the map
and AGENTS.md; loop budget one pass + two repairs.
