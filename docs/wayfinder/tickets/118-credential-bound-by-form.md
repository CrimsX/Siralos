---
title: "The Credential Bound Split by Form"
label: "wayfinder:ticket"
status: "open"
date: "2026-09-10"
supersedes: []
---

# The Credential Bound Split by Form

The `[profile]` credential bound was sized for the env form only:
`MAX_PROFILE_CREDENTIAL_BYTES = 70` is exactly `"env:"` plus a 64-char
name. When decisions 137/138 added verbatim key storage, the
whole-value check kept running before the form branch, so any `key:`
value over 66 characters was refused — a real OpenRouter key is 73
chars (77 bytes with the prefix). The same validator runs on the load
path (`ProfileRecord::validate` via `resolve_profile_overlay`), so such
a profile also failed to apply. The bound is now split by form: `env:`
and bare legacy forms keep the 70-byte whole-value bound; `key:`
values get `MAX_PROFILE_CREDENTIAL_KEY_BYTES = 4096` measured after
the prefix; every message names its bound.

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `530568c`) BEFORE this ticket and its entry review
> were written. There was no prior ticket, no prior entry review, and
> no prior human approval for this change — the provenance is an owner
> bug report, not an approval — and nothing here is backdated. The
> entry-review inversion is named in [the retroactive entry
> review](../decisions/141-credential-bound-entry-review.md): the
> record follows the implementation rather than authorizing it.

Authorized by
[the Credential Bound Split by Form Entry Review (retroactive)](../decisions/141-credential-bound-entry-review.md).
Implemented and recorded in
[decision 142](../decisions/142-credential-bound-by-form.md).

Note: the 70-byte bound was documented in the decision 68 record.
That record is immutable and stays as it is; the supersession is noted
in the new decision record only.

(End of file - total 41 lines)
