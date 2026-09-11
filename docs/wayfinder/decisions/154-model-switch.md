---
title: "The Session Model Switch (/model)"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "124"
supersedes: []
---

# The Session Model Switch (/model)

Ticket [124](../tickets/124-model-switch.md) · entry review
[153](153-model-switch-entry-review.md) · [Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `4e9f4db` "feat(cli): switch the session model with
> /model") BEFORE this record was written. There was no prior ticket
> and no prior entry review for this change; the approval that exists
> is an in-chat design approval from the human owner on 2026-09-10/11 —
> nothing more is claimed here, and nothing is backdated. The entry
> review ([153](153-model-switch-entry-review.md)) is itself
> retroactive and names the inversion.

## 2. The Implemented

| Fix                    | The change                                                                                                                                                                                                                                                                                                                                                                                               | The evidence                   |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| M1 SHARED LIVE CELL    | The model is a shared live cell (`Rc<RefCell<String>>`) in the generic, openai and anthropic providers: `set_model` mutates it in place and each `stream()` reads it AT CALL TIME, so the switched id reaches the request body.                                                                                                                                                                          | the committed provider diff    |
| M2 FAKE AND REPLAY     | Fake is a model-less echo and ignores the switch; replay switches the label but still serves its fixed recording (inherent to playback).                                                                                                                                                                                                                                                                 | the committed diff             |
| M3 PERSIST-BEFORE-LIVE | `SessionProvider::set_live_model` is called only after the persist succeeds, so a refused write cannot leave the session on a model the file does not have.                                                                                                                                                                                                                                              | the committed session diff     |
| M4 REUSED WRITER       | Persistence reuses `write_profile_config` (no duplicated writer logic) with the applied record's provider/credential/endpoint/protocol; the id is validated with the core rule first and the display name is cleared because it described the previous model.                                                                                                                                            | the committed writer diff      |
| M5 CATALOG ENTRY (13)  | `/model <id>` is a new catalog entry (13) so it stays distinct from `/models`, whose argument form still fails the unknown-command honesty gate; bare `/model` opens the fetched-models picker in the TUI.                                                                                                                                                                                               | the committed catalog/TUI diff |
| M6 MESSAGES            | Success `"model switched to <id> - model display name cleared"`; invalid id `"A model must match [a-zA-Z0-9._/:@-]{1,256} with no NUL."`; no profile `"no provider configured - cannot switch model without an applied [profile]"`; write failure `"model switch failed: <writer reason>"`; bare stdio keeps the existing `model: <id>` line plus `"pass /model <id> to switch, or use the TUI picker"`. | the committed message strings  |

## 3. Criteria → Evidence

| Criterion                                                | Evidence                                                                                                                                                                                                                                                                                                 | Verdict  |
| -------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| The switched model reaches the next request and the file | live-cell and persist-reuse tests pass; `set_live_model` only after persist success                                                                                                                                                                                                                      | pass     |
| Fake and replay semantics honest                         | Fake ignores; replay relabels but serves its fixed recording                                                                                                                                                                                                                                             | pass     |
| No regression                                            | `npm run check` exit 0 after the change; `npm run check:differential` exit 0, parity held 352/352 applicable required, 4 platform skips, 0 deviations; suites 614 core / 316 adapters / 25 conformance / 243 cli with 0 failures (evidence gathered by the orchestrator and cited here, not re-invented) | pass     |
| Docs/context gates                                       | `npm run check:docs` and `npm run check:context` outcomes reported below                                                                                                                                                                                                                                 | reported |

## 4. Result

The `/model` session switch is complete as committed in `4e9f4db`:
`/model <id>` switches the model the NEXT request uses and persists
it, bare `/model` opens the TUI picker, and a refused write never
desyncs the session from the file. Retroactive record closed; ticket
124 done.
