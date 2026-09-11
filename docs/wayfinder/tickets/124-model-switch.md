---
title: "The Session Model Switch (/model)"
label: "wayfinder:ticket"
status: closed
date: "2026-09-11"
supersedes: []
---

# The Session Model Switch (/model)

`/model` was display-only; changing the model meant editing
`siralos.toml` and restarting. Now `/model <id>` switches the model
the NEXT request uses and persists it, and bare `/model` opens the
fetched-models picker in the TUI. The model is a shared live cell
(`Rc<RefCell<String>>`) in the generic, openai and anthropic
providers: `set_model` mutates it in place and each `stream()` reads it
AT CALL TIME, so the switched id reaches the request body; Fake is a
model-less echo and ignores the switch; replay switches the label but
still serves its fixed recording (inherent to playback).
`SessionProvider::set_live_model` is called only after the persist
succeeds, so a refused write cannot leave the session on a model the
file does not have. Persistence reuses `write_profile_config` (no
duplicated writer logic) with the applied record's
provider/credential/endpoint/protocol; the id is validated with the
core rule first and the display name is cleared because it described
the previous model.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `4e9f4db` "feat(cli): switch the session model with
> /model") BEFORE this ticket and its entry review were written. There
> was no prior ticket and no prior entry review for this change; the
> approval that exists is an in-chat design approval from the human
> owner on 2026-09-10/11 — nothing more is claimed here, and nothing is
> backdated. The entry-review inversion is named in [the retroactive
> entry
> review](../decisions/153-model-switch-entry-review.md): the record
> follows the implementation rather than authorizing it.

Authorized by
[the Session Model Switch Entry Review (retroactive)](../decisions/153-model-switch-entry-review.md).
Implemented and recorded in
[decision 154](../decisions/154-model-switch.md).
