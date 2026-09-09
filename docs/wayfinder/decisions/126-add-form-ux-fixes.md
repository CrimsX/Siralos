---
title: "The Provider Add-Form UX Fixes"
label: "wayfinder:decision"
status: accepted
date: 2026-08-31
ticket: "110"
supersedes: []
---

# 126 — The Provider Add-Form UX Fixes

Ticket [110](../tickets/110-add-form-ux-fixes.md) · entry review [125](125-add-form-ux-fixes-entry-review.md) · Map.

> **User-directed 2026-08-31 (session HITL).** Three add-form UX defects from live usage fixed: URLs are accepted (all printable chars in the Char handler), a visible cursor shows the current position in each field, and Up/Down navigate between fields with per-field validation.

## 2. Implemented F1–F3 — as delivered

| ID  | Implementation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | Evidence / note                                                                                                                                                                                                             |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F1  | URL ACCEPTANCE: in the form's `KeyCode::Char` handler, accept ALL `KeyCode::Char(c)` where `c` is a printable character (not a control char) — the handler does NOT intercept `:` or `/` or any printable char. The handler checks `!c.is_control()` only and pushes `c` into `form.input`; previously the `:`/`/` filtered path is removed.                                                                                                                                                                                                                                                              | `tui.rs` `handle_key` form `KeyCode::Char` `if ch.is_control() return false; form.input.push(ch)`; test `url_acceptance_char_handler_includes_colon_slash` types `https://api.example.com/v1`                               |
| F2  | VISIBLE CURSOR: in the form render, display the current field's input with a visible cursor: append a `█` (U+2588 FULL BLOCK) at the end of the input text (append-only model). The cursor is styled distinctly (same Yellow Bold as the active field). The cursor is ONLY rendered on the currently active field.                                                                                                                                                                                                                                                                                        | `tui.rs` `provider_add_form_lines` `format!("> {}: {}█", label, form.input)` Yellow Bold; `draw_with_pane`/`render_to_buffer_with_pane` modal; test `cursor_renders_in_form_field` checks `█` in frame                      |
| F3  | UP/DOWN FIELD NAVIGATION: the form's Up/Down handling: Up — move `form.field` to the PREVIOUS field in the enum order (Endpoint -> CredentialEnv -> Model -> Provider), restore the previously validated value into `form.input` for re-editing, clear the error; Down — validate the current input (same validation as Enter), if valid store the value and advance to the NEXT field (same as Enter's advance), if invalid show the error and stay. The completed flow (Endpoint + Enter) is unchanged. Up at Provider is a no-op; Down at Endpoint is the same as Enter (completes the form if valid). | `tui.rs` `handle_key` form `KeyCode::Up`/`Down` branches; `field` enum order Provider->Model->CredentialEnv->Endpoint; tests `up_returns_to_previous_field`, `down_advances_with_validation`, `full_form_flow_with_up_down` |

## 3. Criteria -> evidence

| Criterion                                                     | Evidence                                                                                                                                                                                                                      | Verdict |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------- |
| URL acceptance — Char handler accepts `:` and `/`             | Type `https://api.example.com/v1` via `KeyCode::Char` sequence on Endpoint field; `form.input` contains `:` and `/`; test `url_acceptance_char_handler_includes_colon_slash` green                                            | pass    |
| Cursor renders — `█` or `_` in rendered frame                 | Active field line in `provider_add_form_lines` contains `█` at end; `render_to_buffer` frame contains `█`; test `cursor_renders_in_form_field`                                                                                | pass    |
| Up returns to previous field with value restored              | Advance past Provider with Enter, press Up, `form.field == Provider` and `form.input == provider value`; test `up_returns_to_previous_field`                                                                                  | pass    |
| Down advances with validation — valid advances, invalid stays | Valid Model `Down` advances to CredentialEnv; invalid Model stays with `form.error` set; tests `down_advances_with_validation`                                                                                                | pass    |
| Full form flow with Up/Down corrections                       | Fill Provider `myprov`, Model `m1`, CredentialEnv `MY_KEY`, Endpoint `https://api.example.com/v1` with an Up correction; `form.completed.is_some()`; test `full_form_flow_with_up_down`                                       | pass    |
| Determinism                                                   | Same state + size -> byte-equal buffer including form cursor and navigation; test `add_form_determinism`                                                                                                                      | pass    |
| Red lines + stdio-unchanged proof                             | Sanitizer single output boundary retained; approvals host-gated; no threads; no persistence; stdio frontend byte-unchanged except additive form UX (cursor + Up/Down); decisions <= 124 untouched; `interactive::tests` green | pass    |

## 4. Result

The add-form UX fixes are complete: URLs are accepted, the cursor is visible, and Up/Down navigate between fields with validation.
