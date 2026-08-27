---
title: "R12 Disposition Execution - Retirement vs Retention Verdict"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: "PASS — R12 Verified (retired) at 4bef901 (audit v31 e24f4bb, 231/231; decision 33)"
blockedBy: []
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

## HITL outcome 2026-08-27 — RETIREMENT

The human chose **retirement**: the TypeScript reference is archived as
historical oracle at `4bef901` (corpus v31 e24f4bb, 236 files, 231/231
applicable required, 4 skips). Every required surface is at parity, every
impossible effect still reports typed `unavailable`, and the 17-criteria
`stage4-entry-gate.md` is now 17/17 PASS. The Rust candidate is the sole
behavioral source of truth per ADR 0032; fixing a bug now means fixing
`crates/**`. Retained audit artifacts remain by SHA: `audit.json`
(231/231), `corpus/` digests, and the gate. This closes R12 and unblocks
Stage 4 (generic Controlled Runtime Execution first). Recorded in
[decision 33](../decisions/33-r12-disposition-execution.md).

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
