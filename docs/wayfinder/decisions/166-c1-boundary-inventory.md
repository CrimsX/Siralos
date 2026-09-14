---
title: "Threaded Session C1: Boundary Inventory"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# Threaded Session C1: Boundary Inventory

Ticket [130](../tickets/130-threaded-session-liveness.md) · C1 of C1-C4
[165](165-threaded-session-entry-review.md) · [Map](../siralos-roadmap.md)

## 1. What C1 Was For

The entry review ([165](165-threaded-session-entry-review.md)) recorded that the
size of this arc is not the thread but the number of surfaces that touch the
session. C1 is the inventory that decides the boundary BEFORE any concurrency is
written. Evidence base: an exhaustive scan of \`interactive.rs\` for every session
reference (28 matches), each assigned an owner below.

## 2. The Inventory

| Touch point                                                    | Today                                           | Owner after the boundary                                                           |
| -------------------------------------------------------------- | ----------------------------------------------- | ---------------------------------------------------------------------------------- |
| \`send_prompt\` (TUI \`SlashCommand::Prompt\`)                 | UI thread, inside the dispatcher                | **worker** -- the UI sends a prompt command                                        |
| \`poll_event\` (the drain)                                     | UI thread, blocking on the provider read        | **worker** -- the UI drains an event channel; the blocking read moves off the UI   |
| \`cancel\` (from the progress callback)                        | UI thread, during the drain                     | **worker**, via an external cancel flag the UI sets (polled between events)        |
| \`history\` (context pane build)                               | UI thread, four call sites                      | **worker**, pushed to the UI with the events or on demand (design answer D1)       |
| \`last_projection\` (\`/context\`, \`/tools\`)                 | UI thread, two call sites                       | **worker**, returned as a command result (design answer D2)                        |
| \`enable_provider_progress_ticks\`                             | UI thread at setup                              | **worker** -- part of session construction, before the first prompt                |
| Live provider cells (model / endpoint / protocol / credential) | UI thread writes them (\`/model\`, \`/reload\`) | **worker** owns them; the UI sends \`set-model\`-style commands (design answer D3) |
| Replay flush (\`flush_record_replay\`)                         | UI thread at exit                               | **worker**, exactly once, on shutdown -- one owner, per decision 78                |
| Approvals                                                      | dormant, would surface through the drain        | **worker -> UI -> worker** round trip on the same channels (design answer D4)      |
| Terminal, input queue, \`TuiState\`, reveal/pulse tick, render | UI thread                                       | **UI thread** (unchanged; the input queue stays the single read owner)             |
| Command parsing / catalog, profile file writes                 | UI thread (shared definitions)                  | **UI thread** (unchanged)                                                          |

## 3. The Boundary, Chosen

The **worker owns the session, the provider and the live cells**; the **UI owns
the terminal, the input queue, the render state and the clock**. Commands flow
UI -> worker, events and command results flow worker -> UI. Nothing in core
changes, so the differential corpus and its synchronous turn assumptions stay
untouched (decision 165's invariant).

## 4. The Design Answers C2 Must Settle

| Id  | Question                                                                                                        | Constraint                                                                                                     |
| --- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| D1  | How the context pane gets its history: a snapshot per event batch, or a request/response when the pane is drawn | Must not add an unbounded queue; the pane is advisory and may lag by a frame                                   |
| D2  | How \`/context\` and \`/tools\` get the projection                                                              | They are display commands: a request/response pair, no session mutation                                        |
| D3  | How the live cells are written from the UI                                                                      | \`/model\` persists first (UI-owned file write) and then commands the worker -- persist-before-live stays true |
| D4  | The approval round trip                                                                                         | Dormant today; the channels must not pretend an approval path exists that no gate can honour                   |

## 5. Result

**C1 PASS.** Every session reference in the TUI path has an owner, and the four
open questions are named with their constraints rather than left implicit. C2
(the worker, its channels, the cancel flag, the single replay flush) may now be
written against this boundary; D1-D4 are answered inside it.
