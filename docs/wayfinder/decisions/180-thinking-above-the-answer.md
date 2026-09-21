---
title: "The Thinking Renders Above the Answer It Explains"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-21"
ticket: "136"
supersedes: []
---

# The Thinking Renders Above the Answer It Explains

Ticket [136](../tickets/136-thinking-above-the-answer.md) · Map ·
[164](164-thinking-display-and-ui-polish.md) ·
[170](170-one-character-at-a-time.md) · [171](171-reveal-tracks-the-model.md)

## 1. The report

Owner, 2026-09-21: "thinking should appear above the outputted text from the
model".

The block was appended after the stored transcript, so the answer's committed
lines rendered above it while the answer's unstored in-flight line rendered
below it. The inversion was structural, not a paint ordering: `reveal_char`
commits a line into the transcript when its newline is released, so by the time
the reader saw thinking at all the answer was already above it.

## 2. The rule

- **Placement is part of the row model.** `visible_transcript_rows` composes
  `FrameRows` in reading order: the stored entries before the anchor, `above`
  (the thinking block), the stored entries after it, then `below` (the answer's
  growing line, the indicator gap). Appending the block was the defect.
- **The turn owns the anchor.** `begin_turn` arms `reasoning_anchor` with the
  transcript as it stands after the submitted prompt; the turn's first
  `push_reasoning` takes it. Anchoring at the turn's START rather than at that
  delta is what keeps the block above the answer's FIRST line, because the
  reveal commits answer lines while the turn runs.
- **A turn that never reasons does not move the block.** `end_turn` disarms
  the pending anchor instead of re-anchoring, so the block stays above the
  answer it explains rather than following the conversation down.
- **The reveal follows the layout.** `reveal_char` releases the thinking
  first. [170](170-one-character-at-a-time.md) and
  [171](171-reveal-tracks-the-model.md) fix the STEP (one character per painted
  frame, no cadence of its own); this decides only the ORDER, and the order now
  matches both the layout and the order the model produced the text in.

## 3. Evidence

- `thinking_renders_above_the_models_output`: the reported scenario, rendered
  through the production `draw` path. Red before the fix, with the frame's own
  rows as the counter-example.
- `the_block_stays_with_the_turn_that_produced_it`: three turns -- reasoning,
  none, reasoning -- pinning that the block opens above its own answer, keeps
  its place when a turn streams no thinking, and re-anchors forward when one
  does.
- `thinking_is_released_before_the_answer_it_sits_above`: both buffers owed, so
  the release priority is the only variable. Watched red against the old
  priority before being accepted green.
- `the_visible_window_matches_the_whole_transcript_window`: the bounded window
  still equals the whole-transcript reference for every width, height and
  scroll offset, with the anchor walked across the start, the middle, the end
  and past the end (which clamps). This test found a real ordering mistake in
  its own reference implementation while the change was being written.
- Differential parity held 352/352: the pinned `tui-render` subjects construct
  no thinking and no in-flight line, so an empty `FrameRows` reproduces the
  stored transcript byte for byte.
- `npm run check` exit 0 (core 626 / adapters 364 / conformance 25 / cli 304).

## 4. Consequence

The thinking block now reads as the model's preamble to its own answer -- above
it, revealed before it -- and the answer no longer jumps across the block as
its lines complete. What is retained, bounded, streamed, or sent to the
provider is unchanged: this is a placement decision plus the release order that
placement implies.

Recorded limits (also in the ticket): the reasoning buffer still accumulates
across turns, so a later block carries the retained tail of earlier thinking;
the block is anchored rather than committed, so the expand/collapse keys act on
the single live block; the release priority is the thinking's now, which shows
only under a backlog because the drain is limited by the frame cost; and
verification is headless, not a live terminal.
