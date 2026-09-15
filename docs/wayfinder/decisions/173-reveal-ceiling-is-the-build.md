---
title: "The Reveal's Ceiling Is the Build: 650 Characters a Second, or 2805"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# The Reveal's Ceiling Is the Build: 650 Characters a Second, or 2805

Ticket [130](../tickets/130-threaded-session-liveness.md) ·
[171](171-reveal-tracks-the-model.md) · [170](170-one-character-at-a-time.md) ·
[Map](../siralos-roadmap.md)

## 1. The ask

Decision 171 removed the reveal's cadence and named the frame cost as the only
limiter (~498 characters a second, measured). The owner asked for that ceiling to
be raised. This record is the measurement of what the ceiling actually is, per
build, and the one lever that moves it.

## 2. Measured, same machine, same test

| build                           | backlog drain         | per frame | frames in a 300 ms stall |
| ------------------------------- | --------------------- | --------- | ------------------------ |
| unoptimized (`cargo run`)       | 650 characters/s      | 1.54 ms   | 197 (643 fps)            |
| release (`cargo run --release`) | **2805 characters/s** | 0.36 ms   | 863 (2783 fps)           |

The same numbers come from the widget path alone in each build (592 vs 2572
characters/s), which is the interesting part: **the cost is our own layout, not
the backend**. `render_to_buffer` -- widgets into a fresh buffer, no terminal and
no diff -- is not cheaper than `Terminal::draw` over a TestBackend (it measured
slightly more in both builds). So there is no backend trick to play: the frame
cost is the layout work, and the compiler is what makes it cheap.

## 3. The lever

`npm run siralos:release` (`cargo run --release --locked --bin siralos --`) now
exists and is documented in the README's Run section, because the interactive
TUI's reveal rate IS the build's frame rate: 4.3x more characters a second, which
is above any reasoning stream either way -- 650/s already covers a typical one,
2805/s covers a fast one.

Nothing else changed: no code path, no constant, no behavior. Decision 171's rule
stands (one character per frame, the display rate is the arrival rate); this only
states the constant of proportionality and gives it a switch.

## 4. What would raise it further

- A cheaper frame: the layout is rebuilt every frame, so a cache for the parts
  that did not change is the only in-code lever left, and the measurement above
  says it is worth a few hundred microseconds, not an order of magnitude.
  Not taken: no evidence that any real stream needs it.
- Packaged release binaries, which is packaging work (Stage 6 territory), not a
  reveal change.

## 5. Evidence

- `a_backlog_drains_at_the_frame_rate` prints both numbers and asserts one
  character per frame.
- `a_stall_paints_frames_and_never_stops_the_text` prints the stall counts in the
  same shape.
- Both were run in the two builds above; the release run is a manual measurement,
  not part of `npm run check` (the gate stays debug).
