---
title: "The TUI Usability Pass Entry Review"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "104"
supersedes: []
---

# 110 — The TUI Usability Pass Entry Review

Ticket [104](../tickets/104-tui-usability-pass.md) · entry review [103](103-siralos-tui-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL): U1–U8 approved from the user's real-usage report — two defects root-caused (the keystroke redraw starvation and the command-vocabulary silence), the palette, timestamps, status provider/model, the /provider + /model display commands, and the /evolve discovery command added; modes are not applicable (no mode system exists); runtime provider/model switching and evolution execution from a slash command are out of scope.**

## 2. Fixes U1–U8 — as approved

- U1 INPUT LATENCY (defect): the loop drains the event queue (poll with a short timeout until empty) and draws ONCE per drained batch — keystrokes display immediately; the bounded idle poll remains for the ready-redraw. The submit-during-drain dispatch still freezes the redraw for its duration (the documented T1 limitation, unchanged).
- U2 COMMAND PALETTE: typing / opens a palette popup above the input line listing the command catalog — derived from ONE new catalog fn over the SAME SlashCommand vocabulary (no parallel list) — filtered live by the current input; display-only (Enter still submits the typed line); bounded height.
- U3 UNKNOWN-COMMAND HONESTY (defect): an unknown /-command in the TUI produces an explicit line "unknown command - available: ..." listing the catalog instead of falling through to the prompt path. TUI-only; the stdio fall-through is untouched.
- U4 TIMESTAMPS: transcript entries carry caller-supplied timestamps rendered as a dim line below each message; live sessions stamp with UTC HH:MM:SS computed std-only (civil-from-days math, no new dependency, labeled UTC); differential fixtures carry fixed values so the pinned frames stay deterministic. The tui-render record builder and scenarios re-pin at corpus v77.
- U5 STATUS PROVIDER/MODEL: the status line displays the applied profile's provider / model (e.g. example-vendor / model-a) or "no provider configured" honestly; sourced from the same composed profile the session already holds.
- U6 MODES: NOT APPLICABLE — Siralos has no mode system (approval is per-tool-call, profiles are startup config); shift+tab is recorded as unused; no change.
- U7 /provider AND /model: two new display-only SlashCommand variants in the SHARED vocabulary — /provider prints the applied provider (and credential availability), /model prints the model; both read the same composed profile source as U5; they land in the shared parse + both dispatchers (an additive improvement for stdio too — the worker updates any stdio test that pinned the old fall-through for these inputs, recording it); runtime switching is OUT OF SCOPE.
- U8 /evolve DISCOVERY: a new display-only /evolve variant listing the four Stage 6 bounded evolution surfaces (corpus, workflow, proposal, packaging — crates/siralos-core/src/evolution.rs, decisions 58-59) and stating that execution is host-gated (escalation Profile->Host per the Stage 6 design); it lands in the shared vocabulary + catalog; RUNNING an evolution from the command is OUT OF SCOPE.

## 3. Criteria → evidence

| Criterion                             | Evidence                                                                                                                                                                                                                                                                                                                                                                  | Status |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| U1 input-latency root cause diagnosed | interactive.rs ~1687-1924: the loop handles ONE key event per iteration and the `continue` after non-submit keys SKIPS the loop-bottom draw (~1907), so keystrokes render only when the 100ms poll times out; poll(100ms) + continue-skip at ~1835 + loop-bottom draw at ~1907 pins the starvation. Fix authorized as drain-then-draw-once with short poll.               | pass   |
| U3 vocabulary silence diagnosed       | parse_slash_command runs on Enter; vocabulary is only context/tools/domains*/prompt/exit, there is no help listing, and an unknown /-command falls through to the PROMPT path producing silence. Fix authorized as TUI-only unknown-command honesty.                                                                                                                      | pass   |
| Stage 6 evolution gap diagnosed       | The Stage 6 evolution surfaces exist as core semantics (crates/siralos-core/src/evolution.rs, decisions 58-59: corpus, workflow, proposal, packaging) + differential subjects but no session command surfaces them. Fix authorized as additive shared /evolve discovery command.                                                                                          | pass   |
| Red lines held                        | Sanitizer is the single output boundary; the input queue the single read owner; the command catalog the vocabulary source; approvals host-gated; no threads; no persistence; the stdio session/render behavior stays unchanged except where an additive shared command is explicitly added (U7/U8); decisions <=109 untouched.                                            | pass   |
| Palette / timestamps / status scope   | U2 palette is display-only over the single catalog; U4 timestamps are caller-supplied with std-only UTC HH:MM:SS and fixed fixtures; U5 status reads the same composed profile the session already holds; U6 modes recorded not-applicable; U7/U8 are display-only in the shared vocabulary and both dispatchers; runtime switching and evolution execution out of scope. | pass   |
| Out of scope recorded                 | Modes are not applicable (no mode system exists); runtime provider/model switching and evolution execution from a slash command are out of scope.                                                                                                                                                                                                                         | pass   |

## 4. Result

Entry review PASS: the usability pass is authorized as U1-U8 with /provider, /model, and /evolve display-only; modes recorded not-applicable; runtime switching and evolution execution out of scope.
