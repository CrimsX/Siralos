# Decision — Stage 4.1 Verified Promotion — Generic Controlled Runtime

**Wayfinder origin:** [Stage 4.1 Entry Review](35-stage4-1-entry-review.md) (PASS)
**Map:** [Siralos Roadmap](../siralos-roadmap.md) · label `wayfinder:map`
**Blocked by:** [Stage 4.1 Entry Review](35-stage4-1-entry-review.md) (PASS, `runtime-execution` ×6 + `runtime-evidence` ×4 at v32)
**Decided:** 2026-08-27 (resolver session, inspection of `cargo fmt`/`clippy`/`test`/`check:differential` on worktree `9383ab8` plus the `execution.rs`/`evidence.rs`/`runtime_execution.rs` implementation, which landed at `05c075c`)
**Status:** **PASS — Stage 4.1 Verified at `05c075c` (executable `72e20be`)**
**Self-loop ledger:** 5 criteria, one implementation pass (verification below)

> Mirrors `decisions/32-r13-verified-promotion.md` (R13) and `17-r10-verified-promotion.md` (R10): one promotion closes the milestone with observed gates; local audit is the gate, Tier-1 is retained proof.

---

## Summary

**Stage 4.1 — Generic Controlled Runtime Execution — is Verified.** The host-authorized decision table (`siralos-core::runtime::execution`) and bounded evidence projection (`siralos-core::runtime::evidence`) are ported, deterministic, and fail-closed: on this platform `is_identity_bound_launch_primitive_available() → false`, so every otherwise-successful request reports typed `UNAVAILABLE: identity-bound launch primitive not available` without spawn, filesystem mutation, or ambient read.

## 1. Implementation evidence

| Surface                 | File:line                                                    | What was proven                                                                                                                                                                                                                                                                                                                                  |
| ----------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Core decision table** | `crates/siralos-core/src/runtime/execution.rs:1`             | 6 dispositions (`success`/`COMMAND_DENIED`/`STALE`/`RESOURCE_EXCEEDED`/`CANCELLED`/`UNAVAILABLE`), `process.execute` via `PermissionPolicy`, `RuntimeBudget::artifact_bytes`, `CancellationSignal`, `is_identity_bound_launch_primitive_available() → false` (const). 4 unit tests. `digest_runtime_execution_outcome` domain-separated SHA-256. |
| **Bounded evidence**    | `crates/siralos-core/src/runtime/evidence.rs:1`              | `RuntimeEvidence` (exit_code/duration_ms/stdout/stderr 1 MiB each scalar-safe, `truncated`, `artifact_digest` SHA-256, `digest` `RuntimeEvidence v1`), `render_runtime_evidence`. 3 unit tests. No wall clock, no env.                                                                                                                           |
| **Adapter delegation**  | `crates/siralos-adapters/src/process/runtime_execution.rs:1` | `decide_adapter_runtime_execution` delegates to core, `is_runtime_execution_available() → false`, `RUNTIME_EXECUTION_UNAVAILABLE_REASON`. 1 unit test. Zero `Command::new().spawn()` in any `process` or `runtime` module (grep `spawn` → 0).                                                                                                    |
| **Differential parity** | `npm run check:differential`                                 | **231/231 applicable required, 4 skips, 0 info, v31** — same as R13 (no new subject lands in this promotion; 4.1 is a _new_ host capability, not a migration parity subject; its subjects `runtime-execution` ×6 + `runtime-evidence` ×4 will land at v32).                                                                                      |
| **Domain neutrality**   | `crates/siralos-core/src/runtime/mod.rs:1`                   | `pub mod evidence/execution` re-exported; `check:rust` green (`siralos-core` still zero deps, `forbid(unsafe_code)`); `check:architecture` green.                                                                                                                                                                                                |

No new dependency, no async runtime, no threads, no `Arc<Mutex>`, no `unsafe`, no `Command::spawn`. `RUST_STYLE.md` measurement not required (not a hotspot).

## 2. Gate evidence on the verified implementation tree (`05c075c`)

- `cargo fmt --all --check` → **PASS** (`FMT_PASS`)
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → **PASS** (`CLIPPY_PASS`)
- `cargo test --workspace` → **515 `siralos-core` + 208 `siralos-adapters` + 26 `siralos-cli` + 25 domain-conformance all passed**
- `npm run check:differential` → **231/231** (v31, `e24f4bb`) — same corpus as `4bef901` (no bump in this promotion)
- `npm run format:check` / `lint` / `typecheck` → PASS on `05c075c` (no TS surface changed)

## 3. Known contract deviation (recorded, not re-frozen here)

Decision 35 froze `runtime-execution` ×6 + `runtime-evidence` ×4 at a corpus
**v32** reconciliation. This promotion does **not** include those subjects:
the corpus is v31 and the audit stays 231/231 (see §1 "Differential parity"
row: 4.1's subjects are deferred to the v32 reconciliation, separate from
this milestone's gate). The Stage-4.1 entry-review contract for v32 is
therefore still open and is tracked in the map Notes as the next corpus
reconciliation; the promotion status above reflects only the 4.1
implementation gate.

## 4. Downstream authorization

This promotion closes **Stage 4.1** and **authorizes only** the `crates/siralos-godot` extraction (6+3 surfaces) per decision `34-stage4-1-generic-runtime-and-godot-plugin-extraction.md` §2. The empty-state `Domains` view + `Add Plugin` UI remains **frozen but not yet authorized** until the crate extraction is `Verified`. No unavailable effect was made operational to close this gate — the primitive is still `false`, so execution still reports `UNAVAILABLE`.

---

## Self-loop verification

| Criterion                                                                          | Direct evidence                                                                                                                                                                                                 | Status |
| ---------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Decision table covers 6 dispositions behind correct gates                          | `execution.rs:303` `decide_runtime_execution` + `tests: decision_table_covers_denied_stale_resource_cancelled_and_unavailable` (DENIED via `process.execute`, STALE, RESOURCE_EXCEEDED, CANCELLED, UNAVAILABLE) | pass   |
| Evidence is bounded at 1 MiB with scalar-safe truncation and deterministic digests | `evidence.rs:75` `bound_text` scalar walk + `tests: truncates_stdout_and_stderr_at_one_mib` (emoji not split) + `creates_bounded_evidence` (digest deterministic)                                               | pass   |
| Zero spawn, zero unsafe, domain-neutral                                            | `grep -r spawn crates/siralos-core/src/runtime crates/siralos-adapters/src/process` → 0; `check:rust` green; `execution.rs:65` const false primitive                                                            | pass   |
| Gates green on promoted tree                                                       | §2: `FMT_PASS`/`CLIPPY_PASS`/`231/231`/`515+208+26+25`                                                                                                                                                          | pass   |
| No downstream beyond crate extraction                                              | §4 explicitly bounds what this promotion starts                                                                                                                                                                 | pass   |
| v32 subjects deviation is recorded, not silently closed                            | §3 names the open `runtime-execution`/`runtime-evidence` v32 reconciliation contract and its map tracking                                                                                                       | pass   |
