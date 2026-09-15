---
title: "The Reveal Tracks a Fast Stream Instead of Falling Behind It"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# The Reveal Tracks a Fast Stream Instead of Falling Behind It

Ticket [130](../tickets/130-threaded-session-liveness.md) · C3
[168](168-c2-step3-wiring-placement.md) ·
[164](164-thinking-display-and-ui-polish.md) · [Map](../siralos-roadmap.md)

## 1. The report and the measurement

Owner, after C2 step 3 and C3 landed: "the thinking still seems delayed and not
smooth". The frame cadence was not the problem any more -- C3's own test proves
the UI paints with no provider event at all -- the reveal was.

The reveal paces text at `REVEAL_CHARS_PER_SEC` (240) and caps one frame at
`REVEAL_TICK_CHARS` (480). A stream that arrives faster than the pace therefore
falls behind without bound. Measured on the production `reveal_now` with 16 ms
ticks over two seconds of stream:

| stream                        | arrived | shown (before) | lag (before) | lag (after) |
| ----------------------------- | ------- | -------------- | ------------ | ----------- |
| 960 chars/s (16-char chunks)  | 1920    | 936            | 984          | 157         |
| 1920 chars/s (32-char chunks) | 3840    | 936            | 2904         | 157         |
| 3840 chars/s (64-char chunks) | 7680    | 936            | 6744         | 157         |

Both halves of the report follow from that number. The trace the reader watches
is seconds behind what the model has already produced, and it keeps crawling
after the model has moved on. And once the backlog passes `REASONING_BYTES`
(8192) the trim drops thinking that was never shown at all.

## 2. The rule

- **Paced below the bound.** While a channel is within `REVEAL_MAX_LAG_CHARS`
  (160) of what has arrived, a frame releases only the paced budget, so text
  still grows left to right at a reading speed.
- **Tracked above it.** Past that bound the channel stops being paced and
  closes the gap, at most `REVEAL_TICK_CHARS` per frame -- the same cap that
  keeps a stall from painting a wall of text in one frame. At 60 fps that is
  ~28k chars/s of catch-up, far above any stream, and the settled lag is the
  bound plus one chunk.
- **Per channel.** The answer and the thinking used to share one budget, and
  the answer always spent it first, so a streaming answer froze the thinking
  outright. Each channel carries its own allowance now.

`REVEAL_MAX_LAG_CHARS = 160` is about two lines of an 80-column frame: enough
that a normal stream is still read left to right, small enough that the
collapsed thinking row and its line count stay live.

## 3. Superseded (same day)

The owner's next report -- "i would like it to display/render one character at a
time" -- replaced this rule with a per-CHARACTER one:
[170](170-one-character-at-a-time.md). The pacing and the lag bound are gone; the
reveal releases one character per painted frame. The measurement above still
stands as the record of why a rate budget alone could not work.

## 4. Evidence

- `a_stream_faster_than_the_pace_stays_within_the_lag_bound`: 120 chunks of 40
  chars at 60/s (2400 chars/s), asserting the lag stays under the bound plus one
  chunk, that the revealed prefix never goes backwards, and that no frame
  releases more than a tick's worth. Making the bound unreachable (the pre-fix
  behaviour) fails it.
- `a_stream_at_reading_speed_is_still_paced`: 16 ms releases 3 chars of a
  120-char buffer and the next 100 ms releases 24 -- the pace is intact below
  the bound.
- `reveal_releases_text_at_the_configured_rate` (the pinned rate test) still
  passes unchanged.
- Differential 352/352: the pinned frames do not move, because they are built
  from a state the harness constructs and never reveal.

## 4. Consequence

The reveal is a smoothing device for text that arrives at a human speed, not a
throttle on text that does not. Nothing here changes what is streamed, what is
kept, or what the model sees: it changes when the reader sees it, and it bounds
how far behind that can be.
