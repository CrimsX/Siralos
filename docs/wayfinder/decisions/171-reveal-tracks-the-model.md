---
title: "The Reveal Has No Cadence: It Tracks Whatever the Model Produces"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# The Reveal Has No Cadence: It Tracks Whatever the Model Produces

Ticket [130](../tickets/130-threaded-session-liveness.md) ·
[170](170-one-character-at-a-time.md) · [168](168-c2-step3-wiring-placement.md) ·
[Map](../siralos-roadmap.md)

## 1. The ask

Owner, after decision 170: **"are you able to match the speed the model produces
it?"** and then **"this is model agnostic right? it will match depending on the
model's speed?"**.

## 2. The rule

**Yes, and by construction: nothing in the reveal knows what a model is.** The
reveal reads two text buffers (`stream_buffer` for the answer, `reasoning` for
the thinking) and releases one character per call. Those buffers are filled by
`TextDelta` / `ReasoningDelta` events through the one provider-neutral bridge,
and every provider -- deterministic fake, generic OpenAI-compatible,
Anthropic-messages, replay -- emits exactly those. There is no rate, no model
id, no provider id and no token accounting anywhere on the path.

Decision 170's fixed 6 ms cadence was, in effect, a claim that the display
should run at a fixed 166 characters a second whatever the model does. It is
gone. `paint_interval(owed, idle)` returns ZERO while anything is owed, so a
painter paints the next frame as soon as the last one is done, and the ordinary
cadence applies again once nothing is owed (no spinning while the model is
silent). The display rate is therefore the ARRIVAL rate, whatever produces it.

## 3. The measured ceiling

The only limiter left is what a frame costs. Measured on the production draw
path in the unoptimized build the dev flow runs (`cargo run`):

- 2.0 ms per frame with the reveal owing text (100x30 viewport), so
- **498 characters a second**, measured end to end by
  `a_backlog_drains_at_the_frame_rate` (250 characters, 250 frames, one
  character per frame) -- several times any reasoning stream, and several times
  cheaper again in a release build, because the cost is layout and diffing, not
  I/O.

Above that ceiling a burst is shown slower than it arrived, and a long one takes
proportionally longer to display; nothing is dropped (decision 170's retention
rule). That is arithmetic, and it is a property of the machine and the build,
never of the model.

## 4. Consequences

- A slow model: the UI waits on its events and paints one character per frame at
  that model's pace (the idle cadence between events, no busy loop).
- A fast model: frames are painted back to back, so the text keeps up and the
  CPU is the frame cost times that rate -- the price of matching a fast stream
  with a one-character step.
- A chunked stream: each chunk drains at the frame rate, which is the same
  average rate the model produced it at.

## 5. Evidence

- `the_painters_are_unthrottled_while_text_is_owed`: `paint_interval` is ZERO
  while owed, and the given idle interval otherwise, for both the redraw
  interval and the idle poll.
- `a_backlog_drains_at_the_frame_rate`: 250 characters in exactly 250 frames,
  with the rate printed (498 characters/s in this build).
- The sink's coalescing test now asserts the intent: a delta that is still owed
  asks for its own frame, and the ordinary interval coalesces once nothing is
  owed.
- `the_idle_poll_keeps_the_character_cadence_while_text_is_owed` (source check):
  the loop's idle wait goes through `paint_interval`.
- The pinned frames did not move; differential 352/352.
