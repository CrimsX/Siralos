# Decision — R8 Entry Review — Freeze R8 Contract, Subjects, Measurement

**Wayfinder ticket:** [R8 Entry Review — Freeze R8 Contract, Subjects, Measurement](../tickets/10-r8-entry-review.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R7 Verified Promotion](../decisions/02-r7-verified-promotion.md) (Verified at `61fbf99` / `bb72482`)
**Decided:** 2026-08-19 (resolver session, interactive review of `ARCHITECTURE.md` Godot chapters 385-520, `SECURITY.md:210-228` fail-closed posture, `ROADMAP.md` §2-3 Godot stages, R7.3 entry precedent `R7_BEHAVIOR_EXTRACTION.md` §14 at HEAD `61fbf99`, and the R8 vs R9 cut at `decisions/04-r8-r9-cut.md` §1)
**Status:** **PASS — R8 contract frozen; authorized as next implementation slice**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Wayfinder **Plan, don't do** — this is a decision, not R8 code. No `crates/siralos-godot` is created, no differential subject lands, no Rust gate advances. It mirrors `R7.3 Projection §14` (entry review before any R8 code lands, contract frozen at byte-level, implementation authorized).

---

## Summary

Freeze **R8 — Optional Godot Stage-2 parity** as six surfaces (discovery, recovery, knowledge, check-only, bounded LSP, read-only scenes) with per-surface fail-closed posture, differential subjects, measurement plan, and audit mechanism. R8 is static understanding + engine-derived read-only validation — **no execution of the project and no scene/resource mutation** — and remains optional-domain, explicitly installed (`AGENTS.md:24-26` lean guardrails).

R8 implementation begins only after this gate is **PASS**. R9 still waits on R8 Verified.

---

## 1. Entry state, scope, and audit result

### Verified local entry state at this gate

| Item                              | Value                                                                                                                                                                                                                                                                                                                                                                                       |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Branch                            | `main`                                                                                                                                                                                                                                                                                                                                                                                      |
| Starting HEAD                     | `bb72482 docs: promote R7 to Verified (61fbf99)` (R7 Verified)                                                                                                                                                                                                                                                                                                                              |
| Starting worktree                 | clean                                                                                                                                                                                                                                                                                                                                                                                       |
| Verified worktree for prior slice | `61fbf997d781377b2501af4057920a2064dd8716` (R7 Verified)                                                                                                                                                                                                                                                                                                                                    |
| Wayfinder Destination             | **reached** — 8 decisions + 09 advisory closed, 9/9 tickets closed                                                                                                                                                                                                                                                                                                                          |
| Corpus at entry                   | schema 3, version 15, 133 scenario files                                                                                                                                                                                                                                                                                                                                                    |
| Corpus digest (prior)             | prior digest at `61fbf99` (Wayfinder-formatted; corpus itself unchanged)                                                                                                                                                                                                                                                                                                                    |
| R8 executable Rust                | absent; no R8 source or implementation commit exists                                                                                                                                                                                                                                                                                                                                        |
| R7 Rust gates at HEAD             | `cargo fmt --all --check` PASS, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` PASS (dev 0.72s), `cargo test --workspace --all-features --locked` PASS (51 siralos-cli incl. 10 sanitize + 326 siralos-core), `npm run check:architecture` PASS, `npm run check:rust` PASS, `npm run check:differential` local EPERM (Tier-1 CI matrix is audit mechanism) |

At this gate the only commit after the verified worktree is the R8 entry-review ticket itself (`dd3c994 docs(wayfinder): add R8 Entry Review grilling ticket (10)`). No R8 Rust implementation commit exists.

### Scope audited for the freeze

Every file under `packages/core/src/godot/` and `packages/adapters/src/godot/` relevant to the 6 R8 surfaces (discovery, recovery, knowledge, diagnostics, LSP, plus read-only scene intelligence from the TypeScript oracle), including the TypeScript contracts, ports, models, limits, prepared-check/diagnostic digest contracts, frame-parser, JSON-RPC, URI mapping, port allocation, fixed runners, and CLI commands `apps/cli` Godot flags (`/godot*` / `--godot-path`).

The review also audited `SECURITY.md` fail-closed posture, `ARCHITECTURE.md` workspace/process/sandbox invariants, and the Wayfinder R8 vs R9 cut decision at `decisions/04-r8-r9-cut.md` §1-§4 as the advisory boundary that this gate now freezes.

---

## 2. Co-located classification table

The following table is the complete R8 boundary. "MUST PORT" means the observable, provider-neutral behavior is part of the next Rust parity slice. "GENERIC SEAM ONLY" means R8 consumes a bounded, typed, host-owned input but does not port the producer. "LATER" names the owning milestone. "DO NOT PORT" is TypeScript implementation structure with no independent Rust contract.

| Behavior                              | Classification                                                                     | Frozen boundary and owner                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------------------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Engine discovery & profiling          | **R8 MUST PORT**                                                                   | Version/release/edition/capability models, deterministic selection policy with recorded rationale, compatibility assessment, `GodotProbeRunner`/`GodotInspector` ports; configured + PATH + `.app` bundle + `.godot-version` fingerprints; edition classification; static project detection/profiling (re-scan bounded project each inspection, no cache) — see `ARCHITECTURE.md:385-416`, `ROADMAP.md:2.1`. |
| Recovery contracts                    | **R8 MUST PORT**                                                                   | One-time approval model, diagnostic normalization, truthful unavailable reporting; recovery never creates engine-affecting mirrors until process launch is mechanically sound — see `ARCHITECTURE.md` + `ROADMAP.md:2.2`, `SECURITY.md:210-228`.                                                                                                                                                             |
| Version-bound API knowledge           | **R8 MUST PORT**                                                                   | Dump model, deterministic symbol identities, query/result, cache-validation, manual-channel; knowledge/profile + API symbol + query models; with-docs dump parser + bounded index/lookup — see `ARCHITECTURE.md:420-452`, `ROADMAP.md:2.3`.                                                                                                                                                                  |
| GDScript check-only contracts         | **R8 MUST PORT**                                                                   | Diagnostic model, severity/aggregation, script enumeration/hashing, prepared-check digest contract; enumeration/hashing/output normalization — see `ARCHITECTURE.md:420-452`, `ROADMAP.md:2.4`.                                                                                                                                                                                                              |
| Bounded LSP (language session)        | **R8 MUST PORT**                                                                   | Session port/models/limits/preview/digest, Content-Length framing, JSON-RPC, mirror URI mapping, port allocation, loopback-only + fixed recovery LSP tuple — see `ARCHITECTURE.md:452-490`, `ROADMAP.md:2.5`.                                                                                                                                                                                                |
| Read-only scene/resource intelligence | **R8 MUST PORT**                                                                   | Bounded `.tscn`/`.tres` tokenizer + conservative Variant parser + `GodotSceneModel`/`GodotResourceModel` + relationship index + project settings/autoload/input — static, process-free, revision-bound — see `ROADMAP.md:2.6` / `3.8`.                                                                                                                                                                       |
| Engine probe invocation (fail-closed) | **R8 MUST PORT** (behavior: prove `unavailable`)                                   | Fixed `fixedProbeArguments` private constructor (`--version`/`--help`/`--dump-extension-api-with-docs` only; no `--path`/`--script`/`--editor` outside check-only/LSP runners) — differential proves typed `unavailable` without spawn/mirror.                                                                                                                                                               |
| Check-only runner argument discipline | **R8 MUST PORT**                                                                   | `--script` only paired with `--check-only` + `--headless` + mirror-only `--path`; source workspace never becomes diagnostic `--path` — see `ARCHITECTURE.md` check-only section.                                                                                                                                                                                                                             |
| LSP runner argument discipline        | **R8 MUST PORT**                                                                   | `--lsp-port` only paired with `--headless --editor --recovery-mode --path <mirror>`; DAP/scene/import/quit never appear — see `ARCHITECTURE.md` LSP section.                                                                                                                                                                                                                                                 |
| Scene/resource prepared mutation      | **LATER — R9**                                                                     | Typed scene/resource prepared mutation, impact/regression planning, unified `/develop` — owns authored change; still fail-closed on directory-relative primitives — see `ROADMAP.md:3:8-3.9`.                                                                                                                                                                                                                |
| Engine execution / recovery mirrors   | **LATER — R8 recovery inside R8, but engine-affecting recovery stays fail-closed** | Mirrors remain `unavailable` while process launch is unsound; recovery contracts are R8 but engine-affecting execution is not.                                                                                                                                                                                                                                                                               |
| Provider Godot probes at request time | **DO NOT PORT**                                                                    | Providers never run Godot (`ARCHITECTURE.md` Godot intake); only the fixed probe adapter may, always through `SandboxBackend`, and at R8 it still fails closed.                                                                                                                                                                                                                                              |
| Godot-is-a-plugin marketplace         | **DO NOT PORT — lean freeze**                                                      | `AGENTS.md:24-26` + `ARCHITECTURE.md` lean constitution — optional-domain, explicitly installed, never auto-installed; no placeholder `crates/siralos-godot` before entry review — enforced at `scripts/check-rust-architecture.mjs:29-31/282-283`.                                                                                                                                                          |

---

## 3. Frozen behavioral boundaries (file:line citations)

- **Discovery without invocation is available; probe-dependent ranking falls back** — `ARCHITECTURE.md:385-416` (configured installations + PATH + `.app` + `.godot-version` fingerprints; engine-profile cache is explicitly unavailable no-op, `profile/` never initialized). Differential proves discovery finds configured + PATH entries and ignores reparse/symlink escape (R4 containment `SECURITY.md:210-228`).
- **Knowledge / check-only / LSP execution stays `unavailable` at R8** — `SECURITY.md:210-228` + `ARCHITECTURE.md:420-452` check-only section + `ARCHITECTURE.md:452-490` LSP section. Preparation returns typed `unavailable` before any approval/mirror; `godot.api_search/lookup` degrade to typed unavailable rather than fabricated knowledge; `--check-only` and LSP startup never spawn on the TypeScript oracle either (`ROADMAP.md:2.2-2.5` truthful unavailable reporting).
- **Read-only scene/resource parse is available and purely static** — bounded tokenizer + conservative Variant parser in the TypeScript oracle (and future `siralos-core` typed models + `siralos-adapters` tokenizer in Rust) with revision-bound models `GodotSceneModel`/`GodotResourceModel` and bounded truncation / UID/signal/group preservation — see `ROADMAP.md:2.6` / `3.8` and `decisions/04-r8-r9-cut.md` §1 R8 row.
- **Argument discipline is the boundary, not a comment** — private `fixedProbeArguments` is the only source for `--dump-extension-api-with-docs`/`--version`; `--script` pairs only with check-only; `--lsp-port` pairs only with recovery tuple — enforced by `npm run check:architecture` (fixed runner), not merely by docs.

---

## 4. Differential plan

The future R8 subject family is advisory in `04-r8-r9-cut.md` §1; this gate freezes it:

- `godot-discovery` (configured + PATH + `.app` + static project detection, reparse/symlink escape typed `unavailable`)
- `godot-knowledge` (dump model + deterministic symbol identities + query/result; `godot.api_search/lookup` typed unavailable while `--dump-extension-api-with-docs` is fail-closed)
- `godot-diagnostics` (diagnostic model + severity/aggregation + prepared-check digest; `godot.check_script/check_project_scripts` typed `unavailable`)
- `godot-lsp` (session models + framing + JSON-RPC + URI mapping + loopback-only port allocation; `godot.lsp_session` typed `unavailable`)
- `godot-scene-resolve` (bounded `.tscn`/`.tres` parse + Variant + UID/signal/group + bounded byte accounting — the only available-at-R8 Godot intelligence)

The real corpus version bump lands with the R8 entry review reconciliation, not with this decision. No engine binary is required — fixtures are scripted probe/diagnostic/frame recordings (mirrors `provider-turn` hardening: unknown/malformed discriminators fail closed).

---

## 5. Evidence assignment

| Boundary/gap                                                     | Evidence layer                                                                                         | Owner                               |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------- |
| Discovery + profiling (models + selection policy + fallback)     | Core unit + differential `godot-discovery`                                                             | `siralos-core`                      |
| Knowledge / diagnostics / LSP availability (typed `unavailable`) | Adapter fail-closed orchestration + differential (`godot-knowledge`, `godot-diagnostics`, `godot-lsp`) | `siralos-adapters`                  |
| Bounded scene/resource static parse                              | Core typed models + adapter tokenizer/Variant + differential `godot-scene-resolve`                     | `siralos-core` / `siralos-adapters` |
| Argument discipline (fixed runners)                              | Architecture check (structural TypeScript parsing) + differential probe of bad argument sets           | `scripts/check-architecture.mjs`    |
| Fail-closed reporting (no filesystem mutation / no spawn)        | Differential `unavailable` typed outcome, no mirror created, no executable launched                    | Harness                             |

---

## 6. Security/architecture review

At this gate no R8 Rust code exists to review. The pre-port review is **PASS (scoped)**: the frozen contract preserves the TypeScript oracle's fail-closed posture (every engine-affecting surface reports typed `unavailable` and performs no filesystem mutation or process launch where the cited primitive is unavailable), and the Rust ownership proposal is that Godot discovery/profiling models live in `siralos-core` while all FS/process/socket concerns live in `siralos-adapters` (provider never runs Godot). `npm run check:rust` will enforce `FORBIDDEN_CORE_SYMBOL_PATTERN` and crate direction once R8 code lands; the lean freeze forbids a placeholder `crates/siralos-godot` before this gate.

---

## 7. Measurement

No performance benchmark is required for the freeze — measurement lands with R8. Per `RUST_STYLE.md:568-589` evidence-first rule, benchmarks appear only where a hot spot is measured (e.g., bounded `.tscn` tokenizer throughput, with-docs index build). A faster incorrect fallback ordering for selection policy is a regression.

---

## 8. Authorization

This entry review returns **PASS** — R8 contract frozen; R8 implementation authorized as the next slice. This authorization is limited to R8: it does not satisfy R8 itself, authorize R9 or R8+ or Stage-4 entry, and does not promote the corpus. The next commit after this gate should be the R8 implementation slice, not R9 or Stage-4 work.

---

## Self-loop verification (this decision)

| Criterion                                             | Direct evidence                                                                                                                                                                                                                                           | Status |
| ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| 6 R8 surfaces frozen (which ships)                    | §2 co-located table restates the two-row table from `decisions/04-r8-r9-cut.md` §1 (6 R8 rows + per-row unavailable posture) + §3 file:line citations for each surface — answers the ticket's first two bullets                                           | pass   |
| Per-surface fail-closed posture                       | §2-§3: every engine-affecting surface typed `unavailable` without filesystem mutation or process launch where the cited primitive is unavailable (`SECURITY.md:210-228`); discovery without invocation is the only available path                         | pass   |
| Differential subjects + counts, corpus bump mechanics | §4 freezes 5 subject names (`godot-discovery`, `godot-knowledge`, `godot-diagnostics`, `godot-lsp`, `godot-scene-resolve`); corpus bump lands with the R8 reconciliation, not with this decision — mirrors R7.3 §14 (no corpus promotion at entry review) | pass   |
| Measurement plan                                      | §7: benchmarks only where a hot spot is measured per `RUST_STYLE.md:568-589` — no speculative benchmark, no fake hot spot                                                                                                                                 | pass   |
| Entry gate not authorizing R9                         | §8: authorization limited to R8; R9 still waits on R8 Verified                                                                                                                                                                                            | pass   |

Evidence ladder: L1 observed entry state (HEAD `bb72482` R7 Verified, corpus v15 133, gates) + reads of `ARCHITECTURE.md:385-520` + file:line citations; L3 porting gate precedent (R7.3 §14); L4 decision markdown itself. No R8 Rust code, no corpus bump, no entry-review beyond this decision.

---

## Out of scope for this decision (per lean ADR 0036)

No Rust module, no run-directory primitive, no corpus promotion, no entry-review beyond this decision. R9 remains not due; R9 still waits on R8 Verified. R10+ remain not due.
