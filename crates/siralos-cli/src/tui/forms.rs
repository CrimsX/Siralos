//! Provider picker and provider-add form state, field validation,
//! and the credential-safe display helpers the forms render with.

use super::{Color, Line, Style};

/// One provider entry for the `/provider` picker (H6 — read-only, display-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEntry {
    /// Display name (provider id, sanitized).
    pub name: String,
    /// Base-url host (sanitized, e.g. `api.openai.com` or `—` when absent).
    pub host: String,
    /// Model (sanitized).
    pub model: String,
}

/// Read-only provider picker popup (H6) — listing configured providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPicker {
    /// Providers listed (from workspace config, display-only).
    pub entries: Vec<ProviderEntry>,
    /// Currently selected index.
    pub selected: usize,
}

impl ProviderPicker {
    /// Build a picker from entries; selected starts at 0.
    pub fn new(entries: Vec<ProviderEntry>) -> Self {
        Self { entries, selected: 0 }
    }

    /// Move selection up (saturating).
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down (clamped).
    pub fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    /// Currently selected entry, if any.
    pub fn selected_entry(&self) -> Option<&ProviderEntry> {
        self.entries.get(self.selected)
    }
}

/// Parse host from an endpoint URL (strip scheme, take up to `/`).
#[must_use]
pub fn host_from_endpoint(endpoint: &str) -> String {
    if !siralos_core::composition::is_valid_http_endpoint(endpoint) {
        return "—".to_owned();
    }
    let without_scheme =
        endpoint.split_once("://").map(|(_, rest)| rest).unwrap_or(endpoint);
    let host =
        without_scheme.split(['/', '?']).next().unwrap_or(without_scheme);
    if host.is_empty() {
        "—".to_owned()
    } else {
        crate::sanitize::sanitize_for_display(host)
    }
}

/// Build provider entries from the composed session's provider/model/endpoint (H6).
/// Single-provider config yields one entry; absent yields empty.
#[must_use]
pub fn provider_entries_from_session(
    provider: Option<&str>,
    model: Option<&str>,
    endpoint: Option<&str>,
) -> Vec<ProviderEntry> {
    if let Some(name) = provider {
        if !name.is_empty() {
            let host = endpoint
                .map(host_from_endpoint)
                .unwrap_or_else(|| "—".to_owned());
            let model_disp =
                model.map(safe_model_id).unwrap_or_else(|| "—".to_owned());
            return vec![ProviderEntry {
                name: safe_display_label(name),
                host,
                model: model_disp,
            }];
        }
    }
    Vec::new()
}

/// Sequential field for the provider add-flow form — six fields in user order (S1).
/// Order per O1/I1: DisplayName -> Url -> ApiKey -> ApiProtocol -> Model -> ModelDisplayName.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAddField {
    /// Display name (provider name, auto-derived from URL host) — first field (O1).
    DisplayName,
    /// URL endpoint (optional, https:// or http://).
    Url,
    /// API key env-var name (the env-var NAME, never the secret).
    ApiKey,
    /// API protocol (openai-completions, openai-responses, or anthropic-messages).
    ApiProtocol,
    /// Model id (picker on fetch success, free text otherwise).
    Model,
    /// Model display name (optional, shown in header/status instead of raw id).
    ModelDisplayName,
}

impl ProviderAddField {
    /// Title label for the current field — no examples in ANY label (S1).
    pub fn label(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::ApiKey => "api key",
            Self::DisplayName => "display name",
            Self::ApiProtocol => "api protocol",
            Self::Model => "model",
            Self::ModelDisplayName => "model display name",
        }
    }

    /// Dim description line below the label — short and dim (S1).
    pub fn description(self) -> &'static str {
        match self {
            Self::Url => "the provider endpoint",
            Self::ApiKey => {
                "the environment variable holding your key; set it before starting Siralos"
            }
            Self::DisplayName => "the name shown for this provider",
            Self::ApiProtocol => {
                "openai-completions, openai-responses, or anthropic-messages (Up/Down to pick)"
            }
            Self::Model => "the model id",
            Self::ModelDisplayName => {
                "the name shown for this model (optional)"
            }
        }
    }
}

/// Model picker state — opened after successful fetch, part of the form modal (S2).
/// Also opened by bare `/model` as the live switch picker: the same struct
/// (decision 138), never a second picker.
#[derive(Clone, PartialEq, Eq)]
pub struct ModelPicker {
    /// Fetched model ids in server order.
    pub items: Vec<String>,
    /// Currently selected index (Up/Down wraps).
    pub selected: usize,
}

impl std::fmt::Debug for ModelPicker {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ModelPicker")
            .field("item_count", &self.items.len())
            .field("selected", &self.selected)
            .finish()
    }
}

impl ModelPicker {
    /// Move selection up with wrap (shared by the add-flow form keys and
    /// the `/model` switch picker — one definition).
    pub fn select_prev_wrapping(&mut self) {
        if self.items.is_empty() {
            return;
        }
        if self.selected == 0 {
            self.selected = self.items.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    /// Move selection down with wrap (shared by the add-flow form keys and
    /// the `/model` switch picker — one definition).
    pub fn select_next_wrapping(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.items.len();
    }

    /// Currently selected model id, if any.
    pub fn selected_id(&self) -> Option<&str> {
        self.items.get(self.selected).map(String::as_str)
    }
}

/// Sliding viewport window over model-picker items (decision 138): at most
/// 8 rows, end-anchored so the selection is always visible. Shared by the
/// add-flow form render and the `/model` switch picker render — one
/// definition, identical windows.
#[must_use]
pub fn model_picker_window(total: usize, selected: usize) -> (usize, usize) {
    const VISIBLE: usize = 8;
    let window_start = if total <= VISIBLE {
        0
    } else {
        selected.saturating_sub(VISIBLE - 1).min(total.saturating_sub(VISIBLE))
    };
    let window_end = (window_start + VISIBLE).min(total);
    (window_start, window_end)
}

/// Item lines for a model picker: position header, the windowed items with
/// the selection highlight, and the navigation hint. Shared by the
/// add-flow form render and the `/model` switch picker render — one
/// definition, byte-identical lines.
#[must_use]
pub fn model_picker_lines(picker: &ModelPicker) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let total = picker.items.len();
    let (window_start, window_end) =
        model_picker_window(total, picker.selected);
    lines.push(
        Line::from(format!("    {}/{} ", picker.selected + 1, total))
            .style(Style::default().fg(Color::DarkGray)),
    );
    for idx in window_start..window_end {
        let item = &picker.items[idx];
        let prefix = if idx == picker.selected { "> " } else { "  " };
        let style = if idx == picker.selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(
            Line::from(format!("    {prefix}{}", safe_model_id(item)))
                .style(style),
        );
    }
    lines.push(
        Line::from(
            "    Up/Down to navigate, Enter to select, Esc for free text",
        )
        .style(Style::default().fg(Color::DarkGray)),
    );
    lines
}

/// Protocol picker state — 3-item picker over the real wire protocols (decision 137/138).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolPicker {
    /// Protocol options in order: openai-completions, openai-responses, anthropic-messages.
    pub items: Vec<String>,
    /// Currently selected index (Up/Down wraps).
    pub selected: usize,
}

/// Completed add-flow data — the validated values to write as `[profile]` (S3/S4).
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderAddData {
    /// Provider id (display name) validated `[a-z0-9_-]{1,64}`.
    pub provider: String,
    /// Model id: 1 to 256 bytes, no NUL, letters/numbers or . _ - / : @.
    pub model: String,
    /// Optional credential env-var NAME (without `env:` prefix) validated `A-Z0-9_` up to 64.
    /// `None` means a public endpoint — no credential is written and the
    /// model fetch sends no Authorization header.
    pub credential_env: Option<String>,
    /// Optional endpoint validated `https://` or `http://`, no NUL/space, up to 4096.
    pub endpoint: Option<String>,
    /// Protocol — closed set, default openai-completions (S3).
    pub protocol: String,
    /// Optional model display name — printable, bounded 256 (S1).
    pub model_display_name: Option<String>,
}

/// Sequential add-provider form — six fields, fetching + picker integrated (S1/S2).
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderAddForm {
    /// Validated url (endpoint) — set after Url advance.
    pub endpoint: Option<String>,
    /// Validated api key env-var NAME — set after ApiKey advance.
    pub credential_env: Option<String>,
    /// Validated display name (provider) — set after DisplayName advance.
    pub provider: Option<String>,
    /// Validated api protocol — set after ApiProtocol advance.
    pub protocol: Option<String>,
    /// Validated model — set after Model advance (picker or free text).
    pub model: Option<String>,
    /// Validated model display name — set after ModelDisplayName advance.
    pub model_display_name: Option<String>,
    /// Current field being edited.
    pub field: ProviderAddField,
    /// Current field edit buffer (raw typing, not yet validated).
    pub input: String,
    /// Validation error to display (if last Enter was invalid).
    pub error: Option<String>,
    /// When Some, the form has been completed and these are the validated values
    /// to write atomically — consumed by the interactive loop.
    pub completed: Option<ProviderAddData>,
    /// While true, the interactive loop performs the blocking model fetch once.
    pub fetching_models: bool,
    /// Model picker state — Some after successful fetch, handled inside form keys.
    pub model_picker: Option<ModelPicker>,
    /// Honest fallback note when fetch fails — shown as a dim line in the form.
    pub fetch_note: Option<String>,
    /// Protocol picker state — Some when ApiProtocol field picker is open.
    pub protocol_picker: Option<ProtocolPicker>,
}

/// Live edit display for the endpoint field. The stored-value projection
/// ([`safe_form_endpoint_for_credential`]) discards the path and query, which
/// would hide the very text being typed, so the edit buffer is shown as
/// typed: control characters sanitized, userinfo masked, bounded to the
/// field's own limit.
pub(super) fn safe_form_endpoint_edit(
    value: &str,
    credential: Option<&str>,
) -> String {
    if credential.is_some() {
        return "[credential-bearing endpoint]".to_owned();
    }
    let sanitized = crate::sanitize::sanitize_for_display(value);
    let shown = siralos_core::language::truncate_utf8_bytes(
        &sanitized,
        siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES,
    );
    mask_userinfo(&shown)
}

/// Replace any `user:pass@` userinfo in the authority with `[REDACTED]@`, so
/// a pasted secret is never echoed to the terminal while it is being typed.
fn mask_userinfo(value: &str) -> String {
    let Some((scheme, rest)) = value.split_once("://") else {
        return value.to_owned();
    };
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    match authority.rsplit_once('@') {
        Some((_, host)) => format!("{scheme}://[REDACTED]@{host}{tail}"),
        None => value.to_owned(),
    }
}

pub(super) fn safe_form_endpoint_for_credential(
    value: &str,
    credential: Option<&str>,
) -> String {
    if credential.is_some() {
        "[credential-bearing endpoint]".to_owned()
    } else {
        safe_form_endpoint(value)
    }
}

fn safe_form_endpoint(value: &str) -> String {
    if siralos_core::composition::is_valid_http_endpoint(value) {
        siralos_adapters::provider::safe_endpoint_for_output(value)
    } else if value.contains("://")
        || value.contains('@')
        || value.contains('?')
        || value.contains('#')
        || value.to_ascii_lowercase().contains("key:")
    {
        "[invalid endpoint]".to_owned()
    } else {
        crate::sanitize::sanitize_for_display(value)
            .chars()
            .take(512)
            .collect()
    }
}

fn has_line_control(value: &str) -> bool {
    value.chars().any(|character| {
        matches!(character, '\n' | '\r' | '\t')
            || ((character as u32) >= 0x80 && character.is_control())
    })
}

pub(super) fn safe_form_message(value: &str) -> String {
    if has_line_control(value)
        || value.contains("key:")
        || value.contains('@')
        || value.contains("://")
    {
        "[provider detail redacted]".to_owned()
    } else {
        crate::sanitize::sanitize_for_display(value)
    }
}

/// Project an untrusted provider/model label into a bounded display value.
/// The terminal sanitizer removes control sequences, but it does not know
/// whether a label is shaped like a credential; do that check at the final
/// render boundary as well as at provider ingestion.
pub(super) fn safe_display_label(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if has_line_control(value)
        || lower.starts_with("sk-")
        || lower.starts_with("akia")
        || lower.contains("key:")
        || lower.contains("secret")
        || lower.contains("token")
    {
        return "[REDACTED]".to_owned();
    }
    crate::sanitize::sanitize_for_display(value).chars().take(256).collect()
}

pub(super) fn safe_model_id(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("akia")
        || lower.contains("key:")
        || lower.contains("token")
        || lower.contains("secret")
    {
        "[MODEL REDACTED]".to_owned()
    } else {
        safe_form_message(value).chars().take(256).collect()
    }
}

pub(super) fn safe_form_message_for_credential(
    value: &str,
    credential: Option<&str>,
) -> String {
    if credential.is_some() {
        "[provider detail redacted]".to_owned()
    } else {
        safe_form_message(value)
    }
}

pub(super) fn redacted_form_credential(value: Option<&str>) -> Option<String> {
    value.map(|value| {
        if value.starts_with("key:") {
            "key:***".to_owned()
        } else if value.starts_with("env:") {
            value.to_owned()
        } else {
            "[REDACTED]".to_owned()
        }
    })
}

/// Preserve explicit `key:VALUE` and `env:NAME` forms. A bare legacy
/// environment name follows the add-form help and is normalized to `env:`;
/// all other text is a literal key and is retained byte-for-byte.
pub(super) fn normalize_credential_input(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else if value.starts_with("env:") || value.starts_with("key:") {
        Some(value.to_owned())
    } else if siralos_core::composition::is_credential_env_name(value) {
        Some(format!("env:{value}"))
    } else {
        Some(format!("key:{value}"))
    }
}

impl std::fmt::Debug for ProviderAddData {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderAddData")
            .field(
                "provider",
                &safe_form_message_for_credential(
                    &self.provider,
                    self.credential_env.as_deref(),
                ),
            )
            .field(
                "model",
                &safe_form_message_for_credential(
                    &self.model,
                    self.credential_env.as_deref(),
                ),
            )
            .field(
                "credential_env",
                &redacted_form_credential(self.credential_env.as_deref()),
            )
            .field(
                "endpoint",
                &self.endpoint.as_deref().map(|value| {
                    safe_form_endpoint_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field("protocol", &self.protocol)
            .field(
                "model_display_name",
                &self.model_display_name.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .finish()
    }
}

impl std::fmt::Debug for ProviderAddForm {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let input = match self.field {
            ProviderAddField::ApiKey => "key:***".to_owned(),
            ProviderAddField::Url => safe_form_endpoint_for_credential(
                &self.input,
                self.credential_env.as_deref(),
            ),
            _ => safe_form_message_for_credential(
                &self.input,
                self.credential_env.as_deref(),
            ),
        };
        formatter
            .debug_struct("ProviderAddForm")
            .field(
                "endpoint",
                &self.endpoint.as_deref().map(|value| {
                    safe_form_endpoint_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field(
                "credential_env",
                &redacted_form_credential(self.credential_env.as_deref()),
            )
            .field(
                "provider",
                &self.provider.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field("protocol", &self.protocol)
            .field(
                "model",
                &self.model.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field(
                "model_display_name",
                &self.model_display_name.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field("field", &self.field)
            .field("input", &input)
            .field(
                "error",
                &self.error.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field("completed", &self.completed)
            .field("fetching_models", &self.fetching_models)
            .field(
                "model_picker_count",
                &self.model_picker.as_ref().map(|picker| picker.items.len()),
            )
            .field(
                "fetch_note",
                &self.fetch_note.as_deref().map(|value| {
                    safe_form_message_for_credential(
                        value,
                        self.credential_env.as_deref(),
                    )
                }),
            )
            .field("protocol_picker", &self.protocol_picker)
            .finish()
    }
}

impl Default for ProviderAddForm {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAddForm {
    /// Create a fresh form starting at the display name field (O1 order).
    pub fn new() -> Self {
        Self {
            endpoint: None,
            credential_env: None,
            provider: None,
            protocol: None,
            model: None,
            model_display_name: None,
            field: ProviderAddField::DisplayName,
            input: String::new(),
            error: None,
            completed: None,
            fetching_models: false,
            model_picker: None,
            fetch_note: None,
            protocol_picker: None,
        }
    }

    /// Apply the fetch result — called by the interactive loop once after the flag is set.
    pub fn apply_fetch_result(&mut self, result: Result<Vec<String>, String>) {
        self.fetching_models = false;
        match result {
            Ok(items) if !items.is_empty() => {
                self.model_picker = Some(ModelPicker { items, selected: 0 });
                self.fetch_note = None;
            }
            Ok(_) => {
                // Empty list — treat as failure with honest note.
                self.model_picker = None;
                self.fetch_note = Some(
                    "model list unavailable from this provider - enter the model manually"
                        .to_owned(),
                );
            }
            Err(_) => {
                self.model_picker = None;
                self.fetch_note = Some(
                    "model list unavailable from this provider - enter the model manually"
                        .to_owned(),
                );
            }
        }
    }
}

// T1 composition note (updated T4): `run_interactive_session` blocks on
// `BufRead::read_line`, which would starve the `crossterm::event::poll` pump,
// so the live TUI loop duplicates the dispatch calling the SAME underlying seam
// functions (sanitizer, `ensure_host`, command dispatch, `drain_events` with the
// `TuiSink`). T2 consolidated the approval surface (`evaluate_approval_input` /
// `ApprovalModal::new` / `modal_key_decision` + the `interactive.rs` shims);
// T3 consolidated the audit/pane gating (the single shared helper
// `interactive::context_audit_session` the stdio `/context` arm, the TUI
// `/context` arm, and the pane builder all call) and the pane snapshot
// builders (`build_context_pane`, `context_counters`, `context_ring_tail`,
// `format_tick_record_line`, `tool_activity_from_history`,
// `context_pane_lines`). T4 (decision 108) settles the rest — FINAL LEDGER:
// SHARED now: `interactive::compose_session` (the whole session-composition
// block — config gate, workspace root, tools, host rules, profile
// declare/compose, replay wiring, provider choice, module/context selection,
// context-system build, registry, lock verification, skills segment,
// projection config, application, hosts/manifests); `parse_slash_command`
// (the slash-command vocabulary) + `render_context_segment` /
// `render_tools_segment` + `dispatch_stdio_command` / `dispatch_tui_command`
// (the two thin per-frontend writers over the shared parse — one match, two
// sinks); `handle_key` (the live loop routes ALL non-modal keys through it,
// no inline duplicate); `flush_record_replay` (the decision 78 B2 exit
// flush). PERMANENT RESIDUAL (per-frontend by construction, recorded with
// the why): the stdio loop owns `reader`/`writer` generics; the TUI loop
// owns the `TerminalGuard`/`Terminal`/`TuiState`/`TuiSink` terminal state;
// Ctrl+C-exit, the PageUp viewport lookup (`terminal.size`), and the modal
// verdict lines stay in the TUI loop because they need live terminal state.
// No forced unification beyond this — the residual is the shape, not debt.
// During a blocking provider round the UI simply does not redraw — the status
// line showed "working" before the step and the freeze is documented.
/// Derive provider name from an endpoint URL per R2 rules (pure, unit-testable):
/// take the host (strip the scheme), strip a leading "api." prefix, take the
/// first dot-separated label, lowercase, replace every character outside
/// [a-z0-9_-] with '-', truncate to 64. Empty URL -> empty.
#[must_use]
pub fn derive_provider_name(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let without_scheme = if let Some(rest) = trimmed.strip_prefix("https://") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        rest
    } else {
        trimmed
    };
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        return String::new();
    }
    let lower = host.to_ascii_lowercase();
    let stripped = if let Some(rest) = lower.strip_prefix("api.") {
        rest
    } else {
        lower.as_str()
    };
    let label = stripped.split('.').next().unwrap_or(stripped);
    if label.is_empty() {
        return String::new();
    }
    let mut out: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '-'
                || c == '_'
            {
                c
            } else {
                '-'
            }
        })
        .collect();
    if out.len() > 64 {
        out.truncate(64);
    }
    out
}

/// Validate provider field: [a-z0-9_-]{1,64}, non-empty, no NUL.
/// Human-readable error (D2) — validation rule unchanged.
pub(super) fn validate_provider_name(value: &str) -> Result<(), String> {
    if !siralos_core::composition::is_provider_id(value) {
        return Err(
            "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)"
                .to_owned(),
        );
    }
    Ok(())
}

/// Validate api protocol — closed set: openai-completions (default), openai-responses, anthropic-messages.
pub(super) fn validate_api_protocol(value: &str) -> Result<(), String> {
    if value == "openai-completions"
        || value == "openai-responses"
        || value == "anthropic-messages"
    {
        return Ok(());
    }
    Err("The api protocol must be \"openai-completions\", \"openai-responses\", or \"anthropic-messages\"."
        .to_owned())
}

fn protocol_picker_items() -> Vec<String> {
    vec![
        "openai-completions".to_owned(),
        "openai-responses".to_owned(),
        "anthropic-messages".to_owned(),
    ]
}

pub(super) fn open_protocol_picker(form: &mut ProviderAddForm) {
    let items = protocol_picker_items();
    let selected = form
        .protocol
        .as_deref()
        .and_then(|p| items.iter().position(|x| x == p))
        .unwrap_or(0);
    form.protocol_picker = Some(ProtocolPicker { items, selected });
    form.input.clear();
    form.error = None;
}

/// Validate model display name — optional, printable, bounded 256 (S1/I1).
pub(super) fn validate_model_display_name(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len()
        > siralos_core::composition::MAX_PROFILE_MODEL_DISPLAY_NAME_BYTES
    {
        return Err(
            "The model display name exceeds the 256-byte bound.".to_owned()
        );
    }
    if value.contains('\0') {
        return Err("A model display name must not contain NUL.".to_owned());
    }
    if !siralos_core::composition::is_printable(value) {
        return Err("A model display name must be printable.".to_owned());
    }
    Ok(())
}

/// Validate model field: 1 to 256 bytes, no NUL, ASCII alphanumeric or
/// `.` `_` `-` `/` `:` `@` (the core `is_model_id` rule).
/// Human-readable error (D2) — the message states the enforced rule in
/// plain words, never a regex.
pub(super) fn validate_model_name(value: &str) -> Result<(), String> {
    if !siralos_core::composition::is_model_id(value) {
        return Err(
            "Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"
                .to_owned(),
        );
    }
    Ok(())
}

/// Validate endpoint field: https:// or http://, no NUL, no space, 1..4096.
/// Human-readable error (D2) — validation rule unchanged.
pub(super) fn validate_endpoint_value(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES
    {
        return Err(
            "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)"
                .to_owned(),
        );
    }
    if value.contains('\0') {
        return Err(
            "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)"
                .to_owned(),
        );
    }
    if !siralos_core::composition::has_http_scheme(value) {
        return Err(
            "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)"
                .to_owned(),
        );
    }
    if value.contains(' ') {
        return Err(
            "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)"
                .to_owned(),
        );
    }
    if !siralos_core::composition::is_valid_http_endpoint(value) {
        return Err(
            "Endpoint must have a valid HTTP(S) authority without userinfo, fragment, or controls."
                .to_owned(),
        );
    }
    Ok(())
}
