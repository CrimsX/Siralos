---
title: "Threaded Session for UI-Owned Liveness Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# Threaded Session for UI-Owned Liveness Entry Review

Ticket [130](../tickets/130-threaded-session-liveness.md) · entry review
[the thinking display entry review](163-thinking-display-entry-review.md) ·
[Map](../siralos-roadmap.md)

## 1. The Problem, Measured

The owner reported that streamed text "seems to display, stop, display". The
cause is structural: **frames are produced by provider events**. The TUI loop
blocks inside the session's provider read, so between two deltas there is no
frame at all and nothing can advance -- the reveal's step is literally the frame
interval (\`rate x interval\` characters), the pulsing \`working\` dots hold, and the
spinner cannot animate during the first-byte wait. Two exits were presented to
the owner, both needing a clock independent of the stream:

1. a socket read timeout, so a blocked read returns "nothing yet" periodically
   (\`reqwest\` 0.12's blocking builder exposes no such knob -- attempted, the
   compiler refused); or
2. **a worker thread running the session while the UI keeps its own clock**.

The owner chose (2) on 2026-09-12. That choice is the approval this review
records; nothing else is claimed and nothing is backdated.

## 2. What Is Being Authorized

The arc in ticket 130, slices C1-C4: a boundary inventory (C1), the worker with
its channels, cancel flag and single replay flush (C2), the UI loop with a
short-poll input path and a 16 ms tick that drives reveal, pulse and draw (C3),
and an evidence pack that counts frames during a deliberately stalled provider
(C4).

## 3. Invariants the Entry Review Fixes

| Invariant                     | Why it is the gate                                                                                                                |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Single interactive-read owner | The input queue must stay the ONE reader of the terminal: the worker never touches it, or key handling races the renderer.        |
| A thread is not a capability  | The worker composes the SAME session, policy and approval gate. No new authority may be smuggled in by the concurrency change.    |
| Core stays single-threaded    | The differential corpus (ADR 0033) and its synchronous turn assumptions must be untouched; the concurrency lives at the CLI edge. |
| One replay flush, one owner   | Recordings are flushed exactly once by whoever owns the session; two owners would corrupt the store.                              |
| No new persistence            | Unchanged from the decision 78 contract.                                                                                          |
| Stdio is not quietly changed  | It keeps its present path unless a separate decision moves it.                                                                    |

## 4. Criteria -> Evidence (to be satisfied by C4)

| Criterion                                     | Evidence planned                                                           |
| --------------------------------------------- | -------------------------------------------------------------------------- |
| The UI paints with no provider events         | frame count during a stalled fake provider, over a known wall-clock window |
| The reveal and the pulse advance while silent | the same window, asserting the revealed count and the dot phase both moved |
| Cancellation lands within one tick            | cancel latency measured from the flag to the terminal event                |
| The session's behaviour is unchanged          | differential 352/352, plus the tool-loop scenarios                         |
| Idle frames are untouched                     | the pinned TUI frame snapshots                                             |

## 5. Verdict

**PASS -- authorized as the next implementation arc (C1 first).** The risk is
named in the ticket and it is not the thread: the TUI's slash commands, the
context pane, approvals and the replay flush all touch the session today, and
every one of them moves behind a channel. C1 exists to inventory exactly that
before C2 writes any concurrency.
