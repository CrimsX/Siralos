# Decision — Stage 4.1 Generic Runtime + Godot Plugin Extraction Contract

**Wayfinder ticket:** [Stage 4.1 Entry + Godot Extraction Grilling](../tickets/34-stage4-1-godot-extraction.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R13 Verified Promotion](32-r13-verified-promotion.md) (PASS, R1–R13 Verified, corpus v31) + [R12 Disposition Execution](33-r12-disposition-execution.md) (PASS, retired at 4bef901, `PRE-STAGE-4 ASSURANCE: PASSED`)
**Decided:** 2026-08-27 (resolver session, interactive HITL grilling over `ARCHITECTURE.md:350` modular monolith, `docs/development/PROJECT_CONTEXT.md:344` Godot absent, `docs/adr/0034` WIT boundary, `docs/adr/0036` lean Plugin model, and `crates/siralos-core/src/godot` 6+3 surfaces)
**Status:** **PASS — Stage 4.1 authorized next; Godot extraction contract frozen as in-repo crate `crates/siralos-godot` with empty-state `Add Plugin` UI**
**Self-loop ledger:** 4 criteria, one decision pass (verification below)

> Mirrors `decisions/08-stage4-entry-sequence.md` (generic-first) and `decisions/04-r8-r9-cut.md` (6+3 surfaces). No implementation lands here; this freezes scope, ordering, and the `Add Plugin` empty-state contract.

---

## Summary

HITL confirmed the **two-phase continuation**:

1. **Stage 4.1 Controlled Runtime Execution first** — generic host-authorized process supervision (`siralos-core::runtime` `artifacts/budgets/supervision/doctor` + `siralos-adapters::runtime`) that flips `unavailable`→`available` behind identity-bound handles (`openat/renameat`, `exec-by-handle`). No Godot code moves until this boundary is proven.
2. **Godot Plugin extraction as in-repo crate `crates/siralos-godot`** — moves **exactly the 6+3 R8/R9 surfaces** (`docs/wayfinder/decisions/04-r8-r9-cut.md`):

   - **R8 (6):** discovery/profiling, recovery contracts, version-bound API knowledge, GDScript check-only diagnostics, bounded LSP, read-only scene/resource intelligence
   - **R9 (3):** review context & impact intelligence, prepare-only `scene_mutation` contracts, deterministic unified `/develop` core (`siralos_core::godot::{impact,scene_mutation,development}` + `siralos-adapters::godot::scene_mutation` `apply` `unavailable`)

   No new marketplace, no auto-acquisition (ADR 0036 §35-36). External repo `github.com/CrimsX/siralos-godot` is **FUTURE / NOT DUE** until `siralos.toml`→`siralos.lock` portable locking (Stage 5) is proven; the crate extraction is the refactor under parity.

3. **Empty-state `Domains` view + `Add Plugin` UI** — `Domains` starts **empty**, Godot never appears until explicit user action:

   - Button: **`Add Plugin` → pick folder containing plugin manifest** (`manifest { id, digest, abi }` per `domain::package` `abi_validation_and_exact_compatibility`), not a raw `project.godot` folder
   - Persistence: `siralos.toml` `[plugins.godot] path = "..." digest = "sha256:..."` (portable, `cargo deny` pinned) + `.siralos/` runtime state (`installed`→`enabled`→`active` per `domain::lifecycle` `absent/installed/enabled/active`)
   - Fail-closed: picker does `lstat`+`isFile`+`maxFileSha256Bytes` bounded read, `is_path_within` containment, SHA-256 verification _before_ `Enabled → Active`; `unavailable`/`refused`/`failed` (`reference-identity` audit) keep UI empty with typed reason, never silent success
   - Host authority: `Enable` and `Activate` remain Host-gated (`domain::capability` `grant_equals_request_within_authority`, `prepare` never carries authority across lifecycles per `RUST_MIGRATION.md:198`)

## 1. HITL answers (2026-08-27)

| #   | Frontier question                                                  | Human answer                                                                           |
| --- | ------------------------------------------------------------------ | -------------------------------------------------------------------------------------- |
| 1   | Separation unit: new GitHub repo `siralos-godot` vs in-repo crate? | **New GitHub repo `siralos-godot` eventual**; **in-repo `crates/siralos-godot` first** |
| 2   | Ordering: Stage 4.1 generic runtime first vs immediate extraction? | **OK 4.1 first**                                                                       |
| 3   | Scope: move exactly 6+3 surfaces?                                  | **Move exactly 6+3**                                                                   |
| 4   | Distribution: marketplace/auto-acquisition?                        | **No new marketplace, no auto-acquisition**                                            |
| 5   | Add domain vs Add Plugin (manifest folder)?                        | **Sure — Add Plugin (manifest folder)**                                                |
| 6   | Persistence: `siralos.toml` portable?                              | **Sure**                                                                               |
| 7   | Picker contract: typed fail-closed?                                | **Sure**                                                                               |
| 8   | Local crate first vs immediate external repo?                      | **Sure — local crate first**                                                           |

## 2. Frozen extraction contract

| Item                    | Frozen decision                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **New crate**           | `crates/siralos-godot` (member of `Cargo.toml` workspace, `resolver = "3"`), `edition = "2024"`, `forbid(unsafe_code)`. Dependencies: `siralos-godot → siralos-core` only; `siralos-adapters` must not depend on it; `siralos-core` must not import it (enforced by `npm run check:rust`). `crates/siralos-adapters/wit/domain-abi.wit` `siralos:domain-abi@1.0.0` is the **sole** host/guest boundary.            |
| **Moved surfaces**      | `siralos-core::godot` (models, limits, `impact`, `scene_mutation` prepared contracts, `development` surface routing/order) + `siralos-adapters::godot` (discovery/profiler, knowledge service, diagnostics service, LSP framing, scene/resource parsers). `siralos-core::language` (generic, language-neutral) **stays** in core; GDScript scanner stays TypeScript reference only until extraction proves parity. |
| **Differential parity** | Corpus `godot-*` subjects (discovery 4, knowledge 5, diagnostics 4, lsp 4, scene-resolve 5, review-context 4, mutation-prepare 4, develop-plan 4) remain **150/150 + 162/162** at `check:differential`; crate move is a **refactoring under parity** (ADR 0032 `behavioral parity != structural parity`), no new scenario required for the move itself.                                                            |
| **UI contract**         | `Domains` view (new `siralos-cli` surface) starts `No domains installed. [Add Plugin]`; `Add Plugin` invokes `DomainHost::install` with exact-byte digest verification; `Enable`/`Activate` are separate Host-gated steps; `Godot absent: core builds and tests` stays green (`stage4-entry-gate.md:33`).                                                                                                          |
| **External repo**       | `github.com/CrimsX/siralos-godot` is **not created** in this milestone; it becomes the distribution channel only after Stage 5 `siralos.toml`→`siralos.lock` + `cargo deny` pinning is demonstrated on the in-repo crate. No `path =` hack beyond local dev.                                                                                                                                                       |

## 3. Ordering and gates

```
R12 retired (4bef901, 72e20be) ──► Stage 4.1 generic runtime (host siralos-core::runtime + siralos-adapters::runtime, 17/17 gate re-evaluated)
        │
        └─► crates/siralos-godot extraction (6+3 surfaces) behind siralos:domain-abi@1.0.0, differential 150/150+162/162 retained
                │
                └─► Empty-state Domains view + Add Plugin (siralos.toml) — presentation only, no new authority
                        │
                        └─► (FUTURE) external siralos-godot repo + registry publish (Stage 5)
```

Each arrow is a separate entry-reviewed slice (mirrors `R10 Scope` 3-slice discipline). No slice flips `unavailable` to `available` except Stage 4.1's generic runtime behind identity-bound handles.

## 4. Authorization

**Stage 4.1 is authorized as the next implementation slice** against the generic `artifacts/budgets/supervision/doctor` contract (re-evaluated `stage4-entry-gate.md` 1,3–6). `crates/siralos-godot` extraction is frozen but **not authorized to start** until Stage 4.1 is `Verified` (generic boundary proven). The `Add Plugin` UI slice is frozen but **not authorized** until the crate extraction is `Verified`.

---

## Self-loop verification

| Criterion                                                       | Direct evidence                                                                                                                                           | Status |
| --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Human decided the 8 material cuts                               | §1 table: 8 answers `new repo`/`4.1 first`/`6+3`/`no marketplace`/`Add Plugin`/`siralos.toml`/`typed fail-closed`/`local crate first` recorded 2026-08-27 | pass   |
| Scope is exactly 6+3, no marketplace, lean guardrails preserved | §2: 6 R8 + 3 R9 enumerated per `04-r8-r9-cut.md`; `No new marketplace` per ADR 0036 §35-36; `forbid(unsafe_code)` retained                                | pass   |
| Ordering is generic-first, crate-before-repo                    | §3 DAG: 4.1 → crate → UI → external repo; each arrow entry-reviewed                                                                                       | pass   |
| Empty-state UI is fail-closed and manifest-bound                | §2 UI contract: `lstat`/`is_path_within`/SHA-256 before `Enabled→Active`, `siralos.toml` digest pin, `Enable`/`Activate` Host-gated                       | pass   |

Evidence ladder: L1 reads of `ARCHITECTURE.md:350`, `PROJECT_CONTEXT.md:344`, `RUST_MIGRATION.md:198`, `stage4-entry-gate.md:46`; L2 HITL answers verbatim; L3 lean constitution ADR 0036.
