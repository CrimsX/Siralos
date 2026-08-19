---
title: "R12 Disposition — Retirement vs Retention Evidence Template"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/07-r12-disposition.md
blockedBy: ["06-r11-gate.md"]
---

## Question

Decide the **R12 retirement/retention disposition evidence template** per ADR 0032.

Resolve:

- What corpus + harness + runtime corpora evidence distinguishes "TypeScript reference retirement" from "explicit evidence-backed retention" (which audits, digests, replay corpora, and performance baselines must be filed)?
- Which status pointers update on disposition (`PROJECT_CONTEXT.md`, `RUST_MIGRATION.md`, `ROADMAP.md`, `README.md` badges) and who signs the disposition?
- Guardrails so R12 cannot be declared without passing R11 and the Stage-4 entry gate.

Thin decision/template, not the disposition itself.

Blocked by: `06-r11-gate.md`.
## Resolution

Closed — template frozen in [decisions/07-r12-disposition.md](../decisions/07-r12-disposition.md). Shared evidence 8 rows + retention 5-field vs retirement retained-audits; disposition execution is the future R12 commit (after R11) advancing 8 surfaces atomically. This was blockedBy 06, which closed before this close. This unblocks [Stage 4 Entry Sequence](../tickets/08-stage4-entry-sequence.md) — frontier now includes 08.
