---
title: "Threaded session for UI-owned liveness"
label: "wayfinder:ticket"
status: open
date: "2026-09-12"
supersedes: []
---

# Threaded session for UI-owned liveness

Owner report: "does seem smooth the text displaying, seems to display, stop,
display, etc". The diagnosis is not the renderer. **Frames happen on provider
events**: the TUI loop blocks inside the session's provider read, so between two
deltas there is no frame, nothing advances, and the reveal, the pulsing dots and
the spinner all freeze. Measured consequence: the reveal's step IS the frame
interval (\`rate x interval\` characters), so a slow provider turns the answer into
"display, stop, display".

Two exits were presented; the owner chose the second: **a worker thread running
the session while the UI keeps its own clock**.

## Shape (agreed, to be entry-reviewed before code)

The UI thread owns the terminal, the input queue, the tick and the render. A
worker thread owns the composed session and the provider. They talk over
channels:

- UI -> worker: one prompt, the slash commands that touch the session, cancel.
- worker -> UI: the session's \`ToolLoopEvent\`s, the record-replay flush result,
  and a terminal "turn done" signal.

The UI's loop then becomes: poll crossterm with a short timeout, run the 16 ms
tick (reveal, pulse, draw), drain the event channel. Liveness stops depending on
the provider's cadence.

## Slices

| Slice | Content                                                                                                                                                                                          | Acceptance                                                                                                               |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| C1    | Boundary inventory and choice: where the thread boundary goes, what moves, what stays. The session already composes its own provider, so the worker may build it and core stays single-threaded. | a written inventory of every touch point (turn, commands, approvals, context pane, replay flush) with the owner for each |
| C2    | The worker: spawn, command channel, event channel, an external cancel flag polled between events, clean shutdown, and the record-replay flush on exit.                                           | the session's observable event sequence is UNCHANGED (differential 352/352) and a stalled provider still yields events   |
| C3    | The UI loop: crossterm poll with a short timeout, the 16 ms tick driving reveal/pulse/draw, the drain becoming a channel drain.                                                                  | a test proves the UI paints on its own tick with NO provider events                                                      |
| C4    | Evidence pack: a fake slow provider, frame counts during a stall, cancel latency, idle frames byte-identical.                                                                                    | counts recorded; the pinned frames unchanged when idle; full gate green                                                  |

## Invariants this arc must not break

- The input queue stays the SINGLE interactive-read owner, on the UI thread; the
  worker never reads the terminal.
- The worker gains NO authority: it composes the same session with the same
  policy and the same approval gate; a thread is not a capability.
- Core stays single-threaded; the differential corpus and its synchronous
  assumptions are untouched.
- No new persistence. The recordings store keeps its existing contract and is
  flushed exactly once, by whoever owns the session.
- Stdio keeps its current path unless it is deliberately moved (a separate
  decision).

## Recorded risk

The TUI's slash commands, the context pane, approvals and the replay flush all
touch the session today. Moving the session off-thread moves every one of them
behind a channel, which is the real size of this arc -- not the thread itself.
