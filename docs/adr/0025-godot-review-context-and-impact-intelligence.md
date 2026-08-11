---
id: ADR-0025
status: accepted
domains: [godot, impact, review, executor-briefing]
paths:
  [
    packages/core/src/godot/impact/**,
    packages/adapters/src/godot/intelligence/**,
    packages/adapters/src/godot/tools/**,
  ]
supersedes: []
---

# ADR 0025 — Godot Review Context and Impact Intelligence

- Status: accepted (Stage 3 milestone 9)
- Date: current milestone
- Related: ADR 0021 (read-only scene/resource intelligence), ADR 0020
  (planning), ADR 0013 (quality gates and independent review), ADR 0022–0024
  (executor context and scoped documentation)

## Problem

The independent reviewer and the planner receive a changed GDScript
file plus its diff, but no bounded picture of what the change can affect
and what should be validated. Deriving that by hand (or by loading the
whole project) either wastes context or misses related scenes,
resources, signal endpoints, autoload reach, and likely regression
surfaces. Stage 3.8 provided revision-aware scene/resource relationships;
this milestone turns them into bounded, evidence-backed impact and
validation context.

## Decision

Introduce a structured `ReviewContextManifest` plus a deterministic
impact analyzer in `packages/core/src/godot/impact/`, wired into the
existing scene-intelligence service and the independent reviewer:

- The manifest distinguishes `primaryChanges` (changed surfaces) from
  `relatedSurfaces` (ImpactRelation: script attachment, scene
  inheritance, scene instancing, resource/script dependency, serialized
  signal connection, autoload global reach, candidate test coverage),
  each with revisions, confidence (`verified` vs `candidate`), and
  evidence references. It carries `regressionAreas` (evidence-backed,
  never generic), structured `validation` recommendations with priority
  (`required_now` / `recommended` / `runtime_evidence_unavailable`),
  bounded `evidence`, honest `completeness` (`complete` / `bounded` /
  `partial`), and `diagnostics`.
- The analyzer is PURE and deterministic: it derives impact only from an
  injected `ImpactRelationshipSource` implemented by the adapter over
  the existing S3.8 relationship index, the workspace revision registry,
  the static project scan, and bounded scene parsing. The analyzer owns
  no relationships, no revisions, no filesystem, and no process.
- Traversal is breadth-first, cycle-safe, and bounded (depth, surface
  count, relation count); boundary relations are recorded but not
  expanded, and bound hits are disclosed via `IMPACT.TRAVERSAL_BOUND`
  and a `bounded`/`partial` completeness. Stale relationships (recorded
  source revision no longer current) are excluded from current impact
  and disclosed via `IMPACT.STALE_RELATIONSHIP` — never presented as
  current.
- Verified vs candidate impact is preserved: index-backed relations are
  `verified`; convention-based test discovery (colocated `*.test.gd`/
  `*.spec.gd` or a `tests` tree matching the changed stem, bounded) is
  always `candidate` and never promoted silently.
- Autoload changes are a high-reach risk signal: `IMPACT.AUTOLOAD_GLOBAL`
  plus a broader-repository-validation recommendation — never a claim
  that every project surface is verified impacted. Signal connections
  are surfaced from serialized scene data with a `runtime_validation`
  recommendation at `runtime_evidence_unavailable`, because runtime
  connection validity cannot be proven statically.
- The service exposes one read-only tool, `godot.review_context`
  (bounded output, completeness disclosed, no mutation/process), and the
  development workflow derives a bounded manifest from the changed
  surfaces before independent review (`ChangeReviewRequest.reviewContext`).
  The planner may consume impact context read-only; impact intelligence
  is a capability, never a mandatory stage for `none`-routed tasks.

## Rejected

- **Repository-wide eager dependency graph** — traversal is lazy,
  bounded, per-query; the S3.8 index remains the single derived
  relationship owner.
- **Loading all related source into reviewer context** — the reviewer
  receives the bounded manifest (references + reasons), never whole
  related files.
- **Treating absence of a static dependency as proof of no impact** —
  runtime validation is recommended at `runtime_evidence_unavailable`
  whenever signal/autoload reach is involved.
- **Treating heuristic test matches as verified coverage** — candidate
  tests stay `candidate` with `convention:` evidence.
- **Runtime execution as part of static impact analysis** — no Godot
  process, no project code, no mutation, no checkpoint.
- **Generic "test everything" validation recommendations** — every
  recommendation cites the observed impact that produced it.
- **Impact analysis as a mandatory workflow stage** — planning depth
  stays host-decided; trivial tasks never invoke it.

## Consequences

- The reviewer and planner get a concise, revision-bound picture of what
  a change can affect and what to validate, without project-wide context
  flooding.
- Impact claims are evidence-backed by construction: verified impact
  requires current relationship evidence; everything else is candidate
  or disclosed uncertainty.
- The impact analyzer falls under the existing executor/scene
  architecture checks (no capability, security, mutation, or process
  surface) and reuses the S3.8 index, the revision registry, and the
  quality-stage reviewer — no parallel relationship store, revision
  system, or review pipeline was introduced.
