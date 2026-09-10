---
title: "The Provider Model-Id Charset and Bound"
label: "wayfinder:ticket"
status: "open"
date: "2026-09-10"
supersedes: []
---

# The Provider Model-Id Charset and Bound

The `[profile]` model-id rule allowed only `[a-zA-Z0-9._-]`, so real
provider-issued ids — `example/model-a` (vendor
separator) and `example/model-b:free` (tag suffix) —
could neither be saved by the provider add-flow (`write_profile_config`)
nor applied at load. The charset is now ASCII alphanumeric plus
`. _ - / : @`, defined once as
`siralos_core::composition::is_model_id_char` and used by the core
validator, the write boundary, and the TUI form; `MAX_PROFILE_MODEL_BYTES`
went 128 -> 256 to match what the add-form already told the user it
enforced; every rejection message now states the rule it enforces.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `5520a41`) BEFORE this ticket and its entry review
> were written, at the human owner's explicit request. There was no prior
> ticket, no prior entry review, and no prior human approval for this
> change; nothing here is backdated. The entry-review inversion is named
> in [the retroactive entry review](../decisions/139-model-id-charset-entry-review.md):
> the record follows the implementation rather than authorizing it.

Authorized by
[the Provider Model-Id Charset and Bound Entry Review (retroactive)](../decisions/139-model-id-charset-entry-review.md).
Implemented and recorded in
[decision 140](../decisions/140-model-id-charset.md).

Open item: `npm run check` currently fails at `check:secrets` for an
unrelated, pre-existing reason — the untracked, gitignored workspace file
`siralos.toml` stores a literal key. That file and that script were not
touched by this change.

(End of file - total 41 lines)
