---
title: "The Credential Bound Split by Form Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "118"
supersedes: []
---

# The Credential Bound Split by Form Entry Review

Ticket [118](../tickets/118-credential-bound-by-form.md) · entry review
[the Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record — the inversion, stated plainly.** This entry
> review was written AFTER the change was implemented, verified, and
> committed (commit `68f834f` "fix(profile): split the credential bound
> by form", 2026-09-10). There was no prior ticket, no prior entry
> review, and no prior human approval for this change, and this record
> does not invent or backdate any of them. The provenance is an owner
> bug report (a real OpenRouter key refused by the 70-byte bound), not
> a design approval. The normal order (entry review authorizes →
> implementation lands) is inverted here: the implementation landed
> first and this review records what it should have authorized. The
> verdict below is therefore a retroactive PASS over the
> already-committed diff, not a pre-commit authorization.

> **Owner-reported 2026-09-10 (retroactive).** The `[profile]`
> credential bound was sized for the env form only:
> `MAX_PROFILE_CREDENTIAL_BYTES = 70` is exactly `"env:"` plus a
> 64-char name. When decisions 137/138 added verbatim key storage, the
> whole-value check kept running before the form branch, so any `key:`
> value over 66 characters was refused — a real OpenRouter key is 73
> chars (77 bytes with the prefix). The same validator runs on the load
> path (`ProfileRecord::validate` via `resolve_profile_overlay`), so
> such a profile also failed to apply. The fix: `env:` and bare legacy
> forms keep the 70-byte whole-value bound; `key:` values get
> `MAX_PROFILE_CREDENTIAL_KEY_BYTES = 4096` measured after the prefix;
> every message names its bound.

## 2. The Fixes

| Fix                                    | The change                                                                                                                                                                                                                                                                          | The evidence                                                                            |
| -------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| F1 BOUND SPLIT BY FORM                 | The whole-value 70-byte check no longer runs before the form branch. `env:` and bare legacy credential forms keep the `MAX_PROFILE_CREDENTIAL_BYTES = 70` whole-value bound; `key:` values are measured after the prefix against the new `MAX_PROFILE_CREDENTIAL_KEY_BYTES = 4096`. | `composition.rs` credential validator; the new `MAX_PROFILE_CREDENTIAL_KEY_BYTES` const |
| F2 KEY BOUND MEASURED AFTER THE PREFIX | The 4096-byte `key:` bound applies to the key material only (after the `key:` prefix), so a 73-char real-world key (77 bytes with prefix) validates with wide headroom while unbounded input stays refused.                                                                         | boundary tests around the 4096 post-prefix bound                                        |
| F3 HONEST REJECTIONS                   | Every credential rejection message names the bound it enforces, so a refused value tells the user which bound applied instead of the old bare whole-value message.                                                                                                                  | rejection strings in the committed diff                                                 |
| F4 LOAD PATH COVERED                   | The same split validator runs on the load path (`ProfileRecord::validate` via `resolve_profile_overlay`), so a stored `key:` credential that saves also applies — the save/apply asymmetry is closed.                                                                               | round-trip coverage through save and load                                               |

## 3. Criteria → Evidence

| Criterion                                      | Evidence                                                                                                                                                                                                                                                                                                     | Verdict |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------- |
| A 73-char real key saves AND applies           | new regression test: observed the pre-fix failure `Err("The credential exceeds the 70-byte bound.")` before the change, passes after; covers both the write and the load path                                                                                                                                | pass    |
| `env:` and legacy forms keep the 70-byte bound | existing bound tests unchanged and passing; whole-value check retained for non-`key:` forms                                                                                                                                                                                                                  | pass    |
| `key:` values bounded at 4096 after the prefix | boundary tests at the new const; unbounded input still refused                                                                                                                                                                                                                                               | pass    |
| Every message names its bound                  | rejection strings in the committed diff state the applicable bound                                                                                                                                                                                                                                           | pass    |
| No regression                                  | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 214 cli, 0 failed); `npm run check:rust` green; `npm run check:differential` exit 0, parity 352/352 applicable required (evidence gathered by the orchestrator and cited here, not re-run) | pass    |

## 4. Result

Entry review PASS (retroactive): the by-form credential bound as
committed in `68f834f` is the correct fix — `env:`/legacy keep 70
bytes, `key:` gets 4096 after the prefix, every message names its
bound, and the load path applies what the write path saves. This
review authorized nothing (the code had already landed); it records
what authorization would have covered. The decision 68 record's
70-byte documentation is immutable and untouched; the supersession is
noted in [decision 142](142-credential-bound-by-form.md) only.
