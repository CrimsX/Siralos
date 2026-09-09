---
title: "The Form Order and Fetch-Timeout Fixes Entry Review"
label: "wayfinder:decision"
status: accepted
date: "2026-08-31"
ticket: "114"
supersedes: []
---

# 133 — The Form Order and Fetch-Timeout Fixes Entry Review

Ticket [114](../tickets/114-form-order-timeout-fixes.md) · entry review [103](103-siralos-tui-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** Three fixes from live usage: display name moves to the first field (the name may be left empty and auto-derives from the URL host once the url is entered, or is required at completion when no url is given), the model fetch gets a short 5-second timeout so an unreachable endpoint fails fast with the honest fallback instead of freezing the form for 60 seconds, and the api key error detects when the user typed the key itself and explains the env-var-name distinction.

## 2. Fixes O1–O4 — as authorized

| ID  | Fix                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | Evidence / note                                          |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| O1  | DISPLAY NAME FIRST: the field order becomes DisplayName -> Url -> ApiKey -> ApiProtocol -> Model -> ModelDisplayName. The DisplayName field may be left EMPTY when advancing (no validation error); after the Url field validates, if the display name is still empty the derivation prefill applies (derive_provider_name from the url host); at form completion, an empty display name (no url either) is an error ("a display name is required - enter one or provide a url so one can be derived"). The Up/Down navigation and the Enter/Down advance follow the new order. | `tui.rs` `ProviderAddField`, advance chain, render order |
| O2  | FETCH FAST-FAIL: the fetch_models client timeout drops from 60s to 5 seconds (read) and 3 seconds (connect) — a dead or slow endpoint fails within ~5s and the honest fallback note appears instead of a frozen form. The completion-call client (the existing 60s) is UNCHANGED — only the fetch client tightens.                                                                                                                                                                                                                                                              | `generic.rs` `fetch_models` 5s/3s                        |
| O3  | API KEY ERROR CLARITY: when the api key input fails the env-var-name validation AND looks like a secret (contains lowercase letters, or starts with "sk-", or contains characters outside [A-Z0-9_]), the error says: "this looks like the key itself - Siralos stores the NAME of the environment variable holding your key; create it with setx YOUR_API_KEY_NAME \"the-key\" and enter YOUR_API_KEY_NAME here". Otherwise the standard human-readable env-var message (decision 128) applies.                                                                                | `tui.rs` `validate_credential_env_name` teaching message |
| O4  | THE STALL FIX VERIFIED: with the 5s timeout the api-key advance cannot exceed ~5s; a test proves the fetch failure path completes quickly with the fallback note (a mocked unreachable endpoint or an injected failure).                                                                                                                                                                                                                                                                                                                                                        | test `stall_fix_verified` / fetch failure timing         |

## 3. Criteria -> evidence

| Criterion                         | Evidence                                                                                                                                   | Verdict |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ | ------- |
| Display name first renders        | `provider_add_form_lines` renders display name -> url -> api key -> api protocol -> model -> model display name; test `display_name_first` | pass    |
| Empty display name advance        | Empty display name advances without error; test `empty_display_name_advance`                                                               | pass    |
| Derivation prefill after url      | Url advance prefills empty display name via `derive_provider_name`; test `derivation_prefill_after_url`                                    | pass    |
| Completion error when both empty  | Empty display name with no url errors at completion; test `completion_error_when_both_empty`                                               | pass    |
| Fetch timeout constants 5s/3s     | `fetch_models` timeout 5s and connect 3s asserted; test `fetch_timeout_constants`                                                          | pass    |
| Secret-detection teaching message | Typing sk-abc123 shows teaching message; test `secret_detection_teaching_message`                                                          | pass    |
| Standard error for malformed name | Malformed uppercase name shows standard message; test `standard_error_for_malformed_name`                                                  | pass    |
| Stall fix verified                | Fetch failure completes quickly with fallback note; test `stall_fix_verified`                                                              | pass    |
| Determinism                       | Deterministic render; test `determinism`                                                                                                   | pass    |
| Red lines + no threads + env-only | Sanitizer boundary, no threads, env-only, stdio unchanged, decisions <=132 untouched                                                       | pass    |

## 4. Result

The order and timeout fixes are complete: display name leads, the fetch fails fast, and the api key error teaches the env-var pattern.
