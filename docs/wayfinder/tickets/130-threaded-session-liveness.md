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

That gap is closed as of `e84c4fc`: `SessionComposition::reload` re-reads the
profile, recomposes, applies the same live cells the frontends apply, and
returns the report.
`the_worker_adapter_applies_a_reload_and_returns_the_report` composes a real
session, edits the model on disk, reloads through the boundary, and asserts the
report AND that the NEXT request reads the new model -- so `/reload` does not
regress when the loop switches.

One more thing the inventory settled: the TUI's in-loop `/reload` (4181) never
refreshed the header, it only printed the report, so the switch needs no
applied-config event for it.

The other worker-side item is in as well (`6a29ffd`): the demand loop reads the
session's own history, so `WorkerSession` gained `turn_settled`, the loop calls
it once after the turn's events and before `TurnFinished`, and the adapter
implements it as the demand tick the frontends call today. Two tests pin it --
the once-per-turn call, and that the adapter's hook is a real call site (a
source check, the idiom `compose_session_before_guard_no_terminal_needed` uses).

The header transport is in as well (`13267b9`): `WorkerEvent::Ready(SessionStatus)`
arrives before any command and again after `SetModel`/`Reload`, because the
status segment, the provider and the model are all derived from the composition
(including its context metrics) and the frontend cannot build them once the
session lives in the worker. `set_model` drops a display name that belonged to
the previous model so the header cannot lie, and `apply_worker_event` ignores
the header because it is frontend state, not transcript.

That closes every transport the switch needs. What remains is the rewiring
itself and step 4 -- `Shutdown` on every exit path, join before the terminal is
restored.

R2's piece is in (`856c6cf`): `WorkerCommand::ModelsFetch` ->
`WorkerEvent::Models(Vec<String>)`, with the adapter calling the same
`generic::fetch_models` the frontends call using its OWN endpoint and
credential, so no secret has to cross for the picker to keep working.

R3's piece is in (`47cab9b`): the status snapshot also carries the endpoint, the
protocol and the credential's ALREADY-REDACTED display form, so the picker can
show what it shows today without the raw value crossing --
`the_status_snapshot_never_carries_the_credential` composes a session with
`key:super-secret-value` and asserts the snapshot holds `key:***` and not the
secret.

That is every additive piece decision 168 named. What is left is the atomic
switch in one commit and step 4.

R4 is now measured rather than a principle (decision 168 section 3a): of the
dispatcher's fifteen arms only four touch capability state, and two of those
(`Context`, `Tools`) already have report commands. The three domain arms render
through `render_*(workspace_root, ..)` helpers, so whether they persist or only
display is the one question the switch must answer before it edits.

The rewiring itself now has a placement decision written BEFORE the edit
([168](../decisions/168-c2-step3-wiring-placement.md)): measuring it first showed
the job is not the short call-site list above but roughly ninety
composition-derived values plus six capability parameters on the shared
dispatcher. The decision fixes where each one goes -- the credential stays in
the worker (so `/models` becomes `ModelsFetch`/`Models` and no secret enters UI
state), display values ride the status snapshot, an arm that only RENDERS keeps
a report while an arm that reads capability state to DECIDE moves to the worker,
and the switch stays one commit because a staged one would leave two
compositions alive (the divergence 167 forbids).

## Switch readiness (as of `18bce0c`)

Every prerequisite decision 168 lists is implemented and tested. The
transports: the drain's `EventSource` seam, `WorkerSource` over the channel,
`reload`, `turn_settled`, `Ready(SessionStatus)` (the header plus the picker's
display values, with the credential redacted where it lives),
`ModelsFetch`/`Models`, the three domain commands (`DomainsAdd`,
`DomainsEnable`, `DomainsActivate` -- they mutate the registry, so they cannot
stay in the frontend), and `enable_progress_ticks`, which `spawn_worker` turns
on for the session it composes.

What is left is ONE edit and its follow-up: spawn the worker in the TUI entry,
cache `Ready`/`Pane`, replace the command arms, drop
`dispatch_tui_command`'s six capability parameters (the completion check), then
send `Shutdown` and join on every exit path before the terminal guard restores.
Decision 168 section 3a records the arm classification that edit needs, and
section 2 the measured inventory behind it.

## C2 step 3, C2 step 4 and C3 landed (2026-09-12)

The switch is in, as two commits, plus the tick (and one review fix on top of
it):

- `cb66824` -- C2 step 3, the atomic switch. `run_interactive_tui_with_options`
  resolves the workspace root itself (R1: it owns the profile writes), spawns
  the worker BEFORE the terminal guard, waits for its first events THERE (so
  startup diagnostics and a composition failure stay on the normal screen), and
  then holds no session at all: `TuiState` caches what `Ready`/`Pane` push and
  `pump_worker` relays the channel.
- `dfa450f` -- C2 step 4. `WorkerGuard` owns the source and shuts it down on
  drop, declared after `TerminalGuard` so it drops (and joins) FIRST; the one
  path before that guard (a failed `TerminalGuard::enter`) stops the worker
  explicitly, and `await_worker_ready` takes the source by value so its failure
  paths join too.
- `1d0df16` -- C3. The relay's idle wait IS the 16 ms tick that runs the reveal,
  the pulse, the thinking expansion and the interrupt key while the worker is
  silent, and every frame now drains the channel first (`Until::Drain`, a
  zero-timeout sweep) so the loop is a genuine channel drain, not only while a
  command is outstanding.
- `00d93eb` -- the review fix on C3. Reading the drain back found two defects
  its tests did not cover: a closed channel was announced on EVERY frame (now
  only a relay that was WAITING reports it, once), and an answer seen by a drain
  was collected and dropped (a turn and a drain now render through the shared
  bridge, so an answer is never swallowed). Both are pinned by a test that fails
  when the guard is reverted.

## Owner follow-up: the thinking's reveal lagged (2026-09-12)

Owner, after the three commits landed: "the thinking still seems delayed and not
smooth". The frame cadence was no longer the cause -- C3's test proves the UI
paints with no provider event at all -- the REVEAL was: it paced text at
`REVEAL_CHARS_PER_SEC` (240) and capped a frame at `REVEAL_TICK_CHARS` (480),
so a stream arriving faster than the pace fell behind without bound. Measured on
the production `reveal_now`: after two seconds of a 1920 chars/s reasoning
stream the display had shown 936 of 3840 characters -- **2904 characters, about
twelve seconds, behind** -- and it was still crawling past the reader after the
model had moved on. A longer backlog than `REASONING_BYTES` (8192) was also
trimmed away before it was ever shown.

Decision [169](../decisions/169-reveal-tracks-a-fast-stream.md) records the rule
that replaces it: a channel is PACED below `REVEAL_MAX_LAG_CHARS` (160) and
TRACKED above it (up to one tick's worth per frame), per channel, so a streaming
answer can no longer starve the thinking. After the change the same three
measured streams settle at the same 157-character lag whatever their speed, and
the answer-and-thinking pacing for everything at a human speed is unchanged.

The regression tests measure both halves: a 2400 chars/s stream must stay inside
the bound (the test fails when the bound is made unreachable) and a
reading-speed stream must still be released at the pace.

## Owner follow-up 2: one character at a time (2026-09-12)

Owner, after decision 169's bounded lag: "i would like it to display/render one
character at a time". That is a rule about the STEP, so the reveal was rebuilt
around it (decision [170](../decisions/170-one-character-at-a-time.md)): one
character per painted frame, no rate budget, no debt, no catch-up, and the frame
cadence (`REVEAL_CHAR_INTERVAL`, 6 ms ≈ 166 characters a second) is the
character rate.

That only works if a frame is cheap, and a frame was not: it cloned and wrapped
the WHOLE transcript every time, so the cost grew with the session --
**5.3 ms at 24 lines and 20.6 ms at 1200 in the unoptimized build `cargo run`
produces (~85 ms extrapolated at 5000)** -- which is also why the text could feel
rough in a long session. The render now builds only the rows the viewport shows:
**648 µs at 5000 lines, and the frame is 2.20 ms at 24 lines versus 2.25 ms at
5000**.

Honest trade recorded with the decision: the character rate is capped by the
frame rate, so a stream faster than ~166 chars/s is shown slower than it arrives.
Nothing is dropped to hide that -- `push_reasoning` applies its bound to already
revealed text only, so a backlog the reader is still owed is never trimmed away.

## Owner follow-up 3: match the model's speed (2026-09-12)

Owner: "are you able to match the speed the model produces it?" -- and then "this
is model agnostic right? it will match depending on the model's speed?".

Yes, by construction (decision [171](../decisions/171-reveal-tracks-the-model.md)):
the reveal reads two text buffers fed by `TextDelta`/`ReasoningDelta` through the
one provider-neutral bridge, so it knows nothing about a model, a provider, a
token rate or a protocol. The fixed 6 ms cadence of follow-up 2 is gone --
`paint_interval` returns ZERO while anything is owed and the ordinary cadence
otherwise -- so the display rate IS the arrival rate.

Measured ceiling, production draw path, unoptimized build (`cargo run`): **2.0 ms
per frame, 498 characters a second**, one character per frame
(`a_backlog_drains_at_the_frame_rate`: 250 characters in exactly 250 frames).
Above it a burst displays slower than it arrived and nothing is dropped; that is
the machine and the build, never the model.

**The completion check held**: `dispatch_tui_command` takes
`(command, workspace_root, state, sink, worker, pane, progress, reasoning)` --
no `application`, and none of the six capability parameters. A source check in
`compose_session_before_guard_no_terminal_needed` also asserts the TUI body
never calls `compose_session`, so "one session" is mechanical rather than
remembered.

### What the edit added to the design

- `SessionStatus` gained `credential_resolved` (the `/models` gate decides on
  it today, and the frontend cannot recompute it without the secret) and
  `context_suffix` (so a TRANSIENT status -- the add-flow's "fetching
  models..." -- keeps the `ctx N/4096` readout the frontend no longer has the
  metrics for). Decision 168 R3 extended, not bent: both are display facts.
- The startup pane is pushed BEFORE the header. Sending it after would have made
  the first frame a race between the header and the pane; before, a frontend that
  waits for the header already holds the pane it will draw with it.
- `SetModel` no longer emits a report of its own: the frontend owns the profile
  write and already prints its outcome (167 D3), so a second line would say the
  same thing twice. The header is still re-announced.
- A turn's settled pane is pushed after `turn_settled`, so the snapshot the
  frontend draws reflects the demand tick -- the bottom-of-loop rebuild it
  replaced ran after that tick too.

### Evidence

- `npm run check` exit 0 on `dfa450f` (format, lint, docs, context, identity,
  public hygiene, secrets, architecture, differential, fmt, clippy with warnings
  denied, workspace tests); differential **352/352 applicable required**, 4
  platform skips, 0 informational deviations.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` exit 0;
  `cargo fmt --all --check` exit 0; `cargo test --workspace` green (302 CLI lib
  tests).
- The loop is driven, not just compiled: seven tests dispatch through the real
  `dispatch_tui_command` over a scripted `WorkerSource` and assert the
  `WorkerCommand` each arm sends and the events it applies (header, pane,
  report, failure, models, a whole turn, the persist-then-`SetModel` ordering).
- C3's acceptance: `the_ui_paints_on_its_own_tick_with_no_provider_events`
  drives the PRODUCTION draw path over a `TestBackend` with no provider event
  for 200 ms and asserts the released answer grows across frames and that the
  painted frame carries it. Removing the idle tick makes it fail
  (mutation-checked).
- The recordings flush is observed, not asserted: the guard test drives a real
  worker on a record-replay workspace and finds `.siralos/replay-store.json` on
  disk by the time the guard returns.

### Behaviour notes (deliberate, small)

- `/reload` now refreshes the header, because the worker re-announces it; the
  old in-loop arm printed the report and left a stale header behind.
- A prompt the session REFUSES (already responding) is reported as
  `Worker failed: ...` and the loop continues; the old path returned the error
  out of the whole TUI and ended the session. The refusal is unreachable from the
  key path (submission happens between turns), so this is an error-path
  improvement rather than a change anyone will see.
- `/models` keeps its old gate: a provider, an endpoint AND a credential that
  actually RESOLVED. That is why the snapshot carries the resolution as a fact.

## Slices

| Slice | Status                      | Content                                                                                                                                                                                          | Acceptance                                                                                                               |
| ----- | --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| C1    | done (decision 166)         | Boundary inventory and choice: where the thread boundary goes, what moves, what stays. The session already composes its own provider, so the worker may build it and core stays single-threaded. | a written inventory of every touch point (turn, commands, approvals, context pane, replay flush) with the owner for each |
| C2    | done (`cb66824`, `dfa450f`) | The worker: spawn, command channel, event channel, an external cancel flag polled between events, clean shutdown, and the record-replay flush on exit.                                           | the session's observable event sequence is UNCHANGED (differential 352/352) and a stalled provider still yields events   |
| C3    | done (`1d0df16`)            | The UI loop: crossterm poll with a short timeout, the 16 ms tick driving reveal/pulse/draw, the drain becoming a channel drain.                                                                  | a test proves the UI paints on its own tick with NO provider events                                                      |
| C4    | NOT STARTED                 | Evidence pack: a fake slow provider, frame counts during a stall, cancel latency, idle frames byte-identical.                                                                                    | counts recorded; the pinned frames unchanged when idle; full gate green                                                  |

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
