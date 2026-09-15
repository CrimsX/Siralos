---
title: "The Threaded-Session Evidence Pack: Frames, Cancel Latency, Idle Bytes"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# The Threaded-Session Evidence Pack: Frames, Cancel Latency, Idle Bytes

Ticket [130](../tickets/130-threaded-session-liveness.md) · C4 ·
[167](167-c2-session-cannot-cross-threads.md) · [171](171-reveal-tracks-the-model.md) ·
[Map](../siralos-roadmap.md)

## 1. What this is

Ticket 130's C4 slice: the numbers behind the arc's claims, taken from the
production paths rather than restated. It was deferred when the ticket closed --
the behavioural halves were already covered -- and the owner asked for it
explicitly afterwards, so it is its own slice with its own record, exactly as the
closing note said it would be.

## 2. The measurements

**Frames during a stall.** `a_stall_paints_frames_and_never_stops_the_text` holds
the worker silent for 300 ms with 2001 characters owed and runs the live
progress closure: one character released, then the production draw path over a
100x30 TestBackend.

> stall 300ms: 197 frames (643 fps), 197 characters released, 2001 owed at the
> start, 306 ms wall

Every frame released exactly one character (the assertion), the text never
stopped, and the backlog outlived the stall -- nothing was dropped to keep the
rate up. The frontend cannot tell a stalled provider from a stalled worker, and
that is the point of the boundary: both are silence on the channel.

**Cancel latency.** `a_stalled_turn_is_cancelled_within_an_event_interval` drives
the REAL `run_worker_loop` on a thread with a session whose every event costs
20 ms, drains two events so the turn is genuinely running, then sets the flag.

> cancel latency after a 20ms event interval: 20.05 ms (1.0 intervals)

The cancel is an external flag polled BETWEEN events (decision 167), so its
latency is one event interval and does not grow with the length of the turn: the
same turn would otherwise have run for its remaining ~760 ms.

**Idle frames.** `an_idle_frame_is_byte_identical_and_releases_nothing`: with
nothing owed, 64 sweeps of the loop's release path leave the state untouched and
two frames are byte-identical -- the property the pinned `tui-render`
differential subject depends on (352/352 applicable required holds).

## 3. What the pack does not claim

- No live-terminal measurement: these are the production code paths driven
  headlessly. A real terminal adds I/O the pinned frames already cover.
- No live-provider measurement: the stall is a scripted silence, which is the
  property the frontend sees whatever the provider is doing.
