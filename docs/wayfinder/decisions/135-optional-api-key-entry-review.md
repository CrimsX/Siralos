---
title: "The Optional API Key Entry Review"
label: "wayfinder:decision"
status: "accepted"
date: "2026-08-31"
ticket: "115"
supersedes: []
---

# The Optional API Key Entry Review

Ticket [115](../tickets/115-optional-api-key.md) · entry review
[the Siralos TUI entry review](103-siralos-tui-entry-review.md) ·
[Map](../siralos-roadmap.md)

> **User-directed 2026-08-31 (session HITL).** The api key field becomes
> OPTIONAL: empty = no credential (a public endpoint), matching the profile
> schema which already treats credential as absent-able. When a credential
> IS given it is still the env-var NAME (the env-only policy holds). The
> model fetch proceeds without an Authorization header when no credential
> is set.

## 2. The Fixes

| Fix                       | The change                                                                                                                                                                                                                                                | The evidence                                                                                                                    |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| K1 EMPTY-SKIP             | The ApiKey advance accepts EMPTY input — `credential_env = None`, advance to ApiProtocol. Non-empty input validates as before (the human-readable + teaching errors unchanged).                                                                           | `tui.rs` both key handlers; tests `empty_api_key_advances_without_credential`, `nonempty_public_rejected_with_teaching_message` |
| K2 DATA MODEL             | `ProviderAddData.credential_env` becomes `Option<String>`; `write_profile_config` takes the credential as `Option` and OMITS the `[profile]` credential key when None; the re-parse validation still passes (an absent credential parses — already true). | `write_profile_config_omits_credential_when_none`                                                                               |
| K3 THE FETCH WITHOUT AUTH | The model fetch with no credential sends NO Authorization header.                                                                                                                                                                                         | Verified in `generic.rs` `fetch_models` (the `Option<&HostCredential>` path)                                                    |
| K4 THE PUBLIC FLOW        | url → api key (empty, skipped) → protocol → model fetch (no auth; on failure the fallback note) → model → display → complete. The written config has NO credential key.                                                                                   | `public_flow_completes_with_no_credential`                                                                                      |

## 3. Criteria → Evidence

| Criterion                               | Evidence                                                                                                                                                  | Verdict |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| The public flow completes end-to-end    | `public_flow_completes_with_no_credential`: display name, url, empty key, protocol, model, empty display → completed data carries `credential_env: None`  | pass    |
| The written config omits the credential | `write_profile_config_omits_credential_when_none`: the written `siralos.toml` contains no credential key and `load_workspace_profile` applies the profile | pass    |
| The env-only policy holds               | A provided credential remains the env-var NAME; the secret value never enters the config or the transcript                                                | pass    |
| The exposed latent defect               | The fresh-workspace write path (no existing `siralos.toml`) panicked on a chained toml_edit index — found by the new test and fixed defensively           | pass    |

## 4. Result

Entry review PASS: the api key is optional for public endpoints with the
env-only credential policy untouched.
