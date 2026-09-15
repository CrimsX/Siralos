---
title: "Multi-model evaluation run"
label: "wayfinder:ticket"
status: open
date: "2026-09-12"
supersedes: []
---

# Multi-model evaluation run

Scoped by [decision 178](../decisions/178-evaluation-rows-ruled.md) to close the last
unowned assurance row (HAR-045: "run equivalent evaluation tasks across multiple
models/providers where economically practical").

## Why this is small

Almost everything it needs already exists and is proven:

- real providers and profile selection (decisions 66-71);
- provider usage capture into recordings (decision 102 P2) and the bounded recordings
  store (decisions 70, 73-78);
- the comparison over recorded requests — per-model bias, spread, bytes-per-token fit,
  cached-token ratio — in `crates/siralos-core/src/projection/calibration.rs`,
  explicitly labeled INFORMATIONAL (decision 94);
- the `/evolve` corpus and workflow surfaces (Stage 6, decisions 58-59).

What has never happened is a RUN across more than one model. Decision 102 says it
plainly: "real-usage evidence awaits a live recorded session with a provider that
records usage".

## Contract (frozen)

**Subjects.** One bounded task set, fixed and digest-bound, drawn from the `/evolve`
corpus. It is executed once per configured provider/model combination, at least two.
Each run emits ONE bounded outcome record: turns, tool rounds, provider usage where
reported, typed failures, cancellation, wall time.

**Evidence.** The per-run records plus ONE comparison table over them. Informational
only: it never gates, never changes a threshold, never reruns a benchmark (decision
94's rule). Token-count comparisons stay grouped per model, because token counts are
model-specific.

**Bounds.** An explicit per-run turn budget; an explicit opt-in (nothing runs at
startup); no credential, endpoint or workspace-path material in any record or report
(the decision 70 §4 hygiene contract plus the existing sanitizer boundary); no new
persistence — the records are files the owner names.

**CI strategy.** The offline path must prove the harness inside `npm run check`: the
deterministic fake plus a checked-in replay recording standing in for a second model,
producing two records and a comparison deterministically and without network. The LIVE
path is owner-run and never in the gate, because it spends real budget.

## Acceptance

1. The offline path runs two "models" (fake + replay) end to end and produces two
   outcome records plus one comparison table, asserted deterministically in the gate.
2. A live run is a documented, opt-in command with a bounded budget that produces the
   same shapes from real providers — demonstrated once by the owner and recorded.
3. No secret, endpoint or absolute path appears in any produced record (proven by the
   existing hygiene sweep plus a focused test).
4. Nothing in the feature gates, scores or blocks a session; the label stays
   INFORMATIONAL.

## Out of scope

- Longitudinal storage or trend dashboards (that is HAR-046 and the uncommitted
  persistence).
- Model routing or automatic model selection (HAR-022 stays NOT DUE; ADR 0036).
- Any change to the estimator, the thresholds or the benchmark record.
