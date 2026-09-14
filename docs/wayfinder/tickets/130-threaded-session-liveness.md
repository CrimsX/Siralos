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

## C2 Status (2026-09-12)

Delivered and green: the message contract with its compile-time `Send` proof
(`85b932c`), the worker loop proven against a scripted session (`6702dbc`), the
adapter for the composed session (`d1267dd`), the spawn path with its owned
paths and its single flush (`8c6fb79`), and the event bridge with the
sanitizer boundary intact (`f10b65a`).

**What remains is ONE step, and it cannot be sliced.** Step 3 rewires the TUI
loop to commands and events; step 4 sends `Shutdown` on every exit path and
joins. A partially switched loop would leave the UI holding a session AND the
worker holding one -- the divergence decision 167 exists to forbid -- so the
switch is atomic: either `dispatch_tui_command` no longer takes an
`application`, or nothing changes. The completion check is exactly that
parameter disappearing.

One piece of step 3 is already in place, as a behaviour-preserving refactor
(`24f9dd7`): `drain_events` reads a narrow `EventSource` seam (the two calls it
always made, `poll_event` and `cancel`) that the real session satisfies by pure
delegation, proven with a fake source in
`drain_events_reads_the_source_seam_and_cancels_on_request`. That takes the
drain body out of the atomic switch: when the source becomes worker-backed, the
drain does not change. It creates NO second session -- `compose_session` still
runs exactly once, in the frontend -- so what remains is what the paragraph
above describes.

The frontend half of that switch is in place too (`2457feb`): `WorkerSource`
wraps a `WorkerHandle` as the drain's source. Session events feed the shared
drain; everything else comes back from `take_pending` for `apply_worker_event`,
and `cancel` sets the flag AND sends the command. Four tests cover it, one of
which drives the real `drain_events` from a scripted worker channel, so "the
drain can read a worker" is measured rather than asserted. Still no second
session: nothing calls `spawn_worker` from the loop, so `compose_session` keeps
running exactly once, in the frontend.

Order and hazards for whoever takes it: `%TEMP%\siralos-c2-wiring.md`.

## C2 touch-point inventory (measured)

Every place the TUI loop still reaches the session, and the transport that
replaces it (`crates/siralos-cli/src/interactive.rs`, line numbers as of
`2457feb`):

| Site                   | Reach                                                            | Transport                                                                                                                                                                                                                                                   |
| ---------------------- | ---------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 994, 1031              | `last_projection()` behind the `/context` and `/tools` renderers | `WorkerCommand::{ContextReport,ToolsReport}` -> `WorkerEvent::Report`                                                                                                                                                                                       |
| 1549                   | `send_prompt`                                                    | `WorkerCommand::Prompt`                                                                                                                                                                                                                                     |
| 3516, 3819, 4066, 4286 | `history()` (context demand and the pane)                        | `WorkerEvent::Pane` (decision 167 D1); the demand loop moves to whoever owns the history                                                                                                                                                                    |
| 3675                   | `enable_provider_progress_ticks()`                               | moves into the worker: after the switch IT composes the session the TUI drives                                                                                                                                                                              |
| 3847-3849              | the approval modal                                               | not a cross-thread concern: provider-removal confirmation resolves entirely frontend-side (`handle_pending_approval_key` + `apply_provider_remove_confirmation`), and the tool loop's approval reader (`read_approval_via_input_queue`, 1164) is stdio-only |

`/models` needs no transport either: the fetch never touches the session.

The inventory exposed one lie-in-waiting, fixed in `4dc2d67`: the loop
answered `Reload` with a hardcoded `"reloaded"` while the session's `reload`
refuses, so a switched `/reload` would have claimed success for work that
never happened. `WorkerSession::reload` now returns the report and the loop
relays exactly it.

The one real gap left before the switch is the reload path itself (re-read,
recompose, apply), which still lives in the frontend and must move behind the
boundary, or `/reload` regresses.

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
