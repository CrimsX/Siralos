---
title: "The Credential Bound Split by Form"
label: "wayfinder:decision"
status: "accepted"
date: "2026-09-10"
ticket: "118"
supersedes: []
---

# The Credential Bound Split by Form

Ticket [118](../tickets/118-credential-bound-by-form.md) · entry review
[141](141-credential-bound-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **Retroactive record.** This change was implemented, verified, and
> committed (commit `530568c` "fix(profile): split the credential bound
> by form", 2026-09-10) BEFORE this record was written. There was no
> prior ticket, no prior entry review, and no prior human approval for
> this change — the provenance is an owner bug report, not an
> approval; this record invents none of them and backdates nothing.
> The entry review ([141](141-credential-bound-entry-review.md)) is
> itself retroactive and names the inversion.

## 2. The Implemented

| Fix                    | The change                                                                                                                                                                                                                                                                             | The evidence                                |
| ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| M1 BOUND SPLIT BY FORM | The credential validator branches on form before measuring: `env:` and bare legacy values are checked whole against `MAX_PROFILE_CREDENTIAL_BYTES = 70`; `key:` values are checked after the prefix against `MAX_PROFILE_CREDENTIAL_KEY_BYTES = 4096`.                                 | the committed validator diff; the new const |
| M2 HONEST MESSAGES     | Every credential rejection states the bound it enforces; the old bare whole-value message is gone.                                                                                                                                                                                     | rejection strings in the committed diff     |
| M3 TESTS               | A regression test pins the owner-reported case (a 73-char key, 77 bytes with prefix) on both the write and the load (`ProfileRecord::validate` via `resolve_profile_overlay`) paths; boundary tests pin the 4096 post-prefix bound and the retained 70-byte bound for the other forms. | the new tests in the committed diff         |

## 3. Criteria → Evidence

| Criterion                               | Evidence                                                                                                                                                                                                                                                                                                               | Verdict  |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| Real-world `key:` values save and apply | regression test passes post-fix (failed pre-fix with `Err("The credential exceeds the 70-byte bound.")`); load path covered                                                                                                                                                                                            | pass     |
| `env:`/legacy bound unchanged           | existing 70-byte tests pass unmodified                                                                                                                                                                                                                                                                                 | pass     |
| No regression                           | `cargo test --workspace --all-targets --all-features --locked` exit 0 (614 core / 306 adapters / 25 conformance / 214 cli, 0 failed); `npm run check:rust` green; `npm run check:differential` exit 0, parity held 352/352 applicable required (evidence gathered by the orchestrator and cited here, not re-invented) | pass     |
| Docs/context gates                      | `npm run check:docs` and `npm run check:context` outcomes reported below                                                                                                                                                                                                                                               | reported |

## 4. Result

The by-form credential bound is complete as committed in `530568c`:
`env:` and legacy forms keep the 70-byte whole-value bound, `key:`
values get 4096 bytes after the prefix, every rejection names its
bound, and a stored verbatim key applies at load. Retroactive record
closed; ticket 118 done.

Supersession note (this record only): the 70-byte env-only bound
documented in the decision 68 record is superseded for `key:`-form
credentials by the 4096-byte post-prefix bound. The decision 68 record
itself is immutable and stays as it is.

(End of file - total 48 lines)
