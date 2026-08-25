# Decision — R13.3 Entry Review — External Knowledge Boundaries Scenarios

**Wayfinder ticket:** [R13.3 Entry Review](../tickets/25-r13-3-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13 Continuation Contract](21-r13-remaining-surface-parity-entry-review.md) (PASS) + R13.2 landed (`d682e62`, parity held 222/222 at corpus v25)
**Decided:** 2026-08-24 (resolver session, interactive HITL grilling over reads of `packages/core/src/reference/*`, `packages/core/src/research/*`, `packages/adapters/src/reference/{resolver,materializer,path,cache,services}/*`, and `packages/adapters/src/research/{normalization,fake-sources}/*`)
**Status:** **PASS — R13.3 scenario contract frozen; implementation authorized**
**Self-loop ledger:** 4 criteria, one implementation pass (verification below)

> Mirrors [R13.1 Entry Review](22-r13-1-entry-review.md) / [R13.2 Entry
> Review](23-r13-2-entry-review.md). No implementation lands in this record.

---

## Summary

R13.3 ports the external-knowledge boundary backbone to the Rust
candidate and proves it differentially: strict reference declaration
parsing, the registry as the SINGLE owner of reference identity
(statuses, fail-closed refresh, task-binding snapshots), resolver
identity semantics over harness-injected backends, the materializer's
typed-`unavailable` repository posture, and the research service's
denied-by-default policy gate with bounded normalization/evidence
retention. The existing `siralos_core::security` policy layer from R13.1
is the permission seam and is **not** duplicated; this slice consumes it
(`research.fetch` gate-first) exactly as the TypeScript service does.

## 1. Frozen scenario set (~20 fixtures, corpus v26)

### `reference-identity` ×10

| #   | Case                              | What it proves                                                                                                                                                                                                                                                                                  |
| --- | --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | declaration-parse-strict          | unknown keys rejected with exact reasons (nothing can hide); alias pattern; description byte bound; local-directory requires an absolute path (POSIX/drive/UNC accepted, relative refused); section parse enforces alias-key match and the count bound                                          |
| 2   | origin-normalization              | `owner/repo`, https URLs, `.git`, trailing slashes normalize to `https://github.com/owner/repo`; http://, foreign hosts, userinfo, query, fragment, extra segments refuse with exact reasons                                                                                                    |
| 3   | ref-parsing-and-pins              | commit pins are 7–64 hex; tag/branch charset+length bounds; malformed refs refuse with exact reasons                                                                                                                                                                                            |
| 4   | mutable-ref-declined-pre-resolver | absent ref defaults to branch `main`; the registry declines before any resolver call (spy proves zero invocations) with the exact refusal; pinned commits/tags resolve through the injected fake backend; `allowMutableRefs=true` resolves a branch and records the resolved commit as identity |
| 5   | workspace-containment-refusal     | a local-directory identity resolving inside the workspace namespace declines with the exact reason (Windows-form comparison case-insensitive); outside roots resolve ready; refresh re-checks containment and demotes to declined                                                               |
| 6   | duplicate-alias-audit             | first occurrence wins and stays addressable; duplicates stay listed with status declined `"duplicate alias"` and never enter the lookup maps                                                                                                                                                    |
| 7   | resolver-outcome-matrix           | unavailable/refused/failed outcomes map to statuses with exact reasons; unresolvable references remain listed (auditable config); `ref_` ids are stable digest derivations of the alias                                                                                                         |
| 8   | refresh-fail-closed-invalidation  | identical identity → `unchanged` returning the current revision; changed fingerprint/commit → `refreshed`; failed refresh invalidates the current revision (fail closed); declined references refuse refresh                                                                                    |
| 9   | task-binding-fifo-snapshot        | `bindTask` snapshots ready-only revisions; FIFO eviction beyond the binding limit; evicted bindings read null (never authoritative); snapshots survive later refreshes                                                                                                                          |
| 10  | materializer-posture              | local-directory materialization is a zero-fs no-op returning the canonical root with status `not-required`; repository materialization reports typed `unavailable` with the exact message; `status()` defaults `not-materialized`                                                               |

### `research-policy` ×10

| #   | Case                              | What it proves                                                                                                                                                                                                                                                                         |
| --- | --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | denied-by-default-gate-first      | over every built-in profile the `research.fetch` gate refuses via the R13.1 security seam with the exact per-decision reason (deny vs ask branches both exercised); a source-port spy proves the port is NEVER invoked when the gate does not allow (effect-test contract)             |
| 2   | request-validation-bounds         | empty query, >512-byte query, absolute/backslash/NUL/`.`/`..` path segments, oversized path/ref, malformed version, non-positive maxBytes each refuse with exact reasons; valid requests normalize (maxBytes floored)                                                                  |
| 3   | source-matching-and-refusal       | kind+id matching preferred with label fallback; unconfigured sources refuse with the exact reason                                                                                                                                                                                      |
| 4   | task-binding-required-fail-closed | no active task refuses with the exact reason; the captured binding is a frozen value snapshot (caller mutation cannot defeat staleness)                                                                                                                                                |
| 5   | stale-result-discarded            | a mid-flight task/revision change yields status `stale` and nothing enters the evidence ring                                                                                                                                                                                           |
| 6   | timeout-cancelled-precedence      | an already-aborted signal cancels without invoking the source; timeouts produce the exact `timed out after <ms>ms.` reason under injected bounds                                                                                                                                       |
| 7   | normalization-bounds-disclosure   | markdown/json normalization under bounds: section caps, heading/text byte bounds, truncation markers and disclosure flags; content-type classification incl. unsupported null; content digest computed AFTER final truncation; deterministic `rd_` ids from source id + request digest |
| 8   | provenance-fallback-semantics     | fake godot-docs serves exact versions (`fallback=false`) and explicit fallback chains (`fallback=true` + reason); unknown topics fail "not found"; fake repository provenance: commit pins carry `resolvedRevision`, branch/tag pins leave it null                                     |
| 9   | evidence-ring-retention           | bounded FIFO retention by count and byte budget; oldest entries evicted; sequence evidence ids; excerpt bound with truncation flag; snapshots are detached copies                                                                                                                      |
| 10  | evidence-view-rendering           | the model-facing research evidence view renders byte-equal (Source/Request/Fetched ISO/Revision/Version(+fallback)/Excerpt/Evidence) under the injected clock with maxBytes truncation                                                                                                 |

## 2. Mechanics

- Probe layout follows the established pattern: new oracle probes
  (`reference-identity-oracle.mjs`, `research-policy-oracle.mjs`) execute
  the **real TypeScript reference** functions with bounded stdin JSON and
  emit canonical outcome records; the Rust side adds the corresponding
  modules behind the same subject names. Placement obeys the dependency
  direction (`cli → adapters → core`; core imports no adapters).
- Corpus bumps **v26** at the R13.3 reconciliation commit; schema stays 3.
- **Injected clock everywhere**: `resolvedAtMs`, `fetchedAtMs`,
  `boundAtMs`, and rendered timestamps come from one fixed clock so
  records are byte-stable cross-platform.
- **Local-directory fingerprints use real enumeration** over bounded
  harness-provisioned temporary fixture directories (HITL decision
  2026-08-24): symlink/special-file/cap behavior stays observable.
  Outcome records carry fingerprints, hashes, and reference-relative
  paths only — **absolute paths are redacted** (report-safe by
  construction).
- **No network anywhere** (HITL decision 2026-08-24): research transports
  are exercised only through deterministic fakes; the Node HTTPS wiring
  is adapter-internal and covered by focused unit tests, never
  differentially. Repository resolution/materialization stay on
  unavailable/fake backends — zero spawn paths, nothing fetched.
- Acceptance mirrors every prior slice: all applicable required scenarios
  at byte parity plus focused Rust unit tests; the full local gate passes
  on the reconciliation tree.

## 3. Boundaries — not in R13.3

- `ReferenceAccessPort` implementations (list/read/search) and the three
  reference Tools defer to R13.5 CLI product composition (HITL decision
  2026-08-24); this slice owns identity/resolver/materializer only.
- No unavailable effect becomes operational: repository resolution,
  materialization, and cache stores report typed `unavailable`
  truthfully; the fail-closed posture is exercised, never flipped.
- Research remains denied-by-default in every built-in profile; no
  approval protocol for research exists and none is added.
- Knowledge seeding remains R13.5 (per [R13.2 Entry Review](23-r13-2-entry-review.md));
  the research→knowledge propose gate was already proven there and is not
  duplicated.

## 4. Authorization

Implementation of R13.3 is authorized against this frozen set; landings
are recorded in the [R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                                                                                                                                                                                                                                                                                             | Status |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference-observable behavior | §1 cites concrete surfaces verified by reading the TS sources this session: `parseReferenceDeclaration*`/`normalizeRepositoryOrigin`, registry create/list/bindTask/refresh/declineReason, materializer messages, `evaluatePermission("research.fetch")` gate order in `createResearchService`, `validateResearchRequest` bounds, `buildResearchDocument`/truncation/digest-after-truncation, evidence store FIFO/rendering | pass   |
| Determinism posture preserved                | §2 injects one fixed clock; §1 cases 4–8 run on injected fake/unavailable backends; §2 excludes live network; absolute paths redacted from records                                                                                                                                                                                                                                                                          | pass   |
| Overlap resolved, no double port             | Permission evaluation stays in `siralos_core::security` (R13.1); knowledge seeding/proposal stays with R13.2/R13.5; access port + tools deferred to R13.5                                                                                                                                                                                                                                                                   | pass   |
| Human decided the material cuts              | HITL answers 2026-08-24: 20-case set approved as proposed; access+tools deferred to R13.5; bounded temp dirs with redacted paths confirmed                                                                                                                                                                                                                                                                                  | pass   |
