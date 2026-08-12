---
id: ADR-0029
status: accepted
domains: [determinism, task-runtime, security]
paths:
  [
    packages/core/src/determinism/**,
    scripts/check-nondeterminism.mjs,
    packages/core/src/doctor/doctor-model.ts,
    packages/core/src/doctor/capability-doctor.ts,
  ]
supersedes: []
---

# ADR 0029 — Deterministic Execution and Reproducibility

- Status: accepted (Stage 3 — Deterministic Execution & Reproducibility)
- Date: current milestone
- Related: ADR 0028 (content identity and semantic deltas), ADR 0014
  (task runtime), ADR 0020 (host-controlled planning), ADR 0022 (executor
  briefing), ADR 0027 (unified Godot-native development workflow)

## Problem

Authoritative host decisions (planning depth, approvals, validation
selection, acceptance, retry, reporting) could silently depend on
ambient wall-clock time, randomness, process environment, filesystem
enumeration order, locale/timezone, or concurrency completion order.
The LLM is probabilistic by nature; the host boundary around it should
not add avoidable nondeterminism of its own.

## Decision

Introduce explicit nondeterminism boundaries and deterministic host
decisions (`packages/core/src/determinism/**`):

- **Clock**: `Clock` port with an explicit system-clock seam and a fixed
  clock for tests/policy evaluation. Authoritative time-dependent
  decisions (leases, expiry, retry backoff) take the clock as input.
  Controlled time never means frozen production time.
- **Randomness**: seeded `RandomSource` (mulberry32) used only where
  randomness is genuinely part of the design; most host policy decisions
  need none. Unique entity IDs are not replaced merely because they are
  random.
- **OrderingPolicy**: canonical code-unit (locale-independent) ordering
  for every set whose order affects hashes, context, decisions,
  validation, findings, or reports. Filesystem enumeration and concurrent
  completion order are never semantic ordering.
- **EnvironmentManifest**: bounded, secret-free execution-relevant
  environment identity (build/Node/npm/OS/arch/Godot/sandbox, explicit
  locale and timezone policy, environment allowlist, tool identities)
  with an H1 digest.
- **ReproducibilityManifest**: immutable reference set binding the H1
  digests (ExecutionInput, Environment, TaskContract, TaskPlan, Guidance,
  ToolSurface, Capability, source revisions), validation profile,
  provider/model input identity (route, model, reasoning mode,
  temperature, top-p, seed), and clock/RNG policy. Results identify
  `producedUnder: <ReproducibilityManifest digest>`. `ReproducibilityDelta`
  surfaces exactly which dimension changed between runs.
- **Deterministic validation selection**: `deriveValidationPlan` derives
  `required | recommended | unavailable` minimum validation from actual
  changed surfaces, verified impact relationships, acceptance criteria,
  and a validation registry — the model may recommend additional
  validation but may not remove host-required validation.
- **Deterministic acceptance**: `evaluateAcceptance` maps required
  evidence classes + current evidence identities to a deterministic
  outcome; executor/reviewer prose remains non-authoritative.
- **Typed retry policy**: `classifyRetry` maps failure categories to
  `retry | repair | no_retry` with attempt limits and a deterministic
  backoff schedule; stale revisions never auto-retry under an old
  approval; the model never decides retry counts.
- **Concurrency normalization**: parallel results are collected,
  validated, normalized, stable-ordered, then consumed — completion order
  never equals semantic priority.
- **Deterministic repository discovery**: baseline discovery is
  search → classify relevance → normalize paths → stable rank → bounds;
  exact task targets rank first; naming similarity alone never becomes
  verified relevance.
- **Ownership index**: canonical responsibility → owner metadata
  (architecture/navigation only, never a service registry) so executors
  resolve existing owners before creating overlapping abstractions.
- **Nondeterminism audit**: `scripts/check-nondeterminism.mjs` scans
  core production for uncontrolled `Date.now`/`new Date`/`Math.random`/
  `process.env`/`process.cwd`/`readdir`/`randomUUID` decision inputs;
  adapters own external nondeterminism, tests are fixtures, and the CLI
  is the composition boundary — the audit flags uncontrolled decision
  inputs, not platform APIs. Wired into `npm run check`.
- **Determinism doctor**: `/doctor determinism` reports compactly
  (clock, randomness, locale, timezone, environment snapshot,
  reproducibility manifest, file ordering, documentation selection,
  workspace scope, validation selection, tool surface, acceptance, audit)
  and surfaces degraded dimensions. Read-only and offline.

## Explicit distinctions

```text
deterministic host behavior  ≠  deterministic LLM output
reproducibility identity     ≠  trust
stable ordering              ≠  semantic priority
controlled time              ≠  frozen production time
```

## Explicit rejections

- Ambient time/randomness in authoritative policy.
- Filesystem enumeration as semantic ordering.
- Model-selected mandatory validation.
- Exact LLM-output reproducibility as a requirement.
- Serializing all parallel work merely for ordering.
- Global banning of platform APIs.
- Broad Task Runtime rewrite (H1/H2 refactors only the seams).
- Generic deterministic workflow framework.
- Stage 4 runtime QA; record/replay of every provider token;
  cryptographic signatures; global dependency graphs.

## Consequences

- Equivalent authoritative inputs produce equivalent host decisions
  (proven by shuffled-order, fixed-clock, and probabilistic-boundary
  effect tests).
- LLM nondeterminism cannot bypass deterministic host security,
  approval, validation, or acceptance boundaries.
- Reproducibility manifests enable cross-run comparison and future
  `/evolve` experimentation and Stage 4 runtime evidence.
- H1 identity infrastructure is reused (digests, manifests, deltas);
  nothing is duplicated.
