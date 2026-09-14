---
title: "Threaded Session C2: The Session Cannot Cross Threads"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-12"
ticket: "130"
supersedes: []
---

# Threaded Session C2: The Session Cannot Cross Threads

Ticket [130](../tickets/130-threaded-session-liveness.md) · C2 design finding
[C1 boundary inventory](166-c1-boundary-inventory.md) ·
[165](165-threaded-session-entry-review.md) · [Map](../siralos-roadmap.md)

## 1. The Finding

C1 proposed that "the worker may build the session itself". C2 checked whether
the alternative -- composing on the UI thread and MOVING the session to the
worker -- is available. It is not, and the reason is mechanical:

- every provider carries its live cells as \`Rc<RefCell<..>>\` (seven sites:
  \`generic.rs\` model/endpoint/credential/protocol, \`openai.rs\` and
  \`anthropic.rs\` model, \`replay.rs\` model), and \`SessionProvider\` in the CLI
  wraps a \`HostProvider\` that holds them;
- \`Rc\` is \`!Send\`, so \`SiralosApplication<'a, P>\` is \`!Send\`, and no amount of
  leaking lifetimes changes that.

**Consequence, and it is now part of the design: the worker CONSTRUCTS the
session.** Composition -- provider, registry, policy, tool definitions, hosts,
manifests, plugin/context selection, projection config, recorder -- happens
inside the worker thread. The UI thread holds no session at all.

## 2. What That Does To D1-D4

| Id  | Answer under this finding                                                                                                                                                                                                                                                                                                     |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | The worker owns history outright. The pane is fed by a SNAPSHOT pushed with the event stream (advisory, may lag a frame, never blocks the turn) -- there is no second copy to keep in sync.                                                                                                                                   |
| D2  | \`/context\` and \`/tools\` become request/response commands: the UI asks, the worker answers with detached display data. Display-only, no mutation.                                                                                                                                                                          |
| D3  | The live cells live inside the worker's provider, so the UI cannot write them. \`/model\` keeps PERSIST-BEFORE-LIVE by ordering: the UI writes the profile file (a UI-owned operation), then commands the worker; the worker applies and reports, and a failed apply is surfaced as an event rather than a silent divergence. |
| D4  | Approvals stay dormant; the channels define no path a gate cannot honour.                                                                                                                                                                                                                                                     |

## 3. New Constraints Recorded

- **No second session.** The UI must not keep a session "just for display" --
  that is the divergence C1 was written to prevent. Every session question is a
  command.
- **Stdio is unchanged** (decision 165): it keeps the in-thread path. The two
  frontends therefore differ in threading, and that difference is documented
  rather than hidden.
- **The recorder is created inside the worker**, so the worker owns the single
  flush (decision 78's one-owner rule).
- **The worker's errors are data**: a failed command, a refused profile, a
  composition error all travel back as events the UI can render truthfully.

## 4. Result

**C2 design settled before code.** The boundary is now forced by construction
rather than by convention: the session is created on the thread that uses it, and
the UI's only view of it is commands and events. This removes the \`Send\`
question entirely -- no \`Arc\`/\`Mutex\` migration of the adapters is needed, and the
differential corpus stays untouched because core never learns about threads.
