---
title: "The Add-Form URL-First Reorder"
label: "wayfinder:decision"
status: accepted
date: "2026-08-31"
ticket: "112"
supersedes: []
---

# 130 — The Add-Form URL-First Reorder

Ticket [112](../tickets/112-add-form-url-first.md) · entry review [129](129-url-first-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The URL-first reorder implemented: the endpoint URL comes first matching the user's mental model (URL + key), the provider name auto-derives from the URL host with an editable prefill, and both the URL-first and name-only flows produce valid configurations.

## 2. Implemented I1–I3 — as delivered

| ID  | Implementation                                                                                                                                                                                                                                                                            | Evidence / note                                                                                                                                  |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| I1  | Reordered the ProviderAddField advance chain (Endpoint first, then Model, CredentialEnv, Provider last) in `handle_key` (the Enter advance AND the Up/Down prev/next mappings) and updated the field rendering order in `provider_add_form_lines`. The validation per field is unchanged. | `tui.rs` `ProviderAddField`, `handle_key`, `provider_add_form_lines`; tests `field_order_endpoint_first`, `up_down_navigation_follows_new_order` |
| I2  | Implemented the name derivation fn: `derive_provider_name(url: &str) -> String` per the R2 rules (a pure fn, unit-testable). When the user advances past Endpoint with a non-empty URL, prefill `form.input` with the derived name before showing the Provider field.                     | `tui.rs` `derive_provider_name`; tests `derive_provider_name_examples`, `prefill_is_editable`                                                    |
| I3  | Updated the tests for the new order and added the derivation tests. Kept ALL existing tests compiling (the order-dependent tests update).                                                                                                                                                 | `tui.rs` tests `url_first_flow_completes`, `name_only_flow_still_works`, etc.                                                                    |

## 3. Criteria -> evidence

| Criterion                                 | Evidence                                                                                                                                                                                                                                                 | Verdict |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Field order — Endpoint is the first field | `ProviderAddForm::new` starts at Endpoint; `provider_add_form_lines` order Endpoint -> Model -> CredentialEnv -> Provider; test `field_order_endpoint_first`                                                                                             | pass    |
| Auto-derivation — each example            | `derive_provider_name("https://api.example-vendor.com/v1")` -> "example-vendor"; `derive_provider_name("https://api.openai.com/v1")` -> "openai"; `derive_provider_name("https://vendor.example.com")` -> "vendor"; test `derive_provider_name_examples` | pass    |
| Prefill is editable                       | URL -> prefill present, Backspace clears, typed "my-custom" completes as "my-custom"; test `prefill_is_editable`                                                                                                                                         | pass    |
| Name-only flow still works                | Empty URL -> typed provider "my-provider" completes with endpoint None; test `name_only_flow_still_works`                                                                                                                                                | pass    |
| Up/Down navigation follows the new order  | Endpoint -> Model -> CredentialEnv -> Provider chain for Up/Down; test `up_down_navigation_follows_new_order`                                                                                                                                            | pass    |
| Full form flow with a real URL            | URL https://api.example-vendor.com/v1 -> model gpt-4o -> credential -> derived provider completes; test `url_first_flow_completes`                                                                                                                       | pass    |
| Red lines + stdio-unchanged proof         | Sanitizer single output boundary retained; no threads; no persistence; stdio frontend byte-unchanged except additive form reorder; decisions <= 128 untouched                                                                                            | pass    |

## 4. Result

The URL-first reorder is complete: the form asks for the URL first, the name auto-derives, and both flows work.
