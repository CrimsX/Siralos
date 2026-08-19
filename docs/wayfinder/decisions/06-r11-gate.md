# Decision — R11 Gate — Full Differential, Effect-Boundary, Security, Recovery, Cross-Platform Closure Criteria

**Wayfinder ticket:** [R11 Gate — Full Differential, Effect-Boundary, Security, Recovery, Cross-Platform Closure Criteria](../tickets/06-r11-gate.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [R10 Scope — H1/H2/ICM + H3 Runtime-Readiness Parity Slice Shape](../decisions/05-r10-scope.md) (closed) — also implicitly on R7 Verified + R8/R9 entry review per the milestone sequence
**Decided:** 2026-08-18 (resolver session, reads of `docs/development/RUST_MIGRATION.md` § Porting gate + Stage 4 entry, `docs/development/RUST_STYLE.md` § Engineering priority order + Performance, `SECURITY.md`, `ARCHITECTURE.md` workspace/process/sandbox sections, `docs/development/PROJECT_CONTEXT.md` §14, and the R10 decision artifact at file:line)
**Status:** Closure gate frozen — entry conditions, 4 pillars of fullness, measurement discipline, and PASS artefacts all named; no code or corpus lands here.
**Self-loop ledger:** 4 criteria, one implementation pass (verification below)

> Wayfinder **Plan, don't do** — this is a decision, not implementation. No new mutation primitive is created, no sandbox is claimed available, no recovery loop runs, no benchmark is taken.

---

## Summary

**R11 Verified** means **every TypeScript Stage-3 surface has a Rust differential subject at parity on the same Verified worktree**, the two fail-closed filesystem primitives are modelled exactly where the TypeScript reference proves them, the sandbox conformance truthfully reports its state (with unavailable never passed as secure), typed recovery never broadens authority, and the Tier-1 cross-platform audit is green at the final corpus version.

R11's job is not to add a new product feature. It is the **last parity line before the reference can be retired or retained** — the point where the harness can say "the Rust candidate speaks for the same observables as the TypeScript oracle on all required scenarios, on all three platforms, including the ones that must report `unavailable`."

R10 (identity/determinism/ICM/readiness) is the last _semantic_ milestone; R11 is the last _mechanical_ one. R11's Verified commit is the direct predecessor of [R12 Disposition](../tickets/07-r12-disposition.md) (retirement vs retention), and R12 is the predecessor of [Stage 4 Entry Sequence](../tickets/08-stage4-entry-sequence.md).

---

## 1. Entry conditions — what must exist before R11 hands-on begins

R11 may not begin implementation (no differential subjects land, no effect-boundary Rust module changes) until all of the following are **Verified or frozen** on the main branch head:

| Dependency                                          | Required state                                                                                                                                                                                                                    | Evidence that satisfies it                                                                                  | Why hard                                                                                                                                       |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| **R7 Verified** (overall)                           | `Verified` per [R7 Verified Promotion](../decisions/02-r7-verified-promotion.md) §1-§2 (R7A + R7.1-7.5 all complete and evidence-backed; promotion commit V with 7-surface atomic update + corpus v15 133 + differential 128/128) | `docs/development/PROJECT_CONTEXT.md` head + `RUST_MIGRATION.md` R7 tail advancing atomically               | All later subjects consume provider projection, tool loop, and config seams                                                                    |
| **R8 + R9 entry-reviewed + implemented**            | R8 (6 surfaces) and R9 (3 surfaces) each entry-reviewed (mirrors R7.3 §14) and then `Verified`, per [R8 vs R9 Cut](../decisions/04-r8-r9-cut.md) §1                                                                               | Two entry-review passes + two Verified promotions                                                           | Godot read-only and prepared-mutation surfaces are optional-domain prerequisites for full effect-boundary semantics                            |
| **R10a + R10b + R10c entry-reviewed + implemented** | All three sub-slices entry-reviewed and `Verified` as one R10 milestone (one promotion commit), per [R10 Scope](../decisions/05-r10-scope.md) §1-§2                                                                               | R10 decision artifact + three sub-slice entry reviews + one Verified commit with corpus bump                | H1→H2→ICM→H3 is the hard dependency chain; recovery classification (R11) depends on typed-failure distinctions that R10c makes distinguishable |
| **Differential harness schema stable**              | Schema still at `3` with versioned corpus progression (ADR 0033) — no harness-breaking change landed after R10                                                                                                                    | `tests/differential` harness out/`corpus/` manifest + corpus version digest reproducible on all 3 platforms | R11's Tier-1 audit compares the same harness invocation on Linux/Windows/macOS                                                                 |
| **Fail-closed posture unchanged**                   | Every scope labelled `unavailable` in this decision still reports a typed `unavailable` / `unsupported` / `blocked` and performs **no** filesystem mutation or process launch                                                     | `SECURITY.md:210/224` fail-closed notes + `check-project-context` guardrail                                 | A premature "available" would make the parity line dishonest                                                                                   |

If any of the above is still `Active` or `Not due`, the R11 entry review must be **deferred** — not started in parallel with missing predecessors. The Wayfinder `blockedBy` wiring already enforces this; this table is the reviewer checklist.

---

## 2. What full means — four pillars

### 2.1 Effect-boundary enforcement — the two fail-closed primitives stay models, not launches

R11 **does not claim** workspace create/edit/delete application, safe undo, or new checkpoint creation as operational. It proves the **typed unavailable boundary** at parity on both sides.

| Effect                                                                                                           | Current posture (R4 hardening)                                                                                                                                                                                                                                                                                                                                                              | What R11 differential proves (no operation is claimed operational)                                                                                                                                                                                                                                                                           | Core / adapter owner                                                                                                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Workspace create / edit / delete** — `workspace.create_file` / `workspace.edit_file` / `workspace.delete_file` | Report `unavailable` before any write, approval, or checkpoint; `SECURITY.md:210-224` — Node offers no directory-relative `openat`/\ `renameat` primitive; a same-user process can swap a parent/target between identity verification and a pathname-based create, and cleanup could delete a substituted object; historical checkpoint data may be listed but never repaired automatically | Differential scenarios for prepared-mutation + apply (prove `unavailable` + digest/revision binding) + protected-path classification (`AGENTS.md` at any depth, `.siralos/**`) + case-insensitive Windows/macOS matching; corpus reuses the R4 `workspace-prepare` subject and adds `workspace-apply` (typed `unavailable` without mutation) | `siralos-core::workspace` — validated paths, bounds, prepared-effect + checkpoint contracts (no new subsystem); `siralos-adapters::workspace` — containment-safe resolution, bounded complete exact reads |
| **New checkpoint creation + automatic pruning**                                                                  | `unavailable` before creation; storage inspection/reconciliation over the reference metadata layout remains available only for **existing** checkpoints                                                                                                                                                                                                                                     | Differential proves storage inspection over legacy `metadata` layout is deterministic + byte-limited, and that creating a new checkpoint is `unavailable` with no retried deletion                                                                                                                                                           | Same as above — creation/pruning stay unavailable by design until an identity-bound primitive exists                                                                                                      |
| **Private run-directory creation / cleanup**                                                                     | `unavailable` — no directory-relative `mkdirat`-style or delete-by-handle primitive exists (`SECURITY.md:224`)                                                                                                                                                                                                                                                                              | Differential proves run-directory creation is not attempted even when the engine probe would require it; `godot.lsp_session` / `--check-only` remain `unavailable` (see R8/R9 posture)                                                                                                                                                       | `siralos-adapters::process` / sandbox adapter                                                                                                                                                             |
| **Process launch** (`godot.*`, `--check-only`, LSP, API dump)                                                    | Every Godot probe/runner reports `unavailable` and **never spawns** the executable — backend would re-open the staged copy's pathname at spawn time (`ARCHITECTURE.md:420` probe discipline)                                                                                                                                                                                                | Differential proves typed `unavailable` without creating a mirror or deleting anything                                                                                                                                                                                                                                                       | `siralos-adapters::godot` — fixed `fixedProbeArguments` private constructor, no imported argument arrays                                                                                                  |

**R11 exit:** no new `unavailable` is converted to operational merely to close the gate. The gate closes when the _typed unavailable_ observable is at parity.

### 2.2 Security / sandbox conformance — truthful unavailable, never passed as secure

| Concern                                                                                     | Current state (must not be claimed available to close R11)                                                                                                                                                                                                                                                                                     | What R11 proves                                                                                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`AnthropicSandboxRuntimeBackend` host-read allowlist** (`SECURITY.md`, `ARCHITECTURE.md`) | Linux/macOS: deny-by-default allowlist (deny `/`, re-allow workspace when the profile allows + trusted runner executables + minimum system runtime paths; per-execution filesystem/network config, no profile inheritance); Windows: never reported generally available even when setup is complete (host-read capability is explicitly false) | Live conformance `npm run test:sandbox` matches the TypeScript oracle's probe surface while reporting Windows as **skipped loudly, never as passed** (every probe reports `setup-required`/`unsupported` with the exact package-supported command) |
| **Network/outbound, credentials, dynamic-loader injection**                                 | All built-in profiles deny `network.outbound`; child environments are built from an explicit allowlist with deny-listed `*_API_KEY`, `LD_PRELOAD`, `BASH_ENV`, Godot path overrides, etc.                                                                                                                                                      | Conformance proves denied patterns case-insensitively; no provider API reach-through to sandboxed children                                                                                                                                         |
| **Private run-directory invariant** (see §2.1)                                              | Creation/cleanup fail-closed                                                                                                                                                                                                                                                                                                                   | Conformance proves no private directory is created even when a Godot probe is requested                                                                                                                                                            |
| **Fail-closed reporting** (`SECURITY.md` Fail closed)                                       | Backend reports `setup-required`/`dependency-missing`/`unsupported`/`degraded`/`failed` and none are treated as secure                                                                                                                                                                                                                         | R11 differential + doctor report `unavailable` rather than silent fallback                                                                                                                                                                         |

**R11 exit:** `npm run test:sandbox` matches the oracle on available platforms and **reports skipped, not passed**, on Windows — no execution is ever claimed verified on Windows (fail-closed reporting preserved).

### 2.3 Recovery classification — typed, not substring-matched, authority-preserving

Recovery (automatic retry, reconciliation, re-issue) is **not executed** in R11. What R11 closes is the **typed failure surface** that recovery will consume without substring matching — the same discipline R6 remediation already hardened for `Domain` and R7 hardened for provider cancellation:

| Failure dimension (typed host-observed state) | Provider/model side                                                                                            | Example host code             | Why R11 must close this before R12           |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | ----------------------------- | -------------------------------------------- |
| **Retryable**                                 | transient provider throw, network blip, harness timeout — next attempt is fresh-provider-context               | `retryable`                   | Retry is safe                                |
| **Non-retryable**                             | malformed plan, invalid acceptance ref, unknown tool name                                                      | `non-retryable` / `invalid`   | Retry would loop                             |
| **Capability denied**                         | `PermissionDecision::Deny` (including narrower final authority at activation commit — see R6 remediation)      | `CAPABILITY_DENIED`           | Never converted to a grant                   |
| **Stale / conflict**                          | stale lifecycle generation (`STALE_ACTIVATION`), stale prepared activation, revision mismatch                  | `stale` / `conflict`          | Retry must re-derive, not re-issue           |
| **Resource exceeded**                         | input/output/host-call/Memory/fuel bounds, aggregate byte-bound `256 KiB` / `64 KiB` text                      | `resource_exceeded`           | Budget is host-owned                         |
| **Unavailable / unsupported**                 | effect-boundary primitive not mechanically enforceable, `unavailable` workspace apply, `unsupported` API shape | `unavailable` / `unsupported` | Not retryable until Stage 4 primitive exists |
| **Uncertain**                                 | quarantine-preserved original + later target both exist after failed commit                                    | `uncertain`                   | Preserves both copies, never reports success |

Every typed failure has a **stable code** (not a free-form string). Recovery decisions in Stage 4/R11+ must branch on the code, never on `message.includes(...)` — the same rule that governs `DOMAIN_FAILURE` in `tests/domain-conformance/`.

**R11 exit:** R10c's readiness identity + RunManifest make every failure class reachable in differential scenarios; R11's subjects exercise each class at least once and prove the code is stable across Rust/TS.

### 2.4 Cross-platform parity — the Tier-1 matrix as the audit

| Platform    | Audit                                                                       | Expected outcome                                                                                                                                                                                                                                                          | Artifact retained                                             |
| ----------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| **Linux**   | `npm run check:differential` + full repository gate + harness replay stress | 133+ new scenarios at parity (133 → final corpus after R10 subjects; expected ~170-200 at R11 — bump lands in entry review), audit retains exact reference/candidate/audit/failure records                                                                                | Migration audit JSON with source, corpus, protocol provenance |
| **Windows** | Same harness invocation                                                     | **128/128 applicable required Windows scenarios** (today's Windows audit) + 4 explicit platform skips (POSIX-only symlink scenarios, per R7.4-7.5 pattern) + 1 accepted informational deviation — same rule at larger corpus, exact counts recomputed at R11 entry review | Same audit JSON, Windows-specific                             |
| **macOS**   | Same harness invocation                                                     | All applicable required scenarios match; skips explicit, not implicit                                                                                                                                                                                                     | Same audit JSON, macOS-specific                               |
| **Replay**  | `tests/differential` replay harness + harness stress                        | Deterministic — no `Date.now()`/`Math.random()` in host decisions                                                                                                                                                                                                         | Recorded observations replayed in-process                     |

A failed matrix run blocks integration. The `EPERM` on `stdio:'pipe'` capture inside the DSH partial sandbox is an environmental `unknown`, rerun on Tier-1 CI outside the sandbox — never passed as secure.

---

## 3. Measurement discipline — when a hot spot justifies specialization (RUST_STYLE.md leverage principle)

Per `docs/development/RUST_STYLE.md` §§ Engineering priority order + Performance + Performance (quoted in brief):

> _Priority: correctness > security > determinism > type-driven invariant > ownership > local reasoning > maintainability > API quality > testability > performance evidence > formatting — style never overrides correctness; performance never overrides correctness/security/determinism; unsafe/synchronization/dynamic-dispatch/allocation/async require concrete justification; the workspace is `forbid(unsafe_code)`._

> _Evidence-first performance rule (`RUST_STYLE.md:568-589`): review allocation/clone/repeated I/O/parsing/hashing/serialization/sync/process creation/buffering — but optimize in order correctness → invariants → ownership → architecture → **measurement** → optimization → re-measurement. Do not micro-optimize without measurement; `unsafe`, `Arc`/`Mutex`/`RwLock`, dynamic dispatch, unnecessary allocation, and unnecessary async each need concrete justification._

**For R11 specifically:**

| Rule                                        | Application                                                                                                                                                                                                                                                                                                                                        |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **No benchmark without a measured hotspot** | R6/R7 already demonstrated "no benchmark required" where no hotspot was established (provider, tool loop). Apply the same: do not add `criterion` or `proptest` noise for every module. A benchmark harness appears only where a future Stage-4 hot path (e.g., scene-tokenizer throughput, index query latency) is measured to be the bottleneck. |
| **Leverage before specialized search**      | Prefer `BTreeMap` ordering, zero-cost iterator abstractions, and borrow-over-clone where natural (`R7.3` `projection/evidence.rs` does this). Only if measurement shows the gap does a specialized index/hash become justified.                                                                                                                    |
| **Determinism over micro-speed**            | Any optimization that weakens deterministic ordering, revision correctness, cancellation semantics, approval binding, or evidence provenance is a regression even if faster — must be rejected per the priority order.                                                                                                                             |
| **Gate**                                    | Every R11 PR that introduces `unsafe`, `Arc`/`Mutex`/`RwLock`, explicit spawn, or `Box<dyn>` must include a short justification in the PR description tying the choice to measured evidence — matching the pattern in `RUST_MIGRATION.md` port reviews (§ Port review clause).                                                                     |

No optimization milestone is created — optimization is inline, after measurement, never speculative.

---

## 4. Artefacts that constitute PASS — what the Verified commit records

The **R11 Verified** commit (call it `R11`) is the direct predecessor of R12 — it lands only after all R11 entry reviews are PASS and every gate below is observed PASS on the same worktree that becomes `R11`.

### 4.1 Corpus

- Final corpus version bump (from today v15 133 to final — expected bump lands at the last R11 entry review, with manifest/digest regeneration).
- Exact `scenario- and corpus-digest-bound` fixture set + per-subject coverage in the harness out directory (`tests/differential/out` retained per-platform).
- `npm run check:differential` **128+ /128 applicable** on Windows (exact count recomputed), **all applicable required** on Linux/macOS, Tier-1 matrix green.

### 4.2 Security / sandbox

- `npm run test:sandbox` retains the truthful no-op conformance while engine probing is unavailable — Windows skipped loudly, never passed; Linux/macOS available where installed, otherwise reported loudly as unavailable (not as secure).
- Each differential subject that touches a fail-closed boundary has an explicit `unavailable` case with a typed outcome (not silent fallback).

### 4.3 Review

- One **R11 entry review** (mirrors `R7.3 Projection parity` §14 restart) freezing the full subject table and corpus bump before any R11 implementation; optional sub-slice entry reviews for effect-boundary vs sandbox vs recovery vs cross-platform if the entry review so orders.
- `npm run check:rust` (core domain neutrality + `cli → adapters → core`) + `npm run check:architecture` remain green.

### 4.4 Status surfaces (atomic on commit `R11`)

Same seven-surface pattern as [R7 Verified Promotion](../decisions/02-r7-verified-promotion.md) §2, applied to R11:

- `docs/development/PROJECT_CONTEXT.md` head: `Current completed milestone: R11` + `Last verified commit: R11` + `Latest verified executable worktree: R11` + keep R1-R10 historical pointers.
- `docs/development/PROJECT_CONTEXT.md` table row `R11 | …` → `Verified` (and staged `R12: retirement disposition` language unchanged).
- `docs/development/RUST_MIGRATION.md` — new § R11 implementation and acceptance paragraph (like R7.4-7.5), closing the "R11 is the owner of full recovery parity" line with endpoints.
- `ROADMAP.md` R11 row + § Current tail.
- `README.md` + `AGENTS.md` status prose.
- `scripts/check-project-context.mjs` + test — bump the "R11 COMPLETE" expectation.

The commit message is `docs: promote R11 to Verified (\\R11\\)` and lists the 7 surfaces + corpus audit SHA in the body.

### 4.5 No new effect is claimed operational

The Verified commit does **not** flip any `unavailable` to operational merely to close the gate — the gate closes on typed-parity, not on availability.

---

## Self-loop verification (this decision)

| Criterion                                                     | Direct evidence                                                                                                                                                                                                                                                                                                                                 | Status |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Entry conditions named (which R10 artefacts)                  | §1 five-row table: R7 Verified (`02` DoD), R8+R9 entry-reviewed + Verified (`04` boundary), R10a-c entry-reviewed + Verified (`05` dependency chain), harness schema 3 stable, fail-closed posture unchanged — each with required state + evidence + hard-reason                                                                                | pass   |
| What full means — 4 pillars                                   | §2.1 effect-boundary (typed `unavailable` without mutation), §2.2 security (`test:sandbox` truthful unavailable), §2.3 recovery (7 typed failure dimensions with codes, never substring-matched), §2.4 cross-platform (Tier-1 Linux/Windows/macOS digest-bound audit with replay) — each with mechanism, per-row table, and file:line citations | pass   |
| Measurement discipline (leverage principle)                   | §3 quotes `RUST_STYLE.md:46` priority order + `RUST_STYLE.md:568-589` evidence-first rule + table mapping to R11 (no benchmark without hotspot, leverage before specialization, determinism over speed, gate for unsafe/sync/dyn-dispatch)                                                                                                      | pass   |
| PASS artefacts (corpus bump, manifest/digest, harness replay) | §4 five subsections: final corpus version bump + digest-bound fixtures + per-subject coverage + sandbox truthful skip + entry-review freeze + 7-surface atomic promotion (mirrors `02` pattern) — no code lands here                                                                                                                            | pass   |

Evidence ladder: L1 reads of `PROJECT_CONTEXT.md` §14 H1/H2/ICM/H3, `RUST_MIGRATION.md` milestone table + Stage 4 entry, `RUST_STYLE.md` priority + performance, `SECURITY.md:210/224` fail-closed, `ARCHITECTURE.md` workspace/process/sandbox sections, plus R10 decision artifact; L3 porting gate precedent (R7.3 §14); L4 decision markdown itself. No Rust/TS code, no corpus bump, no entry-review beyond this decision.

---

## Out of scope for this decision (per lean ADR 0036)

No Rust module, no run-directory primitive, no sandbox backend claimed available on Windows, no recovery loop, no benchmark, no corpus promotion. General Hooks, multi-agent machinery, TaskGraph, workflow engines, marketplaces, plugin ecosystems, model-router, generic Memory, GUI/TUI remain Future / Not Due. Stages 4-6 remain staged product direction, not commitments.
