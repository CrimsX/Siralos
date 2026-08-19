---
title: "R7.5 Review Rubric — When Is the /context + /tools Candidate PASS?"
label: "wayfinder:grilling"
type: HITL
status: closed
resolution: decisions/01-r7-5-review-rubric.md
blockedBy: []
---

## Question

Decide the exact **PASS/FAIL rubric** for the pending R7.5 independent review of `crates/siralos-cli` (`interactive.rs` + `output.rs`).

Resolve:

- Which string evidence is PASS-critical (byte-equal `/context` and `/tools` vocabulary against `apps/cli/src/output/context.ts` + `apps/cli/src/interactive-session.ts`, the `not yet computed` truthful pre-prompt case, fingerprint 8-char prefix, pressure ratio rounding, and stable/contextual/volatile byte accounting)?
- Which authority and parity evidence must pass (16 focused `siralos-cli` tests + 11 required `context-projection` differential scenarios remain the typed-value evidence; no new differential subject is required — confirm or correct)?
- How to judge the **TerminalSanitizer gap** — `drain_events` in `interactive.rs:253--295` writes `TextDelta` bytes raw vs. the TS single output boundary (`apps/cli/src/output/sanitize.ts`) — is raw write acceptable because `DeterministicFakeProvider` output is deterministic echo, or must Rust add sanitization before PASS?
- Which CLI flag surface is in scope for this slice (only `--help/-h` + `--version/-V` + no-arg interactive; richer command catalog is Not due) and which output must NOT be checked?

Output is a decision, not code: a written rubric a reviewer can execute with `cargo test`, `npm run check:differential`, and string diff inspection. Link rubric to the 4 `siralos-cli::interactive` tests and 3 `output` tests at `crates/siralos-cli/src/`.

Blocked by: none (frontier). Blocks: `02-r7-verified-promotion`.

## Resolution

Closed — decision recorded in [decisions/01-r7-5-review-rubric.md](../decisions/01-r7-5-review-rubric.md). Advisory follow-up filed as [09-advisory-terminal-sanitizer.md](09-advisory-terminal-sanitizer.md). Frontier now: [02-r7-verified-promotion.md](02-r7-verified-promotion.md) and [03-godot-boundaries-research.md](03-godot-boundaries-research.md).
