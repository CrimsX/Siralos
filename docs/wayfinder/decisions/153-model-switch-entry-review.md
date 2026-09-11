---
title: "The Session Model Switch (/model) Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-11"
ticket: "124"
supersedes: []
---

# The Session Model Switch (/model) Entry Review

Ticket [124](../tickets/124-model-switch.md) · entry review [the
Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `4e9f4db` "feat(cli): switch the session model with
> /model"). There was no prior ticket and no prior entry review for
> this change; the approval that exists is an in-chat design approval
> from the human owner on 2026-09-10/11 — nothing more is claimed here,
> and nothing is backdated. The normal order (entry review authorizes →
> implementation lands) is inverted here: the implementation landed
> first and this review records what it should have authorized. The
> verdict below is therefore a retroactive PASS over the
> already-committed diff, not a pre-commit authorization.

> **Owner-approved 2026-09-10/11 (retroactive, in-chat design
> approval).** `/model` was display-only; changing the model meant
> editing `siralos.toml` and restarting. Now `/model <id>` switches the
> model the NEXT request uses and persists it, and bare `/model` opens
> the fetched-models picker in the TUI. The model is a shared live cell
> (`Rc<RefCell<String>>`) in the generic, openai and anthropic
> providers: `set_model` mutates it in place and each `stream()` reads
> it AT CALL TIME, so the switched id reaches the request body; Fake is
> a model-less echo and ignores the switch; replay switches the label
> but still serves its fixed recording (inherent to playback).
> `SessionProvider::set_live_model` is called only after the persist
> succeeds, so a refused write cannot leave the session on a model the
> file does not have. Persistence reuses `write_profile_config` (no
> duplicated writer logic) with the applied record's
> provider/credential/endpoint/protocol; the id is validated with the
> core rule first and the display name is cleared because it described
> the previous model. `/model <id>` is a new catalog entry (13) so it
> stays distinct from `/models`, whose argument form still fails the
> unknown-command honesty gate.

## 2. The Fixes

| Fix                          | The change                                                                                                                                                                                                                                                                                                                                                                                               | The evidence                  |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| F1 SHARED LIVE CELL          | The model is a shared live cell (`Rc<RefCell<String>>`) in the generic, openai and anthropic providers: `set_model` mutates it in place and each `stream()` reads it AT CALL TIME, so the switched id reaches the request body.                                                                                                                                                                          | the committed provider diff   |
| F2 FAKE AND REPLAY, HONESTLY | Fake is a model-less echo and ignores the switch; replay switches the label but still serves its fixed recording (inherent to playback).                                                                                                                                                                                                                                                                 | the committed diff            |
| F3 PERSIST-BEFORE-LIVE       | `SessionProvider::set_live_model` is called only after the persist succeeds, so a refused write cannot leave the session on a model the file does not have.                                                                                                                                                                                                                                              | the committed session diff    |
| F4 REUSED WRITER, CORE RULE  | Persistence reuses `write_profile_config` (no duplicated writer logic) with the applied record's provider/credential/endpoint/protocol; the id is validated with the core rule first and the display name is cleared (it described the previous model).                                                                                                                                                  | the committed writer diff     |
| F5 DISTINCT CATALOG ENTRY    | `/model <id>` is a new catalog entry (13), distinct from `/models`, whose argument form still fails the unknown-command honesty gate.                                                                                                                                                                                                                                                                    | the committed catalog diff    |
| F6 MESSAGES                  | Success `"model switched to <id> - model display name cleared"`; invalid id `"A model must match [a-zA-Z0-9._/:@-]{1,256} with no NUL."`; no profile `"no provider configured - cannot switch model without an applied [profile]"`; write failure `"model switch failed: <writer reason>"`; bare stdio keeps the existing `model: <id>` line plus `"pass /model <id> to switch, or use the TUI picker"`. | the committed message strings |

## 3. Criteria → Evidence

| Criterion                                                | Evidence                                                                                                                                                                                                                                                                                       | Verdict |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| `/model <id>` reaches the next request body and persists | live-cell tests: `set_model` then `stream()` carries the switched id; `write_profile_config` reuse persists the applied record's fields                                                                                                                                                        | pass    |
| A refused write cannot desync the session                | `set_live_model` ordered after persist success in the committed diff                                                                                                                                                                                                                           | pass    |
| Fake and replay behave as stated                         | Fake ignores the switch (model-less echo); replay relabels but serves its fixed recording                                                                                                                                                                                                      | pass    |
| Ids validated by the core rule; display name cleared     | invalid-id message carries the core `[a-zA-Z0-9._/:@-]{1,256}` rule; success message states the display-name clearing                                                                                                                                                                          | pass    |
| No regression                                            | `npm run check` exit 0 after the change; `npm run check:differential` exit 0, parity 352/352 applicable required, 4 platform skips, 0 deviations; suites 614 core / 316 adapters / 25 conformance / 243 cli with 0 failures (evidence gathered by the orchestrator and cited here, not re-run) | pass    |

## 4. Result

Entry review PASS (retroactive): the `/model` session switch as
committed in `4e9f4db` is the correct fix — a shared live cell read at
call time, persist-before-live ordering, the reused writer with the
core rule, and honest Fake/replay semantics. This review authorized
nothing (the code had already landed); it records what authorization
would have covered.
