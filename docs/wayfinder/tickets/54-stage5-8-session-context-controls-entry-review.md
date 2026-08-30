---
title: "Stage 5.8 Session Context Controls Entry Review"
label: "wayfinder:ticket"
type: HITL
status: open
resolution: ""
blockedBy: []
---

# Ticket 54 — Stage 5.8 Session Context Controls Entry Review

**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Decision:** [Stage 5.8 Session Context Controls Entry Review](../decisions/54-stage5-8-session-context-controls-entry-review.md) (opens after HITL grilling)

## Why now

Decision 49 froze the pure Live/Pinned/Frozen control model and explicitly
deferred consumption surfaces to their own slices. The consumer now exists:
the session composes the applied workspace profile at startup (decision 48)
and renders deterministic projection snapshots through `/context` (R7.5,
byte-equal rubric). This slice completes the loop — the applied profile's
context control narrows what the session claims about content — and the
composition-level decision becomes a differential subject. Skill binding
still waits for a skill consumption surface (no profile `skills` key yet).

## Contract draft (C1–C6)

| #   | Clause              | Draft                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| --- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Ownership           | `siralos-core::composition` owns the composition-level seam: the effective policy carries the applied profile's context control (default `Live`), plus pure decision/evidence/rendering helpers over the frozen 5.3 `evaluate_context_policy`. `siralos-adapters::profile_config` parses the additive `[profile.context]` key (kind + optional digest) through the 5.3 constructor; an invalid control makes the profile not applied (5.2 semantics). `siralos-cli` owns the wiring: the session holds the applied control and the `/context` surface evaluates it against the snapshot's content identity. |
| C2  | Authority invariant | Narrowing-only per decision 49 C2: a control can only narrow what Siralos claims about content — `Live` asserts nothing; `Pinned`-stale proceeds but is labelled stale with expected/actual; `Frozen`-stale refuses the claim use (typed refusal, truthful report-safe diagnostics). Absent key = `Live` (transparent, zero-config); an invalid/refused profile contributes no control (5.2 semantics).                                                                                                                                                                                                     |
| C3  | Posture             | Zero new fs write paths and zero spawn; evaluation is pure over in-memory digests. The snapshot identity is a deterministic digest over existing render state — no new I/O, no wall clock, nothing silently used under `Blocked`.                                                                                                                                                                                                                                                                                                                                                                           |
| C4  | Evidence            | Frozen differential subject `composition-context-control` ×4 at v46 over the composition seam: (1) absent/unset → live, transparent fresh-unbound; (2) pinned current → fresh, digest-bound; (3) pinned stale → stale but usable with expected/actual; (4) frozen stale → blocked with expected/actual. Expectations mechanism covers the new scenarios (decision 40 C7).                                                                                                                                                                                                                                   |
| C5  | Corpus mechanics    | Schema stays 3; corpus bumps v45 → v46 (292 → 296 files, inside the 384 cap); all four contract.mjs sites, protocol validator, and the strict-loader count assert (292 → 296; run `cargo test -p siralos-cli --lib --all-features` before commit per the map Notes reminder) move together.                                                                                                                                                                                                                                                                                                                 |
| C6  | Lean guardrails     | Additive profile schema change limited to `[profile.context]` (mirrors decision 51's `plugins` precedent); no new CLI commands (the control rides the existing `/context`); no lockfile change, no plugin-consumption change, no skill consumption surface; no multi-agent machinery (ADR 0036); measurement per `RUST_STYLE.md`; map/AGENTS.md updated atomically at record-complete.                                                                                                                                                                                                                      |

## Open questions for HITL grilling

1. **Q1 Scope** — composition seam + additive `[profile.context]` parse + session/`/context` wiring + differential `composition-context-control` ×4 at v46?
2. **Q2 Binding target** — what the control evaluates against in the session: the deterministic `/context` snapshot identity (recommended) vs a startup-only disposition report with no render binding?
3. **Q3 Subject matrix** — the four cases as drafted (transparent live / pinned current / pinned stale / frozen stale)?
4. **Q4 Approval** — approve C1–C6 as drafted and authorize the implementation slice?
