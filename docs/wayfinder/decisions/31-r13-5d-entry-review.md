# Decision — R13.5d Entry Review — Full CLI-Session Closure

**Wayfinder ticket:** [R13 Execution Register](../tickets/22-r13-execution-register.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [R13.5 Entry Review](27-r13-5-entry-review.md) (PASS) + R13.5c landed (`8e4c106`, parity held 231/231 at corpus v30)
**Decided:** 2026-08-26 (resolver session, HITL grilling over reads of `apps/cli/src/interactive-session.ts:137-325` dispatch switch (47 catalog entries → ~75 slash variants), `apps/cli/src/session/*`, `crates/siralos-cli/src/harness_cli_session.rs:116-450`)
**Status:** **PASS — R13.5d scenario set frozen; implementation authorized**

---

## Frozen scenario set (`cli-session` closure, ~7 groups, corpus v31)

All cases extend the existing `cli-session` subject (no new subject) and are exercised through the scripted `InputQueue` + fake `SessionIO` + `TerminalSanitizer` seam already used for R13.5a. No filesystem, no network, no wall clock beyond the injected `NowMs` for `/status` timestamps.

| #   | Case                            | What it proves                                                                                                                                                                                                                                                             |
| --- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `godot-commands-unavailable`    | `/godot` / `godot-installations` / `godot-project` / `godot-doctor` / `godot-probe` / `godot-probe-status` / `godot-knowledge` / `godot-knowledge-refresh` / `godot-api` each render the typed `unavailable` posture (no process, no mutation) with sanitizer-exact output |
| 2   | `gdscript-commands-unavailable` | `/gdscript-check` / `gdscript-diagnostics` / `gdscript-lsp` / `gdscript-lsp-stop` / `gdscript-hover` / `gdscript-complete` / `gdscript-definition` each report `unavailable`/`not running` without launching LSP                                                           |
| 3   | `develop-commands-unavailable`  | `/develop` / `plan` / `development-status` / `quality` / `review-change` render the typed `unavailable`/`no active task` posture; `/develop` never mutates                                                                                                                 |
| 4   | `system-commands-unavailable`   | `/sandbox` / `permissions` / `git-status` / `diff` / `checkpoints` / `undo` / `commands` / `cancel` render their read-only or typed `unavailable` status (no checkpoint, no Git spawn)                                                                                     |
| 5   | `input-queue-ownership`         | Scripted queue drained to EOF returns exit 0; `/clear` increments `cleared` exactly once; interleaved prompt + command ordering preserved; empty/whitespace never writes                                                                                                   |
| 6   | `sanitizer-boundary`            | Provider/tool output containing C0/C1, ANSI CSI/OSC, and surrogate pairs is sanitized via `push`+`flush` exactly as R7.5 rubric — `TerminalSanitizer` statefulness proven by split-sequence cases                                                                          |
| 7   | `session-ordering-determinism`  | Two identical scripted sessions (`/status` → prompt → `/status` → `/help`) produce byte-identical `writes` arrays; workspace/config canonicalization `<workspace>`/`<config>` holds                                                                                        |

## Mechanics

- Oracle probe extends `cli-session-oracle.mjs` with 7 new case groups that drive the **real TypeScript** `runInteractiveSession` over the full 47-entry catalog; Rust side extends `harness_cli_session.rs` behind the same `cli-session` subject. Placement obeys `cli → adapters → core`.
- Corpus bumps **v31** at the R13.5d reconciliation commit; schema stays 3.
- **Determinism:** one injected clock per session for `/status` timestamps, no TTY, no ambient env, no network.
- **Fail-closed:** `godot`/`develop`/`gdscript` commands never launch a process; `sandbox`/`git`/`checkpoints` report typed `unavailable`/`none` truthfully; sanitizer is the single output boundary.

## Boundaries — not in R13.5d

- No new capability, no operational effect, no Stage 4 work.
- No product feature beyond TypeScript behavioral parity; lean-porting discipline applies.
- No engine probe, LSP, or checkpoint mutation becomes operational — all remain typed `unavailable`.

## Authorization

Implementation of R13.5d is authorized against this frozen set; landings are recorded in the [R13 Execution Register](../tickets/22-r13-execution-register.md).

---

## Self-loop verification

| Criterion                                    | Direct evidence                                                                                                                                                                                   | Status |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference-observable behavior | `interactive-session.ts:137` exhaustive switch counted 47 catalog ids → 7 new groups; `harness_cli_session.rs:116` `parse_input` already covers catalog; `sanitize.rs:44` stateful `push`+`flush` | pass   |
| Determinism posture preserved                | § Mechanics injects one fixed clock; scripted queue + fake IO + sanitizer only; no fs/network/TTY                                                                                                 | pass   |
| Overlap resolved, no double port             | R13.5a covered `help/status/clear/commands` + input-parsing; R13.5b/c covered briefing/manifests/seams; R13.5d owns only remaining ~75-command surface + queue/sanitizer                          | pass   |
| Human decided the material cuts              | HITL answer 2026-08-26: “7 groups as proposed, v31, all remaining commands as unavailable, queue/sanitizer included”                                                                              | pass   |
