---
title: "The Evaluation Rows: Two Retirements and One Scoped Slice"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "135"
supersedes: []
---

# The Evaluation Rows: Two Retirements and One Scoped Slice

[Ticket 135](../tickets/135-multi-model-evaluation-run.md) ·
[REQUIREMENTS](../../requirements/REQUIREMENTS.md) ·
[decision 102](102-provider-interface-takeaways.md) · [Map](../siralos-roadmap.md)

## 1. Why these rows were unowned

The ticket 131 reconciliation left three assurance rows saying `UNOWNED -- retire or
ticket`: HAR-044 (maintain an adversarial evaluation suite), HAR-045 (equivalent
evaluation across multiple models/providers) and HAR-051 (documentation/RFC/schema
drift detected during review). Each had a plausible-sounding reason that no decision
actually made. This record makes the calls.

## 2. HAR-044 — retired into the persistence boundary

The adversarial evaluation that matters at this size **exists**: unit, behavior,
differential (352/352 applicable required) and domain-conformance suites, plus the
Stage 6 `/evolve` surfaces (decisions 58-59). What the row's open part actually
describes is a LONGITUDINAL program — metrics over time — and that is HAR-046, whose
storage is the persistence the repository deliberately does not commit (ADR 0036).
So: not unowned, not pending; the open part is a boundary-owned row.

**Trigger to revisit:** the persistence decision changing, or a measured need for
trend data rather than the current in-memory metrics (decision 91).

## 3. HAR-051 — retired, with the sweep recorded instead of a check

The mechanical drift classes ARE checked (`check:docs`, `check:context`,
`check:identity`, `check:public`, the architecture gate, the differential). What the
sweep found is a different class: **status drift** — a register saying a milestone is
pending when the record says it is Verified. No check can settle that, because it is a
judgement about prose against a decision history; that is exactly why it was found by
hand. Retired as a requirement on tooling, replaced by a recorded recipe:

1. sweep the marker families (`PARTIAL`, `NOT DUE`, `deferred`, `not yet`, `FUTURE`,
   `BACKLOG`, `awaits`, `unowned`, `remains`) across `docs/**` and the root files;
2. resolve every hit against the tracker (map, tickets, decisions) by grep;
3. classify each as boundary / not-committed / stale / unowned, and fix the unowned
   ones by retiring or ticketing them.

That recipe was executed twice on 2026-09-12 (tickets 131 and the erratum pass) and is
cheap to repeat; the registers now carry dated `Reconciliation` notes so the next
sweep starts from a known state.

## 4. HAR-045 — scoped into a frozen-contract slice

This is the row with real value now: real providers are Verified (decisions 66-71),
usage capture and the recordings store exist (decisions 70, 78, 102), and the
comparison machinery is already built and unit-proven
(`crates/siralos-core/src/projection/calibration.rs` — per-model bias, spread,
bytes-per-token and cache-ratio over recorded requests, explicitly labeled
INFORMATIONAL per decision 94). What has never happened is a RUN across more than one
model; decision 102 says so itself: "real-usage evidence awaits a live recorded
session with a provider that records usage".

So the slice is the run, not the machinery — contract frozen in
[ticket 135](../tickets/135-multi-model-evaluation-run.md):

- **Subjects:** one bounded task set (drawn from the `/evolve` corpus, fixed and
  digest-bound) executed once per configured provider/model combination, at least
  two; each run produces one bounded outcome record (turns, tool rounds, provider
  usage where reported, failures, cancellation, wall time).
- **Evidence:** the per-run records plus ONE comparison table over them, informational
  only — it never gates, never changes a threshold, and never reruns a benchmark
  (decision 94's rule).
- **Bounds:** an explicit per-run turn budget, an explicit opt-in, no credential or
  endpoint material in any record or report (the recordings hygiene contract from
  decision 70 §4), and no new persistence: records are files the owner names.
- **CI:** the offline path (the deterministic fake plus a checked-in replay recording
  standing in for a second model) must prove the harness and the comparison
  deterministically inside `npm run check`; the LIVE path is owner-run and never in
  the gate, because it spends budget.

## 5. Evidence

- The retrieval behind each ruling: `crates/siralos-core/src/projection/calibration.rs`
  (the comparison), decisions 94/102 (its informational status and the missing run),
  and the check inventory in `npm run check` (what HAR-051 already covers).
- The register rows now cite this decision, and HAR-045 additionally cites ticket 135.
- Documentation-only: no code, no capability and no corpus changed by this record.
