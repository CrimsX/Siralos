//! Key and mouse handling: turn keys, scrolling, palette completion,
//! and input acceptance for [`TuiState`].

use super::{
    MAX_INPUT_BYTES, MOUSE_WHEEL_STEP, ProviderAddData, ProviderAddField,
    ProviderAddForm, TuiState, command_catalog, derive_provider_name,
    local_timestamp_now, mouse_capture_message, normalize_credential_input,
    open_protocol_picker, open_provider_remove_confirm, validate_api_protocol,
    validate_endpoint_value, validate_model_display_name, validate_model_name,
    validate_provider_name,
};

/// Compute the longest common prefix among non-empty strings (case-sensitive,
/// char-level). Empty input returns empty.
fn longest_common_prefix(strs: &[String]) -> String {
    if strs.is_empty() {
        return String::new();
    }
    let mut prefix = strs[0].clone();
    for s in &strs[1..] {
        let mut new_len = 0;
        for (a, b) in prefix.chars().zip(s.chars()) {
            if a == b {
                new_len += a.len_utf8();
            } else {
                break;
            }
        }
        prefix.truncate(new_len);
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

/// Handle Tab completion for the palette (C5 + I2): when input starts with `/` and
/// there are matches, Tab completes the typed prefix. If a palette entry is
/// selected (I2), Tab completes to that entry and clears the palette; otherwise
/// one match -> full, multiple -> common prefix. No match is a no-op.
fn complete_palette_prefix(state: &mut TuiState) {
    if !state.input.starts_with('/') {
        return;
    }
    // I2: Tab on a selected entry completes to that entry and clears palette.
    if let Some(idx) = state.palette_selected {
        if let Some(palette) = &state.palette {
            if let Some((name, _)) = palette.get(idx) {
                state.input = name.clone();
                state.palette = None;
                state.palette_selected = None;
                return;
            }
        }
    }
    let prefix_lower = state.input.to_ascii_lowercase();
    let matches: Vec<String> = command_catalog()
        .into_iter()
        .filter(|(name, _)| {
            name.to_ascii_lowercase().starts_with(&prefix_lower)
        })
        .map(|(name, _)| name)
        .collect();
    if matches.is_empty() {
        return;
    }
    if matches.len() == 1 {
        state.input = matches[0].clone();
    } else {
        let common = longest_common_prefix(&matches);
        if common.len() > state.input.len() {
            state.input = common;
        }
    }
    state.update_palette();
}

/// Flip mouse capture and return the resulting state line (pure state
/// flip; the live loop pairs it with the terminal escape immediately).
/// ON hands the wheel to the transcript; OFF hands the mouse back to the
/// terminal for click-drag text selection.
pub fn toggle_mouse_capture(state: &mut TuiState) -> &'static str {
    state.mouse_capture = !state.mouse_capture;
    mouse_capture_message(state.mouse_capture)
}

/// Handle one key pressed WHILE a turn is running (S2 chunk 4b + S3b).
///
/// The input line is busy with the model, so the keys mean: typed characters
/// and backspace are kept as type-ahead for the next prompt, the arrows
/// expand and collapse the thinking block (so thinking is readable MID
/// FLIGHT, not only after the turn), and Esc asks for an interrupt.
///
/// Returns true only for Esc -- the caller owns cancellation.
#[must_use]
pub fn apply_turn_key(
    state: &mut TuiState,
    key: crossterm::event::KeyEvent,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    if key.kind != crossterm::event::KeyEventKind::Press {
        return false;
    }
    // A chorded key belongs to the loop, not to the type-ahead: Ctrl+C is
    // the exit key, and folding it into the prompt would kill it for the
    // whole turn.
    if key.code == KeyCode::Char('c')
        && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return true;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
    {
        return false;
    }
    match key.code {
        KeyCode::Esc => true,
        KeyCode::Char(ch) => {
            if state.input.len() + ch.len_utf8() <= MAX_INPUT_BYTES {
                state.input.push(ch);
            }
            false
        }
        KeyCode::Backspace => {
            state.input.pop();
            false
        }
        KeyCode::Right => {
            if !state.reasoning.trim().is_empty() {
                state.reasoning_expanded = true;
            }
            false
        }
        KeyCode::Left => {
            state.reasoning_expanded = false;
            false
        }
        _ => false,
    }
}
/// Take the submitted input: clear the box and the palette, echo the line
/// into the transcript, and report the line to run (`None` for an empty
/// submit).
///
/// Owner QoL 2026-09-12 (S1): this was inline in the TUI loop, which is why
/// "pressing Enter should clear the message" had no test behind it. The
/// state change is pure and tested here; the loop only composes the status
/// line and dispatches.
#[must_use]
pub fn accept_submitted_input(state: &mut TuiState) -> Option<String> {
    let line = std::mem::take(&mut state.input);
    state.palette = None;
    state.palette_selected = None;
    if line.trim().is_empty() {
        return None;
    }
    let echo = format!("> {}", crate::sanitize::sanitize_for_display(&line));
    state.push_line_stamped(echo, Some(local_timestamp_now()));
    Some(line)
}

/// Handle a mouse event for transcript scrolling (option b).
///
/// Only wheel notches move anything, by [`MOUSE_WHEEL_STEP`] rows through
/// the SAME `scroll_offset` clamp/max logic `PageUp`/`PageDown` use — no
/// second scroll mechanism, no other state touched. All other mouse kinds
/// (press/release/drag/move) are ignored: the TUI takes no click action.
///
/// MODAL DECISION (reported): while any modal, picker, or the add-form is
/// open, wheel events are IGNORED — the same modal discipline that ignores
/// non-modal keys. The transcript behind a confirmation must not move
/// under the question being asked; key handling is untouched (this
/// function never reads or writes keys, input, palette, or history).
pub fn handle_mouse(
    state: &mut TuiState,
    event: crossterm::event::MouseEvent,
    viewport_height: u16,
) {
    use crossterm::event::MouseEventKind;
    if state
        .pending_approval
        .as_ref()
        .and_then(|approval| approval.modal())
        .is_some()
        || state.provider_add_form.is_some()
        || state.provider_picker.is_some()
        || state.model_switch_picker.is_some()
    {
        return;
    }
    match event.kind {
        MouseEventKind::ScrollUp => {
            let max = state.max_scroll(viewport_height);
            state.scroll_offset =
                (state.scroll_offset.saturating_add(MOUSE_WHEEL_STEP))
                    .min(max);
        }
        MouseEventKind::ScrollDown => {
            state.scroll_offset =
                state.scroll_offset.saturating_sub(MOUSE_WHEEL_STEP);
        }
        _ => {}
    }
}

/// Handle a key event for the input line and scroll state. Returns true if the
/// Enter key was pressed (caller should submit `state.input`).
/// While a modal is pending this function returns `false` for all keys
/// (callers must route through [`handle_modal_key`](super::approval::handle_modal_key) first — no typing through
/// a modal).
/// The provider add-form (C1) has modal discipline: while it is Some, NO other
/// keys pass (modal discipline).
pub fn handle_key(
    state: &mut TuiState,
    key: crossterm::event::KeyEvent,
    viewport_height: u16,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    if state
        .pending_approval
        .as_ref()
        .and_then(|approval| approval.modal())
        .is_some()
    {
        // T2: while a modal is pending, ALL other keys are ignored.
        return false;
    }
    // C1: provider add-form has modal discipline — consumes ALL non-Esc/Enter/Char/Backspace keys too.
    if let Some(form) = state.provider_add_form.as_mut() {
        // While completed, consume all until the interactive loop drains it.
        if form.completed.is_some() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                // When the protocol picker is open, Esc falls back to free text.
                if form.field == ProviderAddField::ApiProtocol
                    && form.protocol_picker.is_some()
                {
                    form.protocol_picker = None;
                    return false;
                }
                // When the picker is open, Esc falls back to free text (S2).
                if form.field == ProviderAddField::Model
                    && form.model_picker.is_some()
                {
                    form.model_picker = None;
                    form.fetch_note = Some(
                        "model list unavailable from this provider - enter the model manually"
                            .to_owned(),
                    );
                    return false;
                }
                if form.fetching_models {
                    // While fetching, Esc cancels the whole form (modal discipline).
                    state.provider_add_form = None;
                    return false;
                }
                state.provider_add_form = None;
                return false;
            }
            KeyCode::Enter => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                let current = form.input.clone();
                let trimmed = current.trim().to_owned();
                let field = form.field;
                let field_value = if field == ProviderAddField::ApiKey {
                    current.clone()
                } else {
                    trimmed.clone()
                };
                // Validate current field and advance or error.
                // Picker interception for Protocol field: Enter selects highlighted protocol.
                if form.field == ProviderAddField::ApiProtocol
                    && form.protocol_picker.is_some()
                {
                    if let Some(picker) = form.protocol_picker.take() {
                        let selected = picker.items[picker.selected].clone();
                        form.protocol = Some(selected.clone());
                        form.field = ProviderAddField::Model;
                        form.input.clear();
                        form.error = None;
                    }
                    return false;
                }
                // Picker interception for Model field: Up/Down handled below, Enter here selects.
                if form.field == ProviderAddField::Model
                    && form.model_picker.is_some()
                {
                    // Enter selects the highlighted model id.
                    if let Some(picker) = form.model_picker.take() {
                        let selected = picker.items[picker.selected].clone();
                        form.model = Some(selected.clone());
                        form.field = ProviderAddField::ModelDisplayName;
                        form.input.clear();
                        form.error = None;
                        form.fetch_note = None;
                    }
                    return false;
                }
                let validation: Result<(), String> = match field {
                    ProviderAddField::DisplayName => {
                        if trimmed.is_empty() {
                            Ok(())
                        } else {
                            validate_provider_name(&trimmed)
                        }
                    }
                    ProviderAddField::Url => {
                        if trimmed.is_empty() {
                            Ok(())
                        } else {
                            validate_endpoint_value(&trimmed)
                        }
                    }
                    ProviderAddField::ApiKey => {
                        // Verbatim credential: no validation — field is an interface, not a validator.
                        Ok(())
                    }
                    ProviderAddField::ApiProtocol => {
                        // When picker is present, validation is bypassed (picker selection handled above).
                        // Free-text fallback validated against closed set.
                        validate_api_protocol(&trimmed)
                    }
                    ProviderAddField::Model => validate_model_name(&trimmed),
                    ProviderAddField::ModelDisplayName => {
                        // Optional: empty skips the display name.
                        if trimmed.is_empty() {
                            Ok(())
                        } else {
                            validate_model_display_name(&trimmed)
                        }
                    }
                };
                if let Err(msg) = validation {
                    form.error = Some(msg);
                    return false;
                }
                form.error = None;
                match field {
                    ProviderAddField::DisplayName => {
                        let provider_opt = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        };
                        form.provider = provider_opt;
                        form.field = ProviderAddField::Url;
                        form.input.clear();
                    }
                    ProviderAddField::Url => {
                        let endpoint_opt = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        };
                        form.endpoint = endpoint_opt.clone();
                        // O1 derivation prefill: if display name still empty and url non-empty, prefill derived name
                        if form.provider.is_none() {
                            if let Some(ep) = endpoint_opt.as_deref() {
                                if !ep.is_empty() {
                                    let derived = derive_provider_name(ep);
                                    if !derived.is_empty() {
                                        form.provider = Some(derived);
                                    }
                                }
                            }
                        }
                        form.field = ProviderAddField::ApiKey;
                        form.input.clear();
                    }
                    ProviderAddField::ApiKey => {
                        let credential_opt =
                            normalize_credential_input(&field_value);
                        form.credential_env = credential_opt;
                        form.field = ProviderAddField::ApiProtocol;
                        // Open protocol picker with pre-selected item
                        open_protocol_picker(form);
                        // Model discovery is deferred until the protocol is
                        // selected; the route shape is part of the probe.
                        form.fetching_models = false;
                    }
                    ProviderAddField::ApiProtocol => {
                        form.protocol = Some(trimmed);
                        form.protocol_picker = None;
                        form.field = ProviderAddField::Model;
                        if form
                            .endpoint
                            .as_deref()
                            .is_some_and(|endpoint| !endpoint.is_empty())
                        {
                            form.fetching_models = true;
                            form.fetch_note = None;
                            form.model_picker = None;
                        }
                        // When entering Model field, if picker already populated (fetch completed while on prior fields),
                        // it will render inline; otherwise free text.
                        form.input.clear();
                    }
                    ProviderAddField::Model => {
                        form.model = Some(trimmed);
                        form.field = ProviderAddField::ModelDisplayName;
                        form.input.clear();
                    }
                    ProviderAddField::ModelDisplayName => {
                        let display_opt = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.clone())
                        };
                        form.model_display_name = display_opt;
                        // O1 completion check: display name empty AND url empty -> error
                        if form.provider.is_none() {
                            let endpoint_empty = form
                                .endpoint
                                .as_deref()
                                .map(|s| s.is_empty())
                                .unwrap_or(true);
                            if endpoint_empty {
                                form.error = Some(
                                    "a display name is required - enter one or provide a url so one can be derived"
                                        .to_owned(),
                                );
                                return false;
                            }
                        }
                        // Validate that prior fields were collected.
                        // The api key is optional: an empty key completes with
                        // `credential_env: None` (public endpoint, no credential).
                        if let (Some(provider), Some(model)) =
                            (form.provider.clone(), form.model.clone())
                        {
                            let named_provider = provider.to_ascii_lowercase();
                            let is_named = matches!(
                                named_provider.as_str(),
                                "openai" | "anthropic"
                            );
                            let protocol = if named_provider == "openai" {
                                "openai-completions".to_owned()
                            } else if named_provider == "anthropic" {
                                "anthropic-messages".to_owned()
                            } else {
                                form.protocol.clone().unwrap_or_else(|| {
                                    "openai-completions".to_owned()
                                })
                            };
                            form.completed = Some(ProviderAddData {
                                provider: if is_named {
                                    named_provider
                                } else {
                                    provider
                                },
                                model,
                                credential_env: form.credential_env.clone(),
                                // Named adapters own fixed routes; a custom
                                // endpoint/protocol would be rejected by the
                                // registry and break the next compose.
                                endpoint: if is_named {
                                    None
                                } else {
                                    form.endpoint.clone()
                                },
                                protocol,
                                model_display_name: form
                                    .model_display_name
                                    .clone(),
                            });
                        } else {
                            form.error = Some(
                                "Internal error: missing prior fields"
                                    .to_owned(),
                            );
                        }
                    }
                }
                return false;
            }
            KeyCode::Backspace => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                if form.field == ProviderAddField::ApiProtocol
                    && form.protocol_picker.is_some()
                {
                    return false;
                }
                if form.field == ProviderAddField::Model
                    && form.model_picker.is_some()
                {
                    return false;
                }
                if form.fetching_models
                    && form.field == ProviderAddField::Model
                {
                    return false;
                }
                form.input.pop();
                form.error = None;
                return false;
            }
            KeyCode::Up => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                // Picker navigation: when the protocol picker is open, Up wraps within the picker.
                if form.field == ProviderAddField::ApiProtocol {
                    if let Some(picker) = form.protocol_picker.as_mut() {
                        if picker.items.is_empty() {
                            return false;
                        }
                        if picker.selected == 0 {
                            picker.selected = picker.items.len() - 1;
                        } else {
                            picker.selected -= 1;
                        }
                        return false;
                    }
                }
                // Picker navigation: when the model picker is open, Up wraps within the picker.
                if form.field == ProviderAddField::Model {
                    if let Some(picker) = form.model_picker.as_mut() {
                        if picker.items.is_empty() {
                            return false;
                        }
                        if picker.selected == 0 {
                            picker.selected = picker.items.len() - 1;
                        } else {
                            picker.selected -= 1;
                        }
                        return false;
                    }
                }
                // While fetching, Up is ignored for the Model field (freeze documented).
                if form.fetching_models
                    && form.field == ProviderAddField::Model
                {
                    return false;
                }
                // Up: previous field, restoring validated value for re-editing (O1 order).
                let prev = match form.field {
                    ProviderAddField::DisplayName => None,
                    ProviderAddField::Url => {
                        Some(ProviderAddField::DisplayName)
                    }
                    ProviderAddField::ApiKey => Some(ProviderAddField::Url),
                    ProviderAddField::ApiProtocol => {
                        Some(ProviderAddField::ApiKey)
                    }
                    ProviderAddField::Model => {
                        Some(ProviderAddField::ApiProtocol)
                    }
                    ProviderAddField::ModelDisplayName => {
                        Some(ProviderAddField::Model)
                    }
                };
                if let Some(prev_field) = prev {
                    form.field = prev_field;
                    if prev_field == ProviderAddField::ApiProtocol {
                        open_protocol_picker(form);
                    } else {
                        let restored = match prev_field {
                            ProviderAddField::Url => {
                                form.endpoint.clone().unwrap_or_default()
                            }
                            ProviderAddField::ApiKey => {
                                form.credential_env.clone().unwrap_or_default()
                            }
                            ProviderAddField::DisplayName => {
                                form.provider.clone().unwrap_or_default()
                            }
                            ProviderAddField::ApiProtocol => {
                                form.protocol.clone().unwrap_or_default()
                            }
                            ProviderAddField::Model => {
                                form.model.clone().unwrap_or_default()
                            }
                            ProviderAddField::ModelDisplayName => form
                                .model_display_name
                                .clone()
                                .unwrap_or_default(),
                        };
                        form.input = restored;
                        form.error = None;
                        // Clear protocol picker when leaving ApiProtocol
                        if prev_field != ProviderAddField::ApiProtocol {
                            form.protocol_picker = None;
                        }
                    }
                }
                return false;
            }
            KeyCode::Down => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                // Picker navigation for Down when protocol picker is open.
                if form.field == ProviderAddField::ApiProtocol {
                    if let Some(picker) = form.protocol_picker.as_mut() {
                        if !picker.items.is_empty() {
                            picker.selected =
                                (picker.selected + 1) % picker.items.len();
                        }
                        return false;
                    }
                }
                // Picker navigation for Down when picker is open.
                if form.field == ProviderAddField::Model {
                    if let Some(picker) = form.model_picker.as_mut() {
                        if !picker.items.is_empty() {
                            picker.selected =
                                (picker.selected + 1) % picker.items.len();
                        }
                        return false;
                    }
                }
                if form.fetching_models
                    && form.field == ProviderAddField::Model
                {
                    return false;
                }
                // Down: validate current, if valid advance to next (same as Enter).
                // If protocol picker is open, Down already handled navigation above; validation step only for free-text fallback.
                if form.field == ProviderAddField::ApiProtocol
                    && form.protocol_picker.is_some()
                {
                    // When picker is open, Down is navigation (handled), not advance.
                    // To advance, user must press Enter to select; free-text path requires Esc first.
                    return false;
                }
                let current = form.input.clone();
                let trimmed = current.trim().to_owned();
                let field = form.field;
                let field_value = if field == ProviderAddField::ApiKey {
                    current.clone()
                } else {
                    trimmed.clone()
                };
                let validation: Result<(), String> = match field {
                    ProviderAddField::DisplayName => {
                        if trimmed.is_empty() {
                            Ok(())
                        } else {
                            validate_provider_name(&trimmed)
                        }
                    }
                    ProviderAddField::Url => {
                        if trimmed.is_empty() {
                            Ok(())
                        } else {
                            validate_endpoint_value(&trimmed)
                        }
                    }
                    ProviderAddField::ApiKey => {
                        // Verbatim credential: no validation.
                        Ok(())
                    }
                    ProviderAddField::ApiProtocol => {
                        validate_api_protocol(&trimmed)
                    }
                    ProviderAddField::Model => validate_model_name(&trimmed),
                    ProviderAddField::ModelDisplayName => {
                        // Optional: empty skips the display name.
                        if trimmed.is_empty() {
                            Ok(())
                        } else {
                            validate_model_display_name(&trimmed)
                        }
                    }
                };
                if let Err(msg) = validation {
                    form.error = Some(msg);
                    return false;
                }
                form.error = None;
                match field {
                    ProviderAddField::DisplayName => {
                        let provider_opt = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        };
                        form.provider = provider_opt;
                        form.field = ProviderAddField::Url;
                        form.input.clear();
                    }
                    ProviderAddField::Url => {
                        let endpoint_opt = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed)
                        };
                        form.endpoint = endpoint_opt.clone();
                        // O1 derivation prefill: if display name still empty and url non-empty, prefill derived name
                        if form.provider.is_none() {
                            if let Some(ep) = endpoint_opt.as_deref() {
                                if !ep.is_empty() {
                                    let derived = derive_provider_name(ep);
                                    if !derived.is_empty() {
                                        form.provider = Some(derived);
                                    }
                                }
                            }
                        }
                        form.field = ProviderAddField::ApiKey;
                        form.input.clear();
                    }
                    ProviderAddField::ApiKey => {
                        let credential_opt =
                            normalize_credential_input(&field_value);
                        form.credential_env = credential_opt;
                        form.field = ProviderAddField::ApiProtocol;
                        open_protocol_picker(form);
                        // Model discovery is deferred until the protocol is
                        // selected; the route shape is part of the probe.
                        form.fetching_models = false;
                    }
                    ProviderAddField::ApiProtocol => {
                        form.protocol = Some(trimmed);
                        form.protocol_picker = None;
                        form.field = ProviderAddField::Model;
                        form.input.clear();
                    }
                    ProviderAddField::Model => {
                        form.model = Some(trimmed);
                        form.field = ProviderAddField::ModelDisplayName;
                        form.input.clear();
                    }
                    ProviderAddField::ModelDisplayName => {
                        let display_opt = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.clone())
                        };
                        form.model_display_name = display_opt;
                        // O1 completion check: display name empty AND url empty -> error
                        if form.provider.is_none() {
                            let endpoint_empty = form
                                .endpoint
                                .as_deref()
                                .map(|s| s.is_empty())
                                .unwrap_or(true);
                            if endpoint_empty {
                                form.error = Some(
                                    "a display name is required - enter one or provide a url so one can be derived"
                                        .to_owned(),
                                );
                                return false;
                            }
                        }
                        if let (Some(provider), Some(model)) =
                            (form.provider.clone(), form.model.clone())
                        {
                            let protocol =
                                form.protocol.clone().unwrap_or_else(|| {
                                    "openai-completions".to_owned()
                                });
                            form.completed = Some(ProviderAddData {
                                provider,
                                model,
                                // K1: the api key is optional — None for a
                                // public endpoint (no credential written).
                                credential_env: form.credential_env.clone(),
                                endpoint: form.endpoint.clone(),
                                protocol,
                                model_display_name: form
                                    .model_display_name
                                    .clone(),
                            });
                        } else {
                            form.error = Some(
                                "Internal error: missing prior fields"
                                    .to_owned(),
                            );
                        }
                    }
                }
                return false;
            }
            KeyCode::Char(ch) => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                // Ctrl combos are handled by outer loop; here just char.
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return false;
                }
                if form.field == ProviderAddField::ApiProtocol
                    && form.protocol_picker.is_some()
                {
                    return false;
                }
                // While picker is open, free-text typing is gated behind Esc (S2).
                if form.field == ProviderAddField::Model
                    && form.model_picker.is_some()
                {
                    return false;
                }
                if form.fetching_models
                    && form.field == ProviderAddField::Model
                {
                    return false;
                }
                // F1: accept ALL printable chars including : and / (only reject control chars).
                if ch.is_control() {
                    return false;
                }
                if form.input.len() + ch.len_utf8() <= MAX_INPUT_BYTES {
                    form.input.push(ch);
                }
                form.error = None;
                return false;
            }
            _ => {
                // Modal discipline: consume all other keys while form is open.
                return false;
            }
        }
    }
    // UI spec form (ADR 0039 P4): modal discipline like the
    // provider form, but field-local — Esc cancels, Enter
    // validates and advances, Backspace edits, printable chars
    // append bounded. Completion is drained by the loop.
    if let Some(form) = state.ui_spec_form.as_mut() {
        if form.completed.is_some() {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                state.ui_spec_form = None;
                return false;
            }
            KeyCode::Enter => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                super::forms::advance_ui_spec_form(form);
                return false;
            }
            KeyCode::Backspace => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                form.input.pop();
                form.error = None;
                return false;
            }
            KeyCode::Char(ch) => {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    return false;
                }
                if ch.is_control() {
                    return false;
                }
                if form.input.len() + ch.len_utf8() <= MAX_INPUT_BYTES {
                    form.input.push(ch);
                }
                form.error = None;
                return false;
            }
            _ => {
                return false;
            }
        }
    }
    // H6: provider picker intercepts all keys while visible.
    if state.provider_picker.is_some() {
        match key.code {
            KeyCode::Esc => {
                state.provider_picker = None;
                state.input.clear();
                state.update_palette();
                return false;
            }
            KeyCode::Up => {
                if let Some(picker) = state.provider_picker.as_mut() {
                    picker.select_prev();
                }
                return false;
            }
            KeyCode::Down => {
                if let Some(picker) = state.provider_picker.as_mut() {
                    picker.select_next();
                }
                return false;
            }
            KeyCode::Enter => {
                let selected_name = state
                    .provider_picker
                    .as_ref()
                    .and_then(|p| p.selected_entry())
                    .map(|e| e.name.clone());
                // C1: "add" entry in the picker opens the add-flow form.
                let selected_is_add = selected_name
                    .as_deref()
                    .is_some_and(|n| n == "+ Add provider");
                if selected_is_add {
                    state.provider_picker = None;
                    state.input.clear();
                    state.update_palette();
                    state.provider_add_form = Some(ProviderAddForm::new());
                    return false;
                }
                // Removal entry: arm the y/N confirmation modal.
                let selected_is_remove = selected_name
                    .as_deref()
                    .is_some_and(|n| n == "- Remove provider");
                if selected_is_remove {
                    open_provider_remove_confirm(state);
                    return false;
                }
                if let Some(name) = selected_name {
                    let sanitized =
                        crate::sanitize::sanitize_for_display(&name);
                    state.push_line(format!("provider: {sanitized} selected"));
                }
                state.provider_picker = None;
                state.input.clear();
                state.update_palette();
                return false;
            }
            _ => {
                // Consume all keys while picker is open (no typing through picker).
                return false;
            }
        }
    }
    // `/model` switch picker intercepts all keys while visible (same modal
    // discipline as the provider picker; navigation wraps through the
    // shared `ModelPicker` methods, decision 138).
    if state.model_switch_picker.is_some() {
        match key.code {
            KeyCode::Esc => {
                state.model_switch_picker = None;
                state.pending_model_switch = None;
                state.input.clear();
                state.update_palette();
                return false;
            }
            KeyCode::Up => {
                if let Some(picker) = state.model_switch_picker.as_mut() {
                    picker.select_prev_wrapping();
                }
                return false;
            }
            KeyCode::Down => {
                if let Some(picker) = state.model_switch_picker.as_mut() {
                    picker.select_next_wrapping();
                }
                return false;
            }
            KeyCode::Enter => {
                let selected = state
                    .model_switch_picker
                    .as_ref()
                    .and_then(|picker| picker.selected_id())
                    .map(str::to_owned);
                state.model_switch_picker = None;
                state.input.clear();
                state.update_palette();
                // Arm the resolved switch; the loop persists it through
                // the same path as `/model <id>`.
                state.pending_model_switch = selected;
                return false;
            }
            _ => {
                // Consume all keys while picker is open (no typing through picker).
                return false;
            }
        }
    }
    // UI panels view (ADR 0039): while active the transcript input
    // is inert — only the view keys act. `F2`/`Esc` exit, `[`/`]`
    // switch tabs (wrapping across the grid tab).
    if state.panels_view {
        if key.kind != crossterm::event::KeyEventKind::Press {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::F(2) => {
                state.panels_view = false;
                return false;
            }
            KeyCode::Char('[') => {
                super::panels::cycle_panels_tab(&mut state.panels, true);
                return false;
            }
            KeyCode::Char(']') => {
                super::panels::cycle_panels_tab(&mut state.panels, false);
                return false;
            }
            _ => return false,
        }
    }
    if key.kind != crossterm::event::KeyEventKind::Press {
        return false;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
            // Ctrl+C is handled by the outer loop as exit, not here.
            false
        }
        (KeyCode::Right, _) => {
            // S3b: expand the thinking block (no-op when nothing streamed).
            if !state.reasoning.trim().is_empty() {
                state.reasoning_expanded = true;
            }
            false
        }
        (KeyCode::Left, _) => {
            state.reasoning_expanded = false;
            false
        }
        (KeyCode::F(2), _) => {
            // UI panels view (ADR 0039): enter explicitly; the
            // transcript frame is untouched by everything panels do.
            // A fresh entry with no staged snapshot asks the loop
            // for one panel report.
            state.panels_view = true;
            if state.panels.is_none() {
                state.panels_refresh_pending = true;
            }
            false
        }
        (KeyCode::Esc, _) => {
            // Esc clears palette/picker context or input. Also clears selection/history.
            if state.palette.is_some() {
                state.input.clear();
                state.palette = None;
                state.palette_selected = None;
                state.update_palette();
            } else {
                // When palette is None, Esc clears history navigation state as well
                state.history_index = None;
                state.history_draft = None;
            }
            false
        }
        (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
            // C5 Tab + I2 Tab with selection: if a palette entry is selected, complete to it and clear palette.
            complete_palette_prefix(state);
            false
        }
        (KeyCode::Up, _) => {
            // I2: when palette is Some, Up navigates palette selection
            if let Some(catalog) = &state.palette {
                if !catalog.is_empty() {
                    // Palette navigation mode
                    let len = catalog.len();
                    state.palette_selected =
                        Some(match state.palette_selected {
                            None => len - 1,
                            Some(0) => len - 1,
                            Some(idx) => idx - 1,
                        });
                    return false;
                }
            }
            // Owner ruling 2026-09-12: mouse capture is OFF by default, so
            // the terminal turns the wheel into arrow keys in the alternate
            // screen. An EMPTY prompt with a scrollable transcript treats
            // plain Up/Down as wheel steps; history stays on Ctrl+Up/Down
            // and on any non-empty input.
            if state.palette.is_none()
                && state.input.is_empty()
                && !key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let max = state.max_scroll(viewport_height);
                if state.scroll_offset < max {
                    state.scroll_offset += 1;
                    return false;
                }
            }
            // I4: when palette is None, Up navigates history
            if state.palette.is_none() {
                if state.prompt_history.is_empty() {
                    return false;
                }
                if state.history_index.is_none() {
                    // Entering history navigation — save draft
                    state.history_draft = Some(state.input.clone());
                    let last = state.prompt_history.len() - 1;
                    state.history_index = Some(last);
                    state.input = state.prompt_history[last].clone();
                    state.update_palette();
                } else if let Some(idx) = state.history_index {
                    if idx > 0 {
                        let prev = idx - 1;
                        state.history_index = Some(prev);
                        state.input = state.prompt_history[prev].clone();
                        state.update_palette();
                    }
                    // at 0, stay
                }
            }
            false
        }
        (KeyCode::Down, _) => {
            // I2: when palette is Some, Down navigates palette selection
            if let Some(catalog) = &state.palette {
                if !catalog.is_empty() {
                    let len = catalog.len();
                    state.palette_selected =
                        Some(match state.palette_selected {
                            None => 0,
                            Some(idx) if idx + 1 >= len => 0,
                            Some(idx) => idx + 1,
                        });
                    return false;
                }
            }
            if state.palette.is_none()
                && state.input.is_empty()
                && !key.modifiers.contains(KeyModifiers::CONTROL)
                && state.scroll_offset > 0
            {
                state.scroll_offset -= 1;
                return false;
            }
            // I4: when palette is None, Down navigates history forward / restores draft
            if state.palette.is_none() {
                if let Some(idx) = state.history_index {
                    if idx + 1 < state.prompt_history.len() {
                        let next = idx + 1;
                        state.history_index = Some(next);
                        state.input = state.prompt_history[next].clone();
                        state.update_palette();
                    } else {
                        // Past newest — restore draft
                        state.history_index = None;
                        let draft =
                            state.history_draft.take().unwrap_or_default();
                        state.input = draft;
                        state.update_palette();
                    }
                }
            }
            false
        }
        (KeyCode::Enter, _) => {
            // I2: Enter on a selected palette entry fills input and clears palette (no submit)
            if let Some(selected) = state.palette_selected {
                if let Some(catalog) = &state.palette {
                    if let Some((name, _)) = catalog.get(selected) {
                        state.input = name.clone();
                        state.palette = None;
                        state.palette_selected = None;
                        return false;
                    }
                }
            }
            // I4: push non-slash prompts to history on submit (bounded, per-session)
            if !state.input.trim().is_empty() && !state.input.starts_with('/')
            {
                let to_push = state.input.clone();
                state.push_history(to_push);
            } else {
                // For slash commands, clear history navigation state but don't push
                state.history_index = None;
                state.history_draft = None;
            }
            true
        }
        (KeyCode::Backspace, _) => {
            state.input.pop();
            state.update_palette();
            false
        }
        (KeyCode::Char(ch), _) => {
            if state.input.len() + ch.len_utf8() <= MAX_INPUT_BYTES {
                state.input.push(ch);
            }
            state.update_palette();
            false
        }
        (KeyCode::PageUp, _) => {
            let max = state.max_scroll(viewport_height);
            state.scroll_offset =
                (state.scroll_offset.saturating_add(10)).min(max);
            false
        }
        (KeyCode::PageDown, _) => {
            state.scroll_offset = state.scroll_offset.saturating_sub(10);
            false
        }
        _ => false,
    }
}
