---
title: "The Form Order and Fetch-Timeout Fixes"
label: "wayfinder:decision"
status: accepted
date: "2026-08-31"
ticket: "114"
supersedes: []
---

# 134 — The Form Order and Fetch-Timeout Fixes

Ticket [114](../tickets/114-form-order-timeout-fixes.md) · entry review [133](133-order-timeout-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The fixes implemented: display name leads the form (empty-advance with url-derived prefill, required at completion when no url), the model fetch fails fast at 5 seconds instead of freezing for 60, and the api key error detects a pasted key and teaches the env-var-name pattern.

## 2. Implemented I1–I4 — as delivered

| ID  | Implementation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Evidence / note                                            |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| I1  | Reorder the ProviderAddField advance chain and the Up/Down mappings to DisplayName -> Url -> ApiKey -> ApiProtocol -> Model -> ModelDisplayName; update provider_add_form_lines render order. The DisplayName advance: empty input is accepted (stored as None); the derivation prefill moves to after the Url advance (if display name None and url non-empty -> prefill the derived name into the DisplayName stored value); the completion check: display name empty AND url empty -> error ("a display name is required - enter one or provide a url so one can be derived"). | `tui.rs` field order, advance, render, prefill, completion |
| I2  | In generic.rs fetch_models: .timeout(Duration::from_secs(5)) and .connect_timeout(Duration::from_secs(3)); the completion-call client stays 60s/10s. Update or add tests asserting the constants.                                                                                                                                                                                                                                                                                                                                                                                 | `generic.rs` 5s/3s vs 60s/10s                              |
| I3  | In the api key validation: if the standard validation fails AND the input contains lowercase letters or starts with "sk-", return the teaching message (O3); the validation RULES are unchanged (still [A-Z0-9_]{1,64}).                                                                                                                                                                                                                                                                                                                                                          | `tui.rs` `validate_credential_env_name`                    |
| I4  | Update the tests; keep ALL existing tests compiling (the order-dependent tests update).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | tests ~8 green                                             |

## 3. Criteria -> evidence

| Criterion                         | Evidence                                                                                                        | Verdict |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------- | ------- |
| Display name first renders        | `provider_add_form_lines` renders display name first; test `display_name_first_renders`                         | pass    |
| Empty display name advance        | Empty display name advances without error; test `empty_display_name_advance`                                    | pass    |
| Derivation prefill after url      | Url advance prefills empty display name via `derive_provider_name`; test `derivation_prefill_after_url`         | pass    |
| Completion error when both empty  | Empty display name with no url errors at completion; test `completion_error_when_both_empty`                    | pass    |
| Fetch timeout constants 5s/3s     | `fetch_models` uses 5s read and 3s connect; completion client 60s/10s unchanged; test `fetch_timeout_constants` | pass    |
| Secret-detection teaching message | Typing sk-abc123 shows teaching message; test `secret_detection_teaching_message`                               | pass    |
| Standard error for malformed name | Malformed env-var name shows standard message; test `standard_error_for_malformed_name`                         | pass    |
| Stall fix verified                | Fetch failure completes quickly with fallback note; test `stall_fix_verified`                                   | pass    |
| Determinism                       | Deterministic render; test `determinism`                                                                        | pass    |

## 4. Result

The order and timeout fixes are complete: display name leads, the fetch fails fast, and the api key error teaches the pattern.
