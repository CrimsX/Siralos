---
title: "Threaded Session C2 Step 3: What the Frontend Keeps and What the Worker Owns"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# Threaded Session C2 Step 3: What the Frontend Keeps and What the Worker Owns

Ticket [130](../tickets/130-threaded-session-liveness.md) · C2 step 3 placement
[167](167-c2-session-cannot-cross-threads.md) ·
[166](166-c1-boundary-inventory.md) · [Map](../siralos-roadmap.md)

## 1. Why this record exists

Every transport step 3 needs is now implemented and tested, so the remaining
edit looked mechanical. Measuring it first showed it is not: the TUI body still
derives roughly ninety values from the composition it destructures at
`interactive.rs:3662`, and the shared dispatcher takes six of those pieces of
capability state as parameters. This record fixes where each one goes BEFORE the
edit, so the switch cannot invent a second owner for something.

## 2. The measurement (as of `13267b9`, TUI body 3632-4360)

| Value                                                                                    | Uses   | Where it is used                                              |
| ---------------------------------------------------------------------------------------- | ------ | ------------------------------------------------------------- |
| `applied_provider`                                                                       | 15     | picker, status recompute, reports, removal                    |
| `context_session_holder`                                                                 | 12     | pane builds, context toggles, metrics                         |
| `applied_model` / `applied_endpoint`                                                     | 7 each | picker, `/models` fetch, status                               |
| `applied_model_display_name`                                                             | 6      | header preference                                             |
| `live_provider` / `context_system_enabled`                                               | 5 each | `/models` fetch, context arm                                  |
| `applied_credential` / `applied_credential_raw` / `applied_protocol_str`                 | 4 each | `/models` fetch (4153-4221), picker                           |
| `tool_definitions`, `policy`, `hosts`, `manifests`, `profile_plugins`, `context_control` | 2 each | declaration + the ONE `dispatch_tui_command` call (4274-4282) |
| `record_recorder`, `replay_store_path`                                                   | 2 each | declaration + the exit flush (4355)                           |

The session's own touch points (prompt, drain, reports, history) were inventoried
and closed in the ticket: reports, prompt, pane, ticks, the demand loop, the
reload path and the header all have transports.

## 3. The placement rules

**R1 -- the worker owns the session.** The application, its live provider, the
context session, the recorder and the credential stay inside the worker. The
frontend holds the workspace root (for profile writes), `TuiState`, the
terminal and its sink, the command catalog, and the last `Ready`/`Pane`
snapshots. Nothing else.

**R2 -- the credential never crosses the boundary.** The `/models` fetch reads
the endpoint and the resolved credential, so it becomes
`WorkerCommand::ModelsFetch` -> `WorkerEvent::Models(Vec<String>)`. The
alternative -- sending the credential in a snapshot so the frontend can keep
fetching -- would put a secret into `TuiState` and the render path, which the
decision 143-146 hygiene scope exists to prevent. This is the ONE new contract
piece step 3 needs.

**R3 -- display may cross, authority may not.** The picker's current values
(provider, model, display name, endpoint, protocol and the already-redacted
credential form) ride on the status snapshot the loop already sends
(`Ready`); the picker's WRITES stay frontend-side because it writes
`siralos.toml`, and the worker applies them with `SetModel`/`Reload` --
persist-before-live by ordering, per 167 D3.

**R4 -- capability state is not display.** `tool_definitions`, `policy`,
`hosts`, `manifests`, `profile_plugins` and `context_control` exist in the
frontend only because `dispatch_tui_command` takes them. The switch removes
those parameters (the ticket's completion check). An arm that only RENDERS keeps
a report; an arm that reads capability state to DECIDE (domain activation,
context controls, policy-bearing projections) moves to the worker, because
capability is authority and the frontend may not hold authority it cannot
enforce.

**R5 -- the pane is pushed.** D1's snapshot per event is already implemented; the
twelve `context_session_holder` uses collapse into the cached pane
(`pane_cache`, 3660) plus the status snapshot.

**R6 -- step 4 shuts down last.** `Shutdown` on every exit path, joined BEFORE
the terminal guard restores, so the single flush (decision 78's owner rule) is
known to have happened rather than hoped for.

### 3a. The classification, measured

Only four of the dispatcher's fifteen arms touch capability state at all
(`dispatch_tui_command`, arms as of `47cab9b`):

| Arm                                                                   | Values                                  | Verdict                                                                                                                 |
| --------------------------------------------------------------------- | --------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `Context` (1375)                                                      | `context_control`                       | report -- `ContextReport` already exists, and the report belongs where the control is applied                           |
| `Tools` (1384)                                                        | `tool_definitions`, `policy`            | report -- `ToolsReport` already exists                                                                                  |
| `DomainsAdd` (1395), `DomainsEnable` (1404), `DomainsActivate` (1413) | `hosts`, `manifests`, `profile_plugins` | open -- each is a `render_*(workspace_root, ..)` written to the sink, so any persistence already lives in those helpers |

Every other arm (`Exit`, `Provider`, `ProviderRemove`, `Model`, `Models`,
`Evolve`, `Reload`, `Mouse`, `Prompt`) touches none of the six, and four of them
already have commands (`Prompt`, `SetModel`, `ModelsFetch`, `Reload`).

The open question the switch must settle before it edits, stated rather than
guessed: do the three domain renderers persist (install/enable writing
`siralos.toml`) or only display? If they persist, they stay frontend-side like
the picker and need the installed registry as DISPLAY data on the snapshot
(ids and manifest names, no secrets -- R3); if they only display, they are
reports like `Context` and `Tools`. Either way the six parameters leave the
signature, which is the completion check.

## 4. Consequences

- The switch stays ONE commit. A staged switch -- spawning the worker and
  migrating call sites over several commits -- would leave two compositions
  alive, which is the divergence 167 forbids, so the session handle has to
  disappear in a single edit.
- The credential placement removes a hygiene risk a naive switch would have
  introduced; it is cheaper to add the fetch command first than to audit a
  leaked secret afterwards.
- `ModelsFetch` is additive and testable on its own, like every other
  transport in this arc (the drain seam, the source, the reload, the settled
  turn, the header), so it lands before the switch rather than inside it.

## 5. What remains

1. `ModelsFetch`/`Models` plus the picker's display fields on the status
   snapshot (additive, safe).
2. The atomic switch: spawn the worker, cache `Ready`/`Pane`, replace the
   command arms, drop `dispatch_tui_command`'s composition parameters.
3. Step 4: `Shutdown` and join on every exit path.
