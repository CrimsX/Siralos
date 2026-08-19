---
title: "Advisory: Port TerminalSanitizer to siralos-cli drain path"
label: "wayfinder:task"
type: AFK
status: closed
resolution: crates/siralos-cli/src/sanitize.rs + interactive.rs wiring
blockedBy: ["01-r7-5-review-rubric.md"]
priority: P2
---

## Question

Harden `crates/siralos-cli/src/interactive.rs:253-295` `drain_events` — currently writes `ToolLoopEvent::TextDelta { text }` and `ResponseFailed { message }` raw via `writer.write_all` without sanitization.

Resolve:

- Port `apps/cli/src/output/sanitize.ts` `TerminalSanitizer` (C0/C1, CSI, OSC 8, CR/BS rewriting, surrogate-pair tracking across chunks, `flush` dangling-sequence drop) to Rust — or wrap the drain path with a Rust equivalent — so every byte reaching the terminal is neutralized.
- Decide whether `format_context_status` / `format_tools` also need wrapping (they render Host-computed counts/fingerprints — likely trusted — but confirm).
- Preserve determinism: sanitization must not change the byte-equal vocabulary asserted in the R7.5 rubric decision (`docs/wayfinder/decisions/01-r7-5-review-rubric.md` §1).
- Gate: blocks any real (non-deterministic-fake) provider integration (R11 or earlier); does not block R7.5 PASS.

Priority P2, type AFK. Create-then-wire after R7.5 Review Rubric decision. Linked from that decision's §3.
## Resolution

Closed — `crates/siralos-cli/src/sanitize.rs` (port of `apps/cli/src/output/sanitize.ts` TerminalSanitizer: caret/C1/Csi/Osc, stateful push+flush, path helper) + lib.rs registration + `crates/siralos-cli/src/interactive.rs:253` wiring (TextDelta via push, flush on Completed/Cancelled, ResponseFailed/ToolFailed via single-call). 10 sanitizer tests + host_vocab neutrality; 51 siralos-cli tests pass. Gate before real provider unblocked.
