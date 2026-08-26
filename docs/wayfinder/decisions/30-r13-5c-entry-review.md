# Decision — R13.5c Entry Review — Deferred Seams (Knowledge Seeding + Reference/Research Access)

**Wayfinder ticket:** [R13 Execution Register](../tickets/22-r13-execution-register.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [R13.5 Entry Review](27-r13-5-entry-review.md) (PASS) + R13.5b landed (`700b852`, parity held 228/228 at corpus v29)
**Decided:** 2026-08-26 (resolver session, HITL grilling over reads of `packages/core/src/knowledge/knowledge-seeding.ts:14-92`, `packages/core/src/reference/reference-access.ts`, `packages/adapters/src/reference/reference-services.ts`, `packages/core/src/research/research-service.ts:1-120`, and the deferred-seam notes in decisions 23/24/26)
**Status:** **PASS — R13.5c scenario set frozen; implementation authorized**

---

## Frozen scenario set (deferred seams, ~10 groups, corpus v30)

All cases extend **existing subjects** (no new subject name) — `knowledge-revisions`, `reference-identity`, `research-policy` — and are exercised through the pure `siralos_core` seams with harness-injected ports/clock. No filesystem, no network, no wall clock.

### `knowledge-revisions` extension ×3 (seeding)

| # | Case | What it proves |
|---|------|----------------|
| 1 | `knowledge-seeding-candidates` | `buildGodotProjectKnowledgeCandidates` over `GodotProjectKnowledgeSeed` (null vs `project.godot` sha, declaredEngineVersionRaw, languageProfile, hasDotnet, projectName) — 5 seed shapes → exact candidate sets (subjectKey/content/provenance/confidence/volatility), conservative seeding (no broad inference, always has_dotnet, project.name only when declared, provenance only when sha present) |
| 2 | `knowledge-seeding-coordinator-integration` | Candidates flow through `KnowledgeCoordinator.propose` → `activeFacts` with `kf_` ids, `revision 1`, `retrieved` activation; reproposal no-churn vs revision bump |
| 3 | `knowledge-seeding-bounds` | Empty/invalid seed fields (empty version string, unknown languageProfile, null projectName) produce no fact; has_dotnet always present as `true`/`false` |

### `reference-identity` extension ×4 (access port + tools)

| # | Case | What it proves |
|---|------|----------------|
| 1 | `reference-access-list` | `ReferenceAccessPort.list` returns bound revision identities for ready references only, redacted paths, deterministic order |
| 2 | `reference-access-read` | `ReferenceAccessPort.read` returns bounded exact content for ready local-directory references, `unavailable` for repository references (typed reason), outside-workspace containment fails closed |
| 3 | `reference-access-search` | `ReferenceAccessPort.search` over materialized local-directory references — bounded, deterministic, redacted |
| 4 | `reference-tools-visibility` | `reference.inspect` Tools (`reference.list`/`read`/`search`) appear only when at least one reference is ready; gated by `reference.inspect` capability (allow/deny matrix) |

### `research-policy` extension ×4 (access port + tools)

| # | Case | What it proves |
|---|------|----------------|
| 1 | `research-access-port-list` | `ResearchAccessPort.list` over `ResearchService` evidence ring — bounded, detached, redacted |
| 2 | `research-access-port-read` | `ResearchAccessPort.read` returns bounded research document content for existing evidence, `not-found` for unknown id, detached |
| 3 | `research-tools-visibility` | `research.fetch` Tools (`research.search`/`read`) are always registered but gated by `research.fetch` policy (deny by default, allow when Host grants) — visibility vs capability distinct |
| 4 | `research-evidence-provenance` | Research evidence carries provenance (source id/label, request digest, resolved revision, fetchedAt) and is host-verified before `KnowledgeCoordinator` propose gate |

## Mechanics

- Probe layout: extends `knowledge-oracle.mjs` / `reference-identity-oracle.mjs` / `research-policy-oracle.mjs` with new case groups that drive the **real TypeScript** `buildGodotProjectKnowledgeCandidates` / `ReferenceAccessPort` / `ResearchAccessPort` over harness-injected fakes; Rust side ports via `siralos_core::knowledge::seeding` + `siralos_core::reference::access` + `siralos_core::research::access` behind the same subject names. Placement obeys `cli → adapters → core`.
- Corpus bumps **v30** at the R13.5c reconciliation commit; schema stays 3.
- **Determinism:** one injected clock per run (`NowFn` 1700000000000), injected fixture seeds/ports, no filesystem beyond harness temp fixtures, no network, sanitizer not involved.
- **Fail-closed:** reference repository reads remain typed `unavailable`; research port is in-memory evidence ring only; seeding creates no filesystem state.

## Boundaries — not in R13.5c

- Full `cli-session` closure (remaining ~75 commands, `godot`/`develop` unavailable reporting, sanitizer boundary, input-queue ownership) stays in **R13.5d** per decision 27.
- No new capability grants: `reference.inspect` / `research.fetch` remain Host-enforced; `KnowledgeCoordinator` never grants capability.
- No filesystem mutation, no process, no network.

## Authorization

Implementation of R13.5c is authorized against this frozen set; landings are recorded in the [R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion | Direct evidence | Status |
|-----------|-----------------|--------|
| Cases exercise reference-observable behavior | `knowledge-seeding.ts:34-92` seed shapes + `KnowledgeCoordinator.propose` read this session; `reference-access` port list/read/search signatures and `research-access` list/read signatures verified via `packages/core/src/reference/*` and `packages/core/src/research/*` exports | pass |
| Determinism posture preserved | § Mechanics injects one fixed clock + fixture seeds/ports; no fs/network/TTY; all inputs bounded and detached | pass |
| Overlap resolved, no double port | R13.2 knowledge core (7 cases) stays, R13.5c adds only seeding (3); R13.3 reference/research (10+10) stays, R13.5c adds only access ports/tools (4+4) — no synthesis of R13.5b/d seams | pass |
| Human decided the material cuts | HITL answer 2026-08-26: “10 groups as proposed (3+4+4), v30, existing subjects extended, no new subject” | pass |
