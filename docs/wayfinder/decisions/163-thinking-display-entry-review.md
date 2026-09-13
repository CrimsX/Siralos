---
title: "Thinking Display and UI Polish Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "129"
supersedes: []
---

# Thinking Display and UI Polish Entry Review

Ticket [129](../tickets/129-thinking-display-and-ui-polish.md) · entry review
[the TUI liveness entry review](161-liveness-streaming-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record -- the inversion, stated plainly.** This entry review was
> written AFTER the change was implemented, verified and committed (nine
> commits, listed in the ticket, 2026-09-12). There was no prior ticket and no
> prior entry review. The approvals that exist are in-chat: the owner's bug
> reports and the rulings quoted in the ticket, on top of the S1+S2+S3 scope
> approval recorded in ticket 128. Nothing more is claimed, and nothing is
> backdated. The verdict below is a retroactive PASS over the committed diff,
> not a pre-commit authorization.

## 2. The Fixes

| Fix                             | The change                                                                                                                                                                                                                          | The evidence                                                                                      |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| F1 REASONING IS ITS OWN CHANNEL | \`ModelEvent::ReasoningDelta\` and \`ToolLoopEvent::ReasoningDelta\`; the SSE assembler emits and records it; the collector bounds it separately; the session streams it live.                                                      | \`reasoning_never_becomes_the_assistant_text\`; the SSE channel/replay test                       |
| F2 THE COLLAPSED ROW            | One disclosure row, Right expands, Left collapses, absent when a route never reasons.                                                                                                                                               | \`thinking_block_is_absent_collapsed_and_expands_in_place\`                                       |
| F3 MID-FLIGHT KEYS              | \`tui::apply_turn_key\` keeps type-ahead, expands thinking and reads Esc WHILE a turn runs -- the arrows used to be read and discarded.                                                                                             | \`turn_keys_keep_type_ahead_expand_thinking_and_ask_to_interrupt\`                                |
| F4 THE ROUND BUDGET             | The frozen reference default (8) is untouched for parity; the Session composes with the hard cap the same rules allow (32), which is CLI policy.                                                                                    | the \`over-budget-round\` scenario still pins its own limit at parity                             |
| F5 PAINT DISCIPLINE             | Per-event repaints are throttled; a key press forces a frame -- the lag report and the expand delay had the same cause.                                                                                                             | 228 cli tests; the differential frames unchanged                                                  |
| F6 AN HONEST SYSTEM PROMPT      | The "harness for Godot Engine development" framing and its GDScript section are gone from BOTH copies; optional domain intelligence is installed explicitly and never assumed.                                                      | the prompt text is not pinned by the corpus (verified: 0 hits) and the full gate is green         |
| F7 ALIVE WHILE STREAMING        | The collapsed row previews the newest thinking text; failures render red, tool activity grey; the \`working\` state pulses once a second.                                                                                           | \`working_line_pulses_once_a_second_and_errors_render_red\`; the block test's live-tail assertion |
| F8 STATIC AND SEPARATE          | The indicator owns a CONDITIONAL layout row above the input (zero height when idle, so the pinned frames stay byte-identical), in the banner colour, separated from the thinking block; the bottom bar drops \`ready\`/\`working\`. | 228 cli tests including the pinned frame snapshots and the scroll geometry                        |
| F9 LEFT TO RIGHT                | The transcript is line-based, so a line appeared only when finished. Text now waits in a reveal buffer; the incomplete line renders as a growing tail on one budget shared by answer and thinking.                                  | \`reveal_releases_text_at_the_configured_rate\`; the sink bound test drives the clock             |

## 3. Criteria -> Evidence

| Criterion                                          | Evidence                                                                                                 |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| Thinking is visible and expandable, mid-flight too | F1 + F2 + F3 + F7                                                                                        |
| Text reads left to right in BOTH channels          | F9 (answer and thinking share one budget)                                                                |
| The UI never looks frozen while text arrives       | F5 + F7 + F8                                                                                             |
| What cannot be live is recorded, not implied       | the ticket's limits: the pulse and reveal hold on a silent blocking wait; nothing requests reasoning yet |
| Pinned behaviour is untouched                      | F4's frozen default, F8's zero-height idle row, F9's frame-neutral idle path; differential 352/352       |
| The repository gate holds                          | \`npm run check\` exit 0 at \`fa7534a\` -- 616 core / 326 adapters / 228 cli / 25 conformance            |

## 4. Result

**PASS (retroactive), with recorded limits.** The thinking is now visible,
expandable, bounded, and moving as it arrives, and the answer reads left to
right instead of appearing in provider-sized lumps. Two findings are worth more
than the features: writing the S3a test exposed reasoning leaking into
\`assistant_text\` (it would have become the answer and the history), and removing
the word \`working\` from the bottom bar revealed that the indicator had been
keying off that text -- it now keys off the turn timer, which is the coupling it
should always have had.
