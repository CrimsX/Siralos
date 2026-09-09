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
