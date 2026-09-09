---
title: "The Add-Form URL-First Reorder Entry Review"
label: "wayfinder:decision"
status: accepted
date: "2026-08-31"
ticket: "112"
supersedes: []
---

# 129 — The Add-Form URL-First Reorder Entry Review

Ticket [112](../tickets/112-add-form-url-first.md) · entry review [103](103-siralos-tui-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** The add-form reorder: the endpoint URL comes first (the user's mental model is URL + key), the provider name auto-derives from the URL host (api.example-vendor.com -> example-vendor) with an editable prefill, and the user can still override the name.

## 2. Fixes R1–R3 — as authorized

| ID  | Fix                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Evidence / note                                                                                                     |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| R1  | FIELD REORDER: the form field order becomes Endpoint URL -> Model -> Credential env -> Provider name. The Up/Down navigation and the Enter advance follow the new order. The Esc cancel and the completed-data collection are unchanged.                                                                                                                                                                                                                                                                                                                                                                                                   | `tui.rs` `ProviderAddField` order, `handle_key` Enter/Up/Down, `provider_add_form_lines`                            |
| R2  | NAME AUTO-DERIVATION: when the user advances past the Endpoint URL field, the Provider name field PRE-FILLS with a name derived from the URL host: take the host (strip the scheme), strip a leading "api." prefix, take the first dot-separated label, lowercase, replace every character outside [a-z0-9_-] with '-', truncate to 64. Examples: https://api.example-vendor.com/v1 -> example-vendor; https://api.openai.com/v1 -> openai; https://vendor.example.com -> vendor. The prefill is EDITABLE — the user can clear it and type their own name. If the URL was left empty, the name field starts empty (the old name-only flow). | `tui.rs` `derive_provider_name`, `handle_key` prefill, tests `derive_provider_name_examples`, `prefill_is_editable` |
| R3  | THE BOTH-FLOWS GUARANTEE: URL-first flow: URL -> model -> credential -> auto-derived name (editable) -> complete. Name-only flow: empty URL -> model -> credential -> typed name -> complete (the default endpoint applies). Both flows produce valid completed data.                                                                                                                                                                                                                                                                                                                                                                      | `tui.rs` `handle_key` both flows, tests `url_first_flow_completes`, `name_only_flow_still_works`                    |

## 3. Criteria -> evidence

| Criterion                                   | Evidence                                                                                                                           | Verdict |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ------- |
| Field order — Endpoint is the first field   | `ProviderAddForm::new` field is Endpoint; `provider_add_form_lines` renders Endpoint first; test `field_order_endpoint_first`      | pass    |
| Auto-derivation — each example above        | `derive_provider_name("https://api.example-vendor.com/v1") == "example-vendor"` etc.; test `derive_provider_name_examples`                | pass    |
| Prefill is editable — clear and type custom | Advance past Endpoint with URL, prefill present, Backspace clear and type custom, complete uses custom; test `prefill_is_editable` | pass    |
| Name-only flow still works                  | Empty URL -> model -> credential -> typed name -> complete with endpoint None; test `name_only_flow_still_works`                   | pass    |
| Up/Down navigation follows the new order    | Up from Model goes to Endpoint, Down from Endpoint goes to Model, etc.; test `up_down_navigation_follows_new_order`                | pass    |
| Full form flow with a real URL              | Sequential URL -> model -> credential -> derived provider completes; test `url_first_flow_completes`                               | pass    |
| Red lines honored                           | Sanitizer single output boundary; no threads; no persistence; stdio frontend byte-unchanged; decisions <= 128 untouched            | pass    |

## 4. Result

The URL-first reorder is complete: the form asks for the URL first, the name auto-derives, and both flows (URL-first and name-only) work.
