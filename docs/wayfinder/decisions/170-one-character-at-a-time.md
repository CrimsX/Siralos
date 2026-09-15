---
title: "One Character at a Time: the Reveal Is the Frame"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# One Character at a Time: the Reveal Is the Frame

Ticket [130](../tickets/130-threaded-session-liveness.md) · supersedes the
reveal rule of [169](169-reveal-tracks-a-fast-stream.md) · [168](168-c2-step3-wiring-placement.md) ·
[164](164-thinking-display-and-ui-polish.md) · [Map](../siralos-roadmap.md)

## 1. The ruling

Owner, after decision 169 landed the bounded lag: **"i would like it to
display/render one character at a time"**. That is a rule about the STEP, not
about the rate: a painted frame may release exactly one character, never a
chunk and never a catch-up burst.

So the reveal has no clock of its own any more. `reveal_char()` releases the
next character owed (the answer first, the thinking after it) and the PAINT
path is its only caller: one call, one painted frame, one character. Everything
that used to pace it -- the per-second rate, the per-tick cap, the carried debt
and the lag bound of decision 169 -- is gone, because with one character per
frame the frame cadence IS the character rate, and the cadence is a single
constant (`REVEAL_CHAR_INTERVAL`, 6 ms, ~166 characters a second).

## 2. The prerequisite: a frame had to stop costing the whole session

One character per frame means a frame per character, so the frame cost became
the ceiling. It was measured (production draw path, unoptimized build, the
build `cargo run` produces):

| transcript lines | frame before          | frame after |
| ---------------- | --------------------- | ----------- |
| 24               | 5.3 ms                | 2.20 ms     |
| 100              | 7.4 ms                | —           |
| 300              | 9.5 ms                | —           |
| 600              | 11.7 ms               | —           |
| 1200             | 20.6 ms               | —           |
| 5000             | ~85 ms (extrapolated) | 2.25 ms     |

Every frame cloned the whole transcript and wrapped every entry in it, so the
cost grew with the session and a per-character reveal could not be smooth -- and
in a long session it could not even keep up with a slow model. The render now
builds only the rows the viewport shows (`visible_transcript_rows`, tail first):
648 µs at 5000 lines, and the frame is the same 2.2 ms at 24 lines and at 5000.

## 3. What is honestly traded

**The character rate is capped by the frame rate.** At 6 ms a frame, the reveal
sustains ~166 characters a second; a stream faster than that shows text slower
than the model produces it. That is arithmetic, not a defect: one character per
frame at 60 frames a second is 60 characters a second, and the only way to show
more without breaking the one-character rule is to paint more frames.

**Nothing is dropped to hide the lag.** `push_reasoning` applies the
`REASONING_BYTES` bound to ALREADY REVEALED text only, so a fast trace is kept
whole until the reader has seen it (the previous shape could trim away text
nobody had seen, which made the visible row jump). The buffer settles back to
the bound once the reveal catches up.

**The cadence is the one knob.** A machine or a build where a frame is cheaper
can afford a smaller `REVEAL_CHAR_INTERVAL`; the constant is documented with the
measured cost so the trade is visible where it is made.

## 4. Evidence

- `the_reveal_releases_one_character_per_call`: one call, one character, the
  newline being the character that completes the row, the thinking starting only
  after the answer is out.
- `the_character_cadence_is_the_documented_one`: the interval is pinned, because
  it is the character rate and the CPU budget at once.
- `the_visible_window_matches_the_whole_transcript_window`: the bounded builder
  produces exactly the rows the whole-transcript version produced, over 225
  combinations of transcript length, width, height and scroll offset.
- `a_long_session_does_not_make_a_frame_expensive`: 5000 lines render in exactly
  one viewport of rows, and the frame is not slower than a 24-line one (the
  numbers are printed).
- `a_fast_trace_is_never_dropped_before_it_is_shown` and
  `a_multibyte_trace_is_revealed_and_trimmed_on_character_boundaries`: retention
  and byte-index safety.
- The pinned rate test was replaced (the rule it pinned is gone); the pinned
  frames did not move, and the differential holds 352/352.
