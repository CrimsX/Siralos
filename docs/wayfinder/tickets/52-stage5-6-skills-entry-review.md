---
title: "Stage 5.6 Skills Entry Review - Declarative Digest-Bound Model Guidance"
label: "wayfinder:ticket"
type: HITL
status: closed
resolution: "PASS per decision 52 (2026-08-28): C1-C6 approved; implemented as the Stage 5.6 slice."
blockedBy: []
---

# Ticket 52 — Stage 5.6 Skills Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.6 Skills Entry Review](../decisions/52-stage5-6-skills-entry-review.md) (opens after HITL grilling)

## Why now

ADR 0036 §22 defines the Skill: reusable declarative guidance for model
reasoning with **no authority** — "Skill != Capability". §11 names Skill
version/digest as a lockable identity, and §23 forbids a registry,
marketplace, dependency resolver, or package server now. Stage 5 has the
composition primitives (profiles, the lock, plugin selection) to land the
skill model as the last major Stage 5 item.

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::skills` owns the model: `SkillDefinition` (bounded name, bounded content, content digest over the artifact-digest primitive), a bounded `SkillCatalog` (unique names, deterministic order), and `resolve_profile_skills(catalog, selected)` — selection binds a subset of the declared catalog (intersection; unknown names diagnosed, never bound) + digest-bound `SkillResolutionEvidence`. `siralos-adapters` owns `load_workspace_skills` reading `.siralos/skills/*.md` (lstat-verified regular files, byte-bounded, deterministic listing). No writes, no registry. |
| C2  | Authority invariant | A Skill is guidance only: resolution yields digest-bound references and never grants capability, filesystem, network, process, credentials, or host mutation — the evidence states the no-authority property mechanically ("Skill != Capability"). Selection filters the declared catalog; it cannot introduce a skill the workspace does not declare.                                                                                                                                                                                                                                   |
| C3  | Posture             | Zero spawn; the only fs is bounded read-only catalog loading. Absent selection = typed none (skills are opt-in — nothing binds by default). Catalog resolution is deterministic and order-independent (same declarations in any order → same resolution digest).                                                                                                                                                                                                                                                                                                                         |
| C4  | Evidence            | Frozen differential subject `composition-skills` ×4 at v44 over the pure seam: (1) absent selection → none bound; (2) selection binds declared skills → digest-bound references; (3) selection names unknown skills → diagnostics, bound = intersection; (4) order independence → shuffled catalog declaration yields the identical resolution digest. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                 |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v43 → v44 (284 → 288 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (284 → 288; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                              |
| C6  | Lean guardrails     | No Skill Creator, no registry/marketplace/dependency resolver/package server, no /evolve, no network, no multi-agent machinery (ADR 0036 §23); no profile schema change in this slice (profile-selected wiring waits); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                                                                             |

## Open questions for HITL grilling

1. **Q1 Scope** — core skill model + adapters read-only catalog loader over `.siralos/skills/*.md`, differential over the pure seam, profile-schema wiring deferred?
2. **Q2 Subject matrix** — `composition-skills` ×4 as drafted (none / bound / unknown-diagnosed / order-independent)?
3. **Q3 Semantics** — confirm C2/C3: skills are opt-in (absent = none), selection only filters the declared catalog, evidence states no-authority mechanically?
4. **Q4 Approval** — approve C1–C6 as drafted?
