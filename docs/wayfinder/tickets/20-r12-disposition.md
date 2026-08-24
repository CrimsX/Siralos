---
title: "R12 Disposition Execution - Retirement vs Retention Verdict"
label: "wayfinder:grilling"
type: HITL
status: open
resolution: null
blockedBy: ["22-r13-execution-register.md"]
---

## Question

Per the frozen template ([R12 Disposition](../decisions/07-r12-disposition.md)),
issue the R12 verdict: **retirement** or **retention** of the TypeScript
reference — backed by the shared evidence package and, for retention,
the five-field block (retained surface list + typed reason per surface +
re-evaluation trigger + review date + status-surface note).

## HITL outcome 2026-08-24 — DEFERRED

The human chose **defer - port surfaces first**: the R12 verdict is not
takeable while TypeScript Stage-3 surfaces remain unported. This ticket
is blocked by the [R13 Execution Register](22-r13-execution-register.md);
when the ported slices land, re-present the evidence package here.

Evidence state at ticket open (R11 Verified at `eea0029`):

- Shared rows already satisfied: per-platform digest-bound audits,
  corpus v23 manifest, replay stress on three platforms, fail-closed
  parity subjects, `recovery-taxonomy`, truthful sandbox transcripts,
  performance baseline + no-known-regression, gate criteria 7-10.
- Decisive fact: the Rust candidate covers task/workspace/language/
  domain/provider/tool/projection/config/godot/identity/determinism/
  context/runtime. **Unported TypeScript Stage-3 surfaces** (no Rust
  differential subject): project instructions, project knowledge,
  references, research, host-routed planning, executor briefing
  (ADR 0022-23), self-reference/capability doctor, command catalog +
  security policy surface, approval flow, and the full interactive CLI
  product layer (`apps/cli`).

Retirement therefore appears unavailable without first porting (or
explicitly de-scoping) those surfaces; retention requires the five-field
block with typed reasons. The human decides; the agent never answers for
them (template guardrail h).
