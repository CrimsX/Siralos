---
title: "Thinking renders above the answer"
label: "wayfinder:ticket"
status: closed
date: "2026-09-21"
supersedes: []
---

# Thinking renders above the answer

Owner bug report (2026-09-21), after the threaded-session and reveal arcs

> thinking should appear above the outputted text from the model

Observed on the live TUI: the answer's completed lines rendered first and the
thinking row drifted underneath them, while the answer's IN-FLIGHT line
rendered BELOW the block -- so the answer jumped across the thinking row every
time a line completed.

**Diagnosis.** Not a reveal defect and not a paint defect. The thinking block
was composed as an appended "extra": `visible_transcript_rows` pushed `extras`
AFTER the stored transcript, so every answer line `reveal_char` had already
committed to the transcript rendered above the block, and only the unfinished
tail (which is not stored transcript) rendered below it.

**Delivered.**

- **The block is anchored** (`reasoning_anchor`): it renders ABOVE the stored
  transcript entry it is anchored to. `visible_transcript_rows` takes a
  `FrameRows` split -- `above` at `above_at`, `below` after the last stored
  entry -- and both render paths share one `transcript_frame_rows`, so the
  frame and the headless buffer cannot disagree about where the block goes. An
  empty `above`/`below` reproduces the plain stored transcript exactly, which
  is what keeps every reasoning-free frame byte-identical.
- **The turn owns the anchor** (`begin_turn` / `end_turn`): the submitted
  prompt arms it with the transcript as it stands, and the turn's FIRST
  streamed reasoning delta takes it. Anchoring at the turn's start -- not at
  that delta -- is what keeps the block above the answer's first line, since
  the reveal commits answer lines while it runs. A turn that never streams
  thinking leaves the block with the turn that produced it.
- **The reveal follows the layout** (`reveal_char`): the thinking is released
  first, because it now renders above. The reader meets the model's output in
  the order the model produced it. This costs the answer nothing in practice --
  the reveal is limited by the frame cost, several times faster than a
  reasoning stream -- and the one-character-per-frame rule is unchanged.

**Evidence.** `thinking_renders_above_the_models_output` (red on the pre-fix
code: `[" Siralos", "> what is 2+2", "the answer is four", "and it is exact",
"▸ thinking ..."]`), `the_block_stays_with_the_turn_that_produced_it`,
`thinking_is_released_before_the_answer_it_sits_above` (watched red against the
old priority), and `the_visible_window_matches_the_whole_transcript_window`,
which now walks the anchor across the start, middle, end and past the end of
the transcript against a whole-transcript reference. Differential parity held
352/352: the pinned `tui-render` frames construct no thinking and do not move.

**Recorded limits.**

- The reasoning buffer still accumulates across turns (bounded to the last
  `REASONING_BYTES`), so a later turn's block carries the retained tail of
  earlier thinking. Per-turn blocks were not built.
- The block is anchored, not committed: scrolling back to an older turn shows
  the block at its own anchor, and the expand/collapse keys act on the single
  live block.
- The release priority is the thinking's now, so reasoning that arrives after
  answer text has already been buffered is released before it. The drain is
  limited only by the frame cost, so this shows under a backlog rather than in
  an ordinary stream; the alternative is what put the thinking on screen after
  the answer it introduces.
- Headless verification only (buffer and frame paths over `TestBackend`); the
  owner has not confirmed the layout on a live terminal.
