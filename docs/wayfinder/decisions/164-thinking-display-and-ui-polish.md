---
title: "Thinking Display and UI Polish"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "129"
supersedes: []
---

# Thinking Display and UI Polish

Ticket [129](../tickets/129-thinking-display-and-ui-polish.md) · entry review
[163](163-thinking-display-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record.** Everything below was implemented, verified and
> committed BEFORE this record was written. There was no prior ticket and no
> prior entry review; the approvals are in-chat (the owner's reports and rulings,
> on top of the S1+S2+S3 scope approval in ticket 128). Nothing is backdated.

## 2. The Implemented

| Slice                              | The change                                                                                                                                                                                                                                                                                               | The evidence                                                                          |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| M1 REASONING CHANNEL (\`50f316c\`) | Thinking travels on its own events end to end: the SSE assembler emits \`reasoning\`/\`reasoning_content\` deltas and records the text in the assembled body, \`completion_events_from_body\` reads it back, and the collector bounds it with \`push_reasoning\` -- which appends NOTHING to the answer. | \`reasoning_never_becomes_the_assistant_text\`; the SSE channel + body-replay test    |
| M2 THE COLLAPSED ROW (\`7a5985a\`) | \`reasoning_block_lines\` is pure: nothing when a route never reasons, one row collapsed, a bounded tail expanded; the rows are trailer rows, so they scroll with the conversation and needed no layout surgery.                                                                                         | \`thinking_block_is_absent_collapsed_and_expands_in_place\`                           |
| M3 MID-FLIGHT KEYS (\`d31a356\`)   | \`tui::apply_turn_key\` owns the keys pressed while a turn runs: type-ahead, Right/Left to expand and collapse thinking, Esc to interrupt. The Session budget takes the hard cap (\`MAX_TOOL_ROUNDS\` = 32); the frozen reference default stays 8 for parity.                                            | \`turn_keys_keep_type_ahead_expand_thinking_and_ask_to_interrupt\`                    |
| M4 PAINT DISCIPLINE (\`2a3e653\`)  | Per-event repaints are throttled to \`REDRAW_INTERVAL\`; a key press forces a frame. The loop's own draws stay unconditional.                                                                                                                                                                            | the lag report's cause; differential frames unchanged                                 |
| M5 HONEST PROMPT (\`2a3e653\`)     | The system instructions are product-neutral in BOTH copies (session and harness): no "for Godot Engine development", no GDScript section, and an explicit statement that optional domain intelligence is installed and never assumed.                                                                    | the prompt text is not pinned by the corpus (verified) and the gate is green          |
| M6 LIVE ROW (\`169691b\`)          | The collapsed row previews the newest thinking text, because a line COUNT only moves when a line completes -- which is exactly why it read "line by line".                                                                                                                                               | the block test's live-tail assertion                                                  |
| M7 COLOUR AND PULSE (\`aee5fc4\`)  | \`style_for_transcript_line\` colours failures red and tool activity grey; \`working_line(elapsed)\` pulses one dot per second.                                                                                                                                                                          | \`working_line_pulses_once_a_second_and_errors_render_red\`                           |
| M8 STATIC INDICATOR (\`2f623a1\`)  | The indicator left the transcript for its own layout row ABOVE the input, in the banner colour. The row is conditional (zero height idle) because a permanent row shortens the transcript and moves the pinned frames.                                                                                   | the pinned frame snapshots and the scroll geometry pass unchanged                     |
| M9 CLEAN STATUS (\`34b0237\`)      | The bottom bar drops \`ready\`/\`working\`; the indicator keys off \`busy_since\` (the turn timer) and is cleared when the turn ends; a blank row separates it from the thinking block.                                                                                                                  | 228 cli tests; the coupling is now explicit rather than accidental                    |
| M10 LEFT TO RIGHT (\`fa7534a\`)    | Text waits in a reveal buffer; every paint releases \`REVEAL_CHARS_PER_SEC\` (one tick capped) into the transcript, and the INCOMPLETE line renders as a growing tail. The thinking reveals on the same budget. \`TuiState\` drops \`Eq\` because the debt is a float.                                   | \`reveal_releases_text_at_the_configured_rate\`; the sink bound test drives the clock |

## 3. Scope Limits, Recorded

The pulse and the reveal advance only when a frame is painted, so during a
blocking wait with no provider output (the first byte, a long tool execution)
they hold -- the same constraint as the spinner, and the fix is a socket read
timeout (\`reqwest\` 0.12 blocking has none) or a worker thread behind a decision.
Nothing yet ASKS a route for reasoning, so a model that only reasons when
requested shows no block. The named \`openai\`/\`anthropic\` adapters still send
dotted tool names, and provider-captured recordings carry tool aliases.

## 4. Criteria -> Evidence

| Criterion                                 | Evidence                                                                                      |
| ----------------------------------------- | --------------------------------------------------------------------------------------------- |
| Thinking is visible, expandable, bounded  | M1 + M2 + M6                                                                                  |
| It works while the model is still working | M3 (keys mid-flight) + M9 (the timer, not the status text)                                    |
| The answer reads left to right            | M10 -- one budget for both channels                                                           |
| Failures are obvious, tool noise is quiet | M7                                                                                            |
| The harness describes itself truthfully   | M5 -- product-neutral, domain guidance arrives with the domain                                |
| Pinned behaviour is untouched             | M3's frozen default, M8's zero-height idle row, M4's unchanged frames; differential 352/352   |
| The repository gate holds                 | \`npm run check\` exit 0 at \`fa7534a\` -- 616 core / 326 adapters / 228 cli / 25 conformance |

## 5. Result

The thinking is a first-class part of the session now: it streams, it is
bounded, it can be read while the model is still working, and it is
unmistakably not the answer. The answer itself reads left to right. The two
defects found while writing evidence -- reasoning leaking into the assistant
text, and the indicator depending on a word in the status bar -- are the reason
the loop keeps insisting on a test per claim.
