---
title: "The Six-Field Provider Form"
label: "wayfinder:decision"
status: accepted
date: "2026-08-31"
ticket: "113"
supersedes: []
---

# 132 — The Six-Field Provider Form

Ticket [113](../tickets/113-six-field-provider-form.md) · entry review [131](131-six-field-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The six-field form implemented in the user's exact order with no examples in the labels; the model fetch is wired into the form (a picker on success, an honest free-text fallback on failure); two additive profile keys (protocol, model display name) stored and displayed with absent-transparency; the api key field stores the env-var name only — the env-only credential policy holds.

## 2. Implemented I1–I4 — as delivered

| ID  | Implementation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | Evidence / note                                                                                                                                    |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| I1  | Restructure ProviderAddField to the six-field enum (Url, ApiKey, DisplayName, ApiProtocol, Model, ModelDisplayName), the advance chain, the Up/Down mappings, and provider_add_form_lines — labels without examples, descriptions short. The validation per field: url (the existing endpoint validation — https:// or http://, optional), api key (the existing credential env-name validation — the label changes, the policy does not), display name (the existing provider-name validation), api protocol (the closed set), model (free text as before), model display name (optional, printable, bounded 256). | `tui.rs` `ProviderAddField`, `handle_key`, `provider_add_form_lines`, validation fns; tests six-field order, labels, derivation, protocol, display |
| I2  | The fetch integration: after the ApiKey field advance, if the url is non-empty, trigger the fetch (the form gains a fetching_models flag the interactive loop honors — the loop performs the blocking fetch ONCE and hands the result to the form: success -> the form opens the picker state (ModelPicker { items, selected }), failure -> the form sets the fallback note and continues to free text). The picker is part of the form modal (not a separate modal) — Up/Down/Enter/Esc handled inside the form key handler while the picker is open.                                                              | `tui.rs` `ProviderAddForm` fetching + picker, `interactive.rs` loop fetch, `fetch_models`                                                          |
| I3  | The core+adapters schema: ProfileRecord += protocol: Protocol (enum, default OpenAiCompatible) + model_display_name: Option<String>; profile_config parses protocol + model_display_name with absent-transparency and malformed-unapplies; write_profile_config writes them conditionally; the status/header composition prefers the display name.                                                                                                                                                                                                                                                                  | `composition.rs` `Protocol`, `ProfileRecord`, `profile_config.rs` parse, `interactive.rs` write, `tui.rs` display                                  |
| I4  | Update the tests; keep ALL existing tests compiling.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | `tui.rs` + `profile_config` tests ~10, existing suites green                                                                                       |

## 3. Criteria -> evidence

| Criterion                                                  | Evidence                                                                                                                                      | Verdict |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Six-field order renders                                    | `provider_add_form_lines` order url -> api key -> display name -> api protocol -> model -> model display name; test `six_field_order_renders` | pass    |
| No examples in labels                                      | Labels lack "(e.g."; test `no_examples_in_labels`                                                                                             | pass    |
| Derivation prefill on display name                         | Url advance prefills display name; test `derivation_prefill_on_display_name`                                                                  | pass    |
| Protocol validation                                        | openai-compatible and anthropic accepted, gopher rejected; test `protocol_validation`                                                         | pass    |
| Model picker render + Up/Down + Enter                      | Picker with wrapping navigation and Enter selection; test `model_picker_navigation_and_selection`                                             | pass    |
| Picker failure fallback note                               | Honest note on fetch failure; test `picker_failure_fallback_note`                                                                             | pass    |
| Profile parse — protocol present/absent/malformed          | protocol present/absent/malformed matrix; test `profile_parse_protocol_matrix`                                                                | pass    |
| Profile parse — model_display_name present/absent/oversize | model_display_name present/absent/oversize matrix; test `profile_parse_model_display_matrix`                                                  | pass    |
| Write includes new keys                                    | protocol omitted when default, display omitted when empty; test `write_includes_new_keys`                                                     | pass    |
| Header shows display name when present                     | header/status prefer display name; test `header_shows_display_name`                                                                           | pass    |
| Security — api key env-var name only                       | Env-var name only, secret-hygiene green; test `api_key_env_only`                                                                              | pass    |
| Fetch picker flow                                          | fetch -> picker -> select and fallback path covered; test `picker_flow`                                                                       | pass    |
| Parse matrix honest                                        | Present/absent/malformed for both keys exercised with Invalid diagnostic; tests `profile_parse_*`                                             | pass    |
| Determinism                                                | Deterministic render; test `determinism`                                                                                                      | pass    |

## 4. Result

The six-field form is complete: the user's exact structure, the fetch picker integrated, and the additive keys stored and displayed; the protocol shaping is recorded as future work.
