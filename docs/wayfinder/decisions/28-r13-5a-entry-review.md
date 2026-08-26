# Decision — R13.5a Entry Review — Slash-Dispatch Core Fixtures

**Wayfinder ticket:** [R13 Execution Register](../tickets/22-r13-execution-register.md) · label `wayfinder:grilling` HITL
**Map:** [Siralos Roadmap](../siralos-roadmap.md)
**Blocked by:** [R13.5 Entry Review](27-r13-5-entry-review.md) (PASS)
**Decided:** 2026-08-25 (interactive HITL grilling)
**Status:** **PASS — R13.5a scenario set frozen; implementation authorized**

---

## Frozen scenario set (`cli-session`, ~6 case groups, corpus v28)

1. **input-parsing** — empty/whitespace input, plain prompt text, valid
   slash command with args, unknown `/nope`, and catalog completeness
   (every `COMMAND_CATALOG_IDS` entry parses).
2. **session-lifecycle** — scripted input queue drained to EOF → exit code
   0; `/clear` invokes the IO clear hook.
3. **help-and-commands** — `/help` renders byte-equal `formatHelp()`;
   `/commands` lists catalog ids.
4. **status-view** — `/status` renders `buildSessionStatusView` under the
   injected clock and deterministic fakes.
5. **unknown-command** — invalid-command rendering through the sanitizer.
6. **prompt-turn** — one plain prompt drives the bounded single model turn
   over the deterministic fake provider with transcript pairing rendered.

## Mechanics

- Oracle probe drives the REAL TypeScript composition:
  `runInteractiveSession(io, application, info, controls, queue)` with a
  fake `SessionIO` (captured writes, clear flag), a scripted `InputQueue`,
  and the real application from the bootstrap factory (deterministic fake
  provider). Injected clock; no TTY; no ambient environment.
- Rust candidate mirrors behind the same subject name; output comparison is
  byte-exact including sanitizer statefulness.

## Boundaries

System read-only commands (/sandbox, /permissions, /git-status, /diff,
/checkpoints, /undo), godot/develop/gdscript commands, briefing commands,
and deferred seams stay in their later sub-slices per
[decision 27](27-r13-5-entry-review.md).

## Self-loop verification

| Criterion                         | Evidence                                                                                  | Status |
| --------------------------------- | ----------------------------------------------------------------------------------------- | ------ |
| Cases exercise reference behavior | parse-input.ts (51 lines, four parse outcomes) and dispatch switch arms read this session | pass   |
| Human approved the cut            | HITL answer 2026-08-25: "Approve as proposed"                                             | pass   |
| Determinism posture               | scripted queue, fake SessionIO/IO capture, injected clock, fake provider                  | pass   |
