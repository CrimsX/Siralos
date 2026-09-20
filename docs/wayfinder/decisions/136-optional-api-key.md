---
title: "The Optional API Key"
label: "wayfinder:decision"
status: "accepted"
date: "2026-08-31"
ticket: "115"
supersedes: []
---

# The Optional API Key

Ticket [115](../tickets/115-optional-api-key.md) · entry review
[135](135-optional-api-key-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** The api key field is
> optional: empty = no credential for public endpoints (the profile schema
> already supported an absent credential); the written config omits the
> credential key; the fetch proceeds without authentication; a provided
> credential remains the env-var NAME.

## 2. The Implemented

| Fix                   | The change                                                                                                                                                                                                                                                           | The evidence                                                                                  |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| I1 EMPTY-SKIP         | Both key handlers accept an empty ApiKey advance (`credential_env = None`); non-empty validates as before.                                                                                                                                                           | `empty_api_key_advances_without_credential`, `nonempty_public_rejected_with_teaching_message` |
| I2 DATA MODEL         | `ProviderAddData.credential_env: Option<String>`; `write_profile_config(credential_env: Option<&str>)` writes the credential line only when present and removes the key otherwise; the verify-shim comparison handles the None case.                                 | `write_profile_config_omits_credential_when_none`                                             |
| I3 FETCH WITHOUT AUTH | `fetch_models` with no credential sends no Authorization header (verified, no change needed).                                                                                                                                                                        | K3 verification recorded                                                                      |
| I4 THE LATENT DEFECT  | The fresh-workspace write (no existing `siralos.toml`) panicked on a chained `toml_edit` index in the `[profile].name` defaulting — exposed by the new test, fixed with defensive navigation. The empty ModelDisplayName advance also fixed (the field is optional). | the panic test exposed both; both fixed                                                       |

## 3. Criteria → Evidence

| Criterion                 | Evidence                                                                                   | Verdict |
| ------------------------- | ------------------------------------------------------------------------------------------ | ------- |
| The public flow completes | `public_flow_completes_with_no_credential` end-to-end                                      | pass    |
| The config omission       | `write_profile_config_omits_credential_when_none` (no credential key; the profile applies) | pass    |
| The env-only policy       | The provided credential remains `env:<NAME>`; the teaching error unchanged                 | pass    |
| Determinism & stdio       | The full gate green (fmt, clippy, all-features tests, differential 352/352)                | pass    |

## 4. Result

The optional api key is complete: empty skips the credential, public
endpoints work end-to-end, and the config omits the credential key.

## 5. Correction — the I1 evidence and the teaching error (round 23)

Two cells above cite the teaching error as evidence that is unchanged:

- **I1** cites `nonempty_public_rejected_with_teaching_message` alongside
  `empty_api_key_advances_without_credential`. The first name exists nowhere in
  the repository.
- The criteria row "The env-only policy … the teaching error unchanged" records
  as passing something that no longer exists. The teaching text was deleted in
  the round-3 consolidation, and neither live ApiKey handler validates the field:
  the advance (`crates/siralos-cli/src/tui.rs:3510`) stores a non-`env:` value
  verbatim as `key:<value>`, and that field's validator is
  `// Verbatim credential: no validation.` followed by `Ok(())`
  (`crates/siralos-cli/src/tui.rs:3771`).

`empty_api_key_advances_without_credential` and
`write_profile_config_omits_credential_when_none` both survive
(`crates/siralos-cli/src/tui/tests.rs:2308`,
`crates/siralos-cli/src/interactive/tests.rs:1094`), so the empty-skip and
config-omission rows still rest on runnable evidence. Decision 135 §5 records the
same correction; whether to restore validation and the teaching message is an
owner decision in `ROADMAP.md` §10.
