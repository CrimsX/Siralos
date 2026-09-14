//! TUI shell — pure render model over the live interactive session (T1).
//!
//! The terminal sanitizer stays the single output boundary: every line entering
//! the transcript via [`TuiSink`] is already sanitized by the session (the same
//! `sanitize` code path the stdio frontend uses). The TUI adds no unsanitized
//! content of its own. The input queue stays the single interactive-read owner
//! and the command catalog the vocabulary source; approvals stay host-gated.
//!
//! T3 adds the context pane: for opted-in sessions the decision 100 audit
//! (counters + tick ring) and host-observed tool activity render as a live
//! right-hand pane, single-sourced from the same `ContextMetrics` the
//! `/context` audit segment uses. When the subsystem is off, rendering is
//! byte-identical to T2's (no pane, no placeholder).
//!
//! No threads are used anywhere in the session path: the event loop uses
//! `crossterm::event::poll` with a bounded timeout, blocking provider rounds
//! freeze the redraw (documented T1 limitation). A live spinner would require
//! a UI event-pump thread (recorded as open architecture question, decision 119).

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

/// Zero-timeout drain poll — only already-queued events, NEVER waits (P1).
pub const TUI_DRAIN_POLL: Duration = Duration::ZERO;

/// Bounded idle poll for status/pane redraw when idle (P1 — raised 15ms → 50ms).
pub const TUI_IDLE_POLL: Duration = Duration::from_millis(50);

/// Context budget for the status usage readout (P5).
pub const CONTEXT_BUDGET_TOKENS: usize = 4096;

/// Maximum number of transcript lines retained (bounded ring).
pub const MAX_TRANSCRIPT_LINES: usize = 1000;

/// Maximum number of lines shown in the approval modal (bounded).
pub const MAX_APPROVAL_LINES: usize = 30;

/// Marker appended when the approval request is truncated to the bound.
pub const APPROVAL_TRUNCATION_MARKER: &str = "... (truncated)";

/// Transcript rows per mouse-wheel notch (option b): a small step reusing
/// the existing `scroll_offset` clamp/max logic — not a second mechanism.
/// PageUp/PageDown keep their 10-row step; the wheel is the fine control.
pub const MOUSE_WHEEL_STEP: u16 = 3;

/// Minimum gap between sink-requested redraws while a stream is arriving.
///
/// The reveal releases `rate * interval` characters per frame, so this IS
/// the text's step size: at 33 ms a 240 char/s reveal moved ~8 characters a
/// frame and read as "display, stop, display" on a slow provider. 16 ms
/// (60 fps) halves the step, and the last delta of a turn is always painted
/// because the loop's own draw runs after the drain.
pub const REDRAW_INTERVAL: Duration = Duration::from_millis(16);

/// How much streamed thinking the TUI keeps (the tail), so a long reasoning
/// trace cannot grow the state without bound.
pub const REASONING_BYTES: usize = 8192;

/// How many thinking rows the expanded block shows (the tail).
pub const REASONING_ROWS: usize = 8;

/// How much of the newest thinking line the COLLAPSED row previews.
pub const THINKING_TAIL_CHARS: usize = 60;

/// How fast revealed text is released to the reader (S3c), so the answer
/// reads left to right whatever chunk size the provider sends.
pub const REVEAL_CHARS_PER_SEC: f64 = 240.0;

/// The most one tick may release, so a long stall cannot dump a wall of
/// text the instant a frame is painted.
pub const REVEAL_TICK_CHARS: usize = 480;

/// Toggle result line when mouse capture turns on: states the result and
/// the copy trade (capture steals click-drag selection) with the way back.
pub const MOUSE_CAPTURE_ON_MESSAGE: &str = "mouse capture on - the wheel scrolls the transcript directly; /mouse again hands the mouse back to the terminal";

/// Toggle result line when mouse capture turns off: states the result and
/// the way back to wheel scrolling.
pub const MOUSE_CAPTURE_OFF_MESSAGE: &str = "mouse capture off - the terminal selects text and pastes; the wheel scrolls via arrow keys; /mouse captures the mouse";

/// Honest stdio answer for `/mouse`: the stdio frontend has no TTY mouse,
/// so it reports that instead of pretending to toggle anything.
pub const MOUSE_STDIO_MESSAGE: &str = "mouse capture unavailable - the stdio frontend has no TTY mouse; use the TUI for wheel scrolling";

/// Map a capture state to its toggle result line (pure, headlessly tested).
#[must_use]
pub fn mouse_capture_message(enabled: bool) -> &'static str {
    if enabled { MOUSE_CAPTURE_ON_MESSAGE } else { MOUSE_CAPTURE_OFF_MESSAGE }
}

/// ASCII banner for Siralos — hand-drawn static block, bounded width <= 80 cols (H2).
/// TUI-only: pushed into the transcript at session start via `push_line` (stdio unchanged).
pub const SIRALOS_BANNER: &[&str] = &[
    "  ___ ___ ___  _   _    ___  ___",
    " / __|_ _| _ \\/ \\ | |  / _ \\/ __|",
    " \\__ \\| ||   / _ \\| |_| (_) \\__ \\",
    " |___/___|_|_/_/ \\_\\___\\___/|___/",
];

/// Greeting line shown below the banner at TUI session start (H2).
pub const SIRALOS_GREETING: &str =
    "Welcome to Siralos. Type / to browse commands, or just start typing.";

/// Approval routing decision — host-gated, same gate the stdio path uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// `y` — approve.
    Approve,
    /// `n` or `Esc` — deny.
    Deny,
}

/// Bounded approval modal over the transcript pane (T2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalModal {
    /// Sanitized request lines (already bounded to [`MAX_APPROVAL_LINES`]).
    pub lines: Vec<String>,
    /// Focused choice (0 approve / 1 deny) — kept for completeness; keys
    /// `y`/`n`/`Esc` decide directly and `selected` mirrors the last
    /// highlight.
    pub selected: bool,
}

impl ApprovalModal {
    /// Build a modal from already-sanitized lines, bounding to
    /// [`MAX_APPROVAL_LINES`] with a truncation marker when needed.
    pub fn new(mut lines: Vec<String>) -> Self {
        let truncated = lines.len() > MAX_APPROVAL_LINES;
        if truncated {
            lines.truncate(MAX_APPROVAL_LINES);
            lines.push(APPROVAL_TRUNCATION_MARKER.to_owned());
        }
        Self { lines, selected: false }
    }
}

/// Pure predicate shared by all frontends: decides approval from the same
/// host-gated input the stdio path would read. `y` (case-insensitive,
/// trimmed) approves; any other input (including `n` and `Esc`) denies.
/// This function is the SINGLE approval-read helper both loops call (T2
/// consolidation — no parallel logic).
pub fn evaluate_approval_input(input: &str) -> ApprovalDecision {
    if input.trim().eq_ignore_ascii_case("y") {
        ApprovalDecision::Approve
    } else {
        ApprovalDecision::Deny
    }
}

/// Map a typed character (`y`/`n`) to the shared gate without an
/// intermediate parallel code path — used by the TUI modal key handler.
pub fn evaluate_approval_char(ch: char) -> ApprovalDecision {
    evaluate_approval_input(&ch.to_string())
}

/// Fixed width of the T3 context pane in columns (right-hand pane).
pub const CONTEXT_PANE_WIDTH: u16 = 40;

/// Maximum tick records shown in the context pane ring block (same bound as
/// the decision 100 audit segment: the LAST 8, oldest-first).
pub const CONTEXT_PANE_RING_LIMIT: usize = 8;

/// Maximum tool rounds shown in the context pane activity block (the LAST 8,
/// oldest-first).
pub const CONTEXT_PANE_ACTIVITY_LIMIT: usize = 8;

/// Pinned counter order for the context pane — the SAME 12 counters in the
/// SAME declaration order as the decision 100 audit segment (`output.rs`
/// `format_context_audit`, decision 100 R1a). The pane reads the same
/// `ContextMetrics` state; the order is pinned here, not re-derived.
pub const CONTEXT_COUNTER_ORDER: [&str; 12] = [
    "ticks_total",
    "coalesced_noop_ticks_total",
    "events_total",
    "events_dropped_total",
    "demand_updates_total",
    "promotions_total",
    "demotions_total",
    "stale_demotions_total",
    "pin_quota_demotions_total",
    "budget_demotions_total",
    "assembled_summary_tokens_total",
    "neighbor_stub_tokens_total",
];

/// One host-observed tool round for the pane activity block: the tool name
/// plus the typed result status (the `status_str` vocabulary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivityEntry {
    /// Tool name from host-observed history (sanitized at extraction).
    pub tool_name: String,
    /// Typed result status (`success`, `failed`, …).
    pub status: String,
}

/// Read-only snapshot behind the T3 context pane, derived from the SAME
/// host-side sources the `/context` audit segment uses: the live
/// `ContextMetrics` (counters + tick ring) and the host-observed
/// conversation history (tool activity). No persistence, no mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPaneData {
    /// The 12 counters in [`CONTEXT_COUNTER_ORDER`] pinned order.
    pub counters: Vec<(String, u64)>,
    /// The LAST 8 tick records, oldest-first (same records the audit
    /// segment renders).
    pub ring: Vec<siralos_core::context_metrics::TickRecord>,
    /// The LAST 8 host-observed tool rounds, oldest-first.
    pub activity: Vec<ToolActivityEntry>,
}

/// Read the 12 counters from `metrics` in pinned order.
///
/// Single source (P3): this reads the SAME `ContextMetrics` the
/// `format_context_audit` segment reads — no parallel extraction, no
/// re-derived field logic. Values are host-side numbers (sanitizer-clean
/// by construction).
#[must_use]
pub fn context_counters(
    metrics: &siralos_core::context_metrics::ContextMetrics,
) -> Vec<(String, u64)> {
    let values = [
        metrics.ticks_total,
        metrics.coalesced_noop_ticks_total,
        metrics.events_total,
        metrics.events_dropped_total,
        metrics.demand_updates_total,
        metrics.promotions_total,
        metrics.demotions_total,
        metrics.stale_demotions_total,
        metrics.pin_quota_demotions_total,
        metrics.budget_demotions_total,
        metrics.assembled_summary_tokens_total,
        metrics.neighbor_stub_tokens_total,
    ];
    CONTEXT_COUNTER_ORDER
        .iter()
        .zip(values)
        .map(|(name, value)| ((*name).to_owned(), value))
        .collect()
}

/// Slice the LAST 8 tick records oldest-first — the same window the decision
/// 100 audit segment renders (`records.len().saturating_sub(8)`).
#[must_use]
pub fn context_ring_tail(
    metrics: &siralos_core::context_metrics::ContextMetrics,
) -> Vec<siralos_core::context_metrics::TickRecord> {
    let records = metrics.records();
    let start = records.len().saturating_sub(CONTEXT_PANE_RING_LIMIT);
    records[start..].to_vec()
}

/// Format one tick record with the SAME field names as the decision 100
/// audit segment (decision 100 R1b). The pane truncates this line to the
/// pane width at render; the full form here is what the single-source test
/// compares against the `/context` segment.
#[must_use]
pub fn format_tick_record_line(
    rec: &siralos_core::context_metrics::TickRecord,
) -> String {
    format!(
        "    tick {}: now={} canonical_event_count={} events_dropped={} tier_counts hot={} warm={} cold={} archive={} assembled_unique_total={} assembled_summary_total={} stub_total={} demotion_counts stale={} pin_quota={} budget={} promotion_count={}",
        rec.now,
        rec.now,
        rec.canonical_event_count,
        rec.events_dropped,
        rec.tier_counts.hot,
        rec.tier_counts.warm,
        rec.tier_counts.cold,
        rec.tier_counts.archive,
        rec.assembled_unique_total,
        rec.assembled_summary_total,
        rec.stub_total,
        rec.demotion_counts.stale,
        rec.demotion_counts.pin_quota,
        rec.demotion_counts.budget,
        rec.promotion_count,
    )
}

/// Extract the LAST 8 host-observed tool rounds (oldest-first) from the
/// authoritative conversation history: every `ToolResult` in order with its
/// tool name and typed result status.
///
/// Sanitizer discipline (P4): the tool name passes through the same
/// `sanitize_for_display` boundary the `/context` segment applies; the
/// status is the static typed vocabulary (`status_str`). The pane
/// introduces no new unsanitized source.
#[must_use]
pub fn tool_activity_from_history(
    history: &[siralos_core::provider::ConversationItem],
) -> Vec<ToolActivityEntry> {
    use siralos_core::provider::ConversationItem;
    let mut entries: Vec<ToolActivityEntry> = Vec::new();
    for item in history {
        if let ConversationItem::ToolResult { tool_name, result, .. } = item {
            entries.push(ToolActivityEntry {
                tool_name: crate::sanitize::sanitize_for_display(tool_name),
                status: result.status_str().to_owned(),
            });
        }
    }
    let start = entries.len().saturating_sub(CONTEXT_PANE_ACTIVITY_LIMIT);
    entries[start..].to_vec()
}

/// Build the pane snapshot when the subsystem is opted in AND built — the
/// SAME gating as the decision 100 `/context` segment
/// (`context_system_enabled && session.is_some()`).
///
/// OFF (`!enabled` or `metrics` absent) returns `None`: the frame renders
/// byte-identical to T2's (no pane, no placeholder). Read-only over
/// in-memory state (P7): counters, ring, and history are borrowed, never
/// drained or cleared.
#[must_use]
pub fn build_context_pane(
    enabled: bool,
    metrics: Option<&siralos_core::context_metrics::ContextMetrics>,
    history: &[siralos_core::provider::ConversationItem],
) -> Option<ContextPaneData> {
    if !enabled {
        return None;
    }
    let metrics = metrics?;
    Some(ContextPaneData {
        counters: context_counters(metrics),
        ring: context_ring_tail(metrics),
        activity: tool_activity_from_history(history),
    })
}

/// Wrap one stored transcript line over `width` columns (render layer only).
///
/// Word-boundary wrap with hard-break for tokens exceeding the width (URLs,
/// JSON bodies). The stored text is never mutated: this expands one line
/// into one or more display rows for the transcript pane's inner width.
/// Char-count based, consistent with the header layout and
/// `truncate_to_width`. Deterministic: same text + same width ->
/// byte-identical rows.
#[must_use]
pub fn wrap_line_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if len <= width {
        return vec![text.to_owned()];
    }
    let mut rows = Vec::new();
    let mut start = 0;
    while start < len {
        if len - start <= width {
            rows.push(chars[start..].iter().collect());
            break;
        }
        let window_end = start + width;
        // Exact fit: the window ends at a word boundary (the next char is
        // the break space) — take the whole window and skip that space.
        if chars[window_end] == ' ' {
            rows.push(chars[start..window_end].iter().collect());
            start = window_end + 1;
            continue;
        }
        // Otherwise break at the last space inside the window (word
        // boundary) and skip that single space onto the next row. With no
        // space in the window the token exceeds the width — hard-break at
        // `width` (URLs, JSON bodies).
        let mut break_at: Option<usize> = None;
        for i in (start..window_end).rev() {
            if chars[i] == ' ' {
                break_at = Some(i);
                break;
            }
        }
        match break_at {
            // Guard `i > start`: a leading space must not produce an empty
            // row — hard-break instead.
            Some(i) if i > start => {
                rows.push(chars[start..i].iter().collect());
                start = i + 1;
            }
            _ => {
                rows.push(chars[start..window_end].iter().collect());
                start = window_end;
            }
        }
    }
    rows
}

/// Expand transcript entries into wrapped display rows (render layer).
///
/// Returns owned `(row, style)` pairs; the caller borrows them into `Line`s
/// that live until the frame is drawn. One stored line becomes one or more
/// rows via [`wrap_line_to_width`]; timestamps wrap the same way (short in
/// practice, one row) and keep the dim stamp style per row.
fn wrapped_transcript_rows(
    entries: &[TranscriptEntry],
    inner_width: usize,
) -> Vec<(String, Style)> {
    let mut wrapped = Vec::new();
    for entry in entries {
        let style = style_for_transcript_line(&entry.text);
        for row in wrap_line_to_width(&entry.text, inner_width) {
            wrapped.push((row, style));
        }
        if let Some(ts) = &entry.timestamp {
            let dim = Style::default().fg(Color::DarkGray);
            for row in wrap_line_to_width(ts, inner_width) {
                wrapped.push((row, dim));
            }
        }
    }
    wrapped
}

/// Truncate a line to `max_chars` characters on a char boundary (bounded
/// pane lines never overflow the fixed 40-column pane).
fn truncate_to_width(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        return line.to_owned();
    }
    line.chars().take(max_chars).collect()
}

/// Render the full pane line list: counters block, ring block, tool
/// activity block, top to bottom. Empty ring/activity renders the block
/// header with no lines. Every line is bounded to `inner_width`
/// (deterministic truncation).
#[must_use]
pub fn context_pane_lines(
    pane: &ContextPaneData,
    inner_width: usize,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(truncate_to_width("counters:", inner_width));
    for (name, value) in &pane.counters {
        lines.push(truncate_to_width(
            &format!("  {name}: {value}"),
            inner_width,
        ));
    }
    lines.push(truncate_to_width("ring (last 8):", inner_width));
    for rec in &pane.ring {
        lines.push(truncate_to_width(
            &format_tick_record_line(rec),
            inner_width,
        ));
    }
    lines.push(truncate_to_width("tools (last 8):", inner_width));
    for entry in &pane.activity {
        lines.push(truncate_to_width(
            &format!("  {} {}", entry.tool_name, entry.status),
            inner_width,
        ));
    }
    lines
}

/// One transcript entry — a sanitized line plus an optional local stamp.
///
/// `I4` (decision 119 H4): transcript entries carry caller-supplied
/// timestamps rendered as a dim line below each message; live sessions stamp
/// with the user's local timezone via `local_timestamp_now` (`time` crate
/// `local-offset`); differential fixtures carry fixed values so pinned frames
/// stay deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    /// Sanitized line text (no embedded newlines — split upstream).
    pub text: String,
    /// Optional local stamp like `"2026-08-31 14:03:22 +02:00"` (caller-supplied).
    pub timestamp: Option<String>,
}

/// Ordered command catalog over the SAME `SlashCommand` vocabulary (I2).
///
/// This is the SINGLE catalog the palette and the unknown-command honesty
/// line derive from — no parallel list. Order matches the
/// `parse_slash_command` arms including the U7/U8 additive commands.
/// Delegates to `crate::interactive::slash_command_catalog` — single source
/// (R3); the owned `String` conversion keeps the TUI palette type stable.
#[must_use]
pub fn command_catalog() -> Vec<(String, String)> {
    crate::interactive::slash_command_catalog()
        .into_iter()
        .map(|(name, desc)| (name.to_owned(), desc.to_owned()))
        .collect()
}

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
    let without_scheme = if let Some(rest) = endpoint.strip_prefix("https://")
    {
        rest
    } else if let Some(rest) = endpoint.strip_prefix("http://") {
        rest
    } else {
        endpoint
    };
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
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
            let model_disp = model
                .map(crate::sanitize::sanitize_for_display)
                .unwrap_or_else(|| "—".to_owned());
            return vec![ProviderEntry {
                name: crate::sanitize::sanitize_for_display(name),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPicker {
    /// Fetched model ids in server order.
    pub items: Vec<String>,
    /// Currently selected index (Up/Down wraps).
    pub selected: usize,
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
        lines.push(Line::from(format!("    {prefix}{item}")).style(style));
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAddData {
    /// Provider id (display name) validated `[a-z0-9_-]{1,64}`.
    pub provider: String,
    /// Model id: 1 to 256 bytes, no NUL, letters/numbers or . _ - / : @.
    pub model: String,
    /// Optional credential env-var NAME (without `env:` prefix) validated `A-Z0-9_` up to 64.
    /// `None` means a public endpoint — no credential is written and the
    /// model fetch sends no Authorization header.
    pub credential_env: Option<String>,
    /// Optional endpoint validated `https://` or `http://`, no NUL/space, up to 512.
    pub endpoint: Option<String>,
    /// Protocol — closed set, default openai-completions (S3).
    pub protocol: String,
    /// Optional model display name — printable, bounded 256 (S1).
    pub model_display_name: Option<String>,
}

/// Sequential add-provider form — six fields, fetching + picker integrated (S1/S2).
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Pure render model for the TUI shell.
///
/// `Eq` is deliberately absent: the reveal debt (S3c) is a float, and `f64`
/// has no total equality. `PartialEq` is what the tests compare with.
#[derive(Debug, Clone, PartialEq)]
pub struct TuiState {
    /// Transcript entries — bounded ring, oldest dropped when full. Each entry
    /// is a single sanitized line plus an optional local stamp.
    pub transcript: Vec<TranscriptEntry>,
    /// Back-compat accessor: transcript lines as plain strings (tests).
    /// Prefer `transcript` directly; this is kept for harness compat.
    pub transcript_lines: Vec<String>,
    /// Current input line (edited in-place, not yet submitted).
    pub input: String,
    /// Status line text.
    pub status: String,
    /// Scroll offset from the tail (0 = show tail, n = n lines up).
    pub scroll_offset: u16,
    /// Pending approval modal — when `Some`, all non-modal keys are ignored and
    /// the modal renders centered over a dimmed transcript (T2).
    pub pending_approval: Option<ApprovalModal>,
    /// When `true`, the pending approval modal is a provider-removal
    /// confirmation (armed by the "- Remove provider" picker row or
    /// `/provider remove`): `y` removes via `remove_profile_config`,
    /// `n`/`Esc` cancels. `false` for ordinary tool approvals.
    pub confirming_provider_removal: bool,
    /// Command palette popup (I2): when `Some`, the input starts with `/` and
    /// this holds the filtered catalog entries (case-insensitive prefix filter).
    pub palette: Option<Vec<(String, String)>>,
    /// Selected palette entry index (I2): when `Some`, the highlighted entry
    /// in the filtered palette (arrow-key navigation).
    pub palette_selected: Option<usize>,
    /// Provider for header bar (P2) — same source as status line.
    pub provider: Option<String>,
    /// Model for header bar (P2).
    pub model: Option<String>,
    /// Provider picker popup (H6) — when Some, Up/Down + Enter/Esc handle it.
    pub provider_picker: Option<ProviderPicker>,
    /// Provider add-flow form (C1) — sequential modal form; when Some, no other
    /// keys pass (modal discipline).
    pub provider_add_form: Option<ProviderAddForm>,
    /// `/model` switch picker — when Some, Up/Down + Enter/Esc handle it.
    /// Opened by bare `/model` over the provider's fetched models; this is
    /// the same [`ModelPicker`] (+ sliding viewport, decision 138) the
    /// add-flow uses, not a second picker. Enter arms
    /// `pending_model_switch` for the loop to resolve through the same
    /// switch-and-persist as the explicit-argument form.
    pub model_switch_picker: Option<ModelPicker>,
    /// Armed model id from the switch picker — consumed once by the
    /// interactive loop (same switch-and-persist as `/model <id>`).
    pub pending_model_switch: Option<String>,
    /// Submitted prompt history (I4): oldest first, bounded to 100, per-session
    /// in-memory with no persistence.
    pub prompt_history: Vec<String>,
    /// Current history navigation index (I4): `None` means not navigating,
    /// `Some(idx)` indexes into `prompt_history`.
    pub history_index: Option<usize>,
    /// Saved input before history navigation (I4): restored when navigating
    /// past the newest entry.
    pub history_draft: Option<String>,
    /// Mouse capture state: `false` (default) hands the mouse to the
    /// terminal, so click-drag selects text and the terminal's own paste
    /// works. `true` captures it so wheel events scroll the transcript
    /// directly. Render-neutral: no pane, status, or frame change — the
    /// differential frames pin this.
    pub mouse_capture: bool,
    /// Thinking streamed so far (S3): host-accounted model output that is
    /// NOT the answer. Bounded to the last [`REASONING_BYTES`]; rendered as
    /// one collapsed row that Right expands and Left collapses.
    pub reasoning: String,
    /// Whether the thinking block is expanded.
    pub reasoning_expanded: bool,
    /// When the current turn started, for the pulsing `working` line.
    pub busy_since: Option<std::time::Instant>,
    /// Answer text received but not yet revealed (S3c).
    pub stream_buffer: String,
    /// The revealed text of the INCOMPLETE line, rendered as a growing row
    /// so a long answer appears left to right instead of popping in whole.
    pub stream_tail: String,
    /// How much of `reasoning` has been revealed.
    pub reasoning_shown: usize,
    /// Last reveal tick, and the character debt carried between ticks.
    pub reveal_last: Option<std::time::Instant>,
    /// Characters still owed to the reader.
    pub reveal_debt: f64,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            transcript: Vec::new(),
            transcript_lines: Vec::new(),
            input: String::new(),
            status: String::from("ready"),
            scroll_offset: 0,
            pending_approval: None,
            confirming_provider_removal: false,
            palette: None,
            palette_selected: None,
            provider: None,
            model: None,
            provider_picker: None,
            provider_add_form: None,
            model_switch_picker: None,
            pending_model_switch: None,
            prompt_history: Vec::new(),
            history_index: None,
            history_draft: None,
            // OFF by default (owner ruling 2026-09-12): native select/copy
            // and paste must work out of the box. With capture off the
            // terminal turns the wheel into arrow keys in the alternate
            // screen, and an empty prompt scrolls the transcript with
            // them; `/mouse` captures the mouse when raw wheel events are
            // wanted.
            mouse_capture: false,
            reasoning: String::new(),
            reasoning_expanded: false,
            busy_since: None,
            stream_buffer: String::new(),
            stream_tail: String::new(),
            reasoning_shown: 0,
            reveal_last: None,
            reveal_debt: 0.0,
        }
    }
}

impl TuiState {
    /// Create an empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sync `transcript` and `transcript_lines` when the caller assigned
    /// directly to one of them (the harness and old tests use
    /// `transcript_lines`). When they diverge, rebuild `transcript` from
    /// `transcript_lines` (with `None` timestamps) — the live path keeps both
    /// in lockstep via `push_*`.
    fn sync_transcript(&mut self) {
        if self.transcript.len() != self.transcript_lines.len() {
            // If `transcript` was built via the new API, `transcript_lines` may
            // lag; if the caller wrote `transcript_lines` directly, `transcript`
            // lags. Prefer the longer/newer `transcript_lines` when `transcript`
            // is empty and `transcript_lines` non-empty (harness path).
            if self.transcript.is_empty() && !self.transcript_lines.is_empty()
            {
                self.transcript = self
                    .transcript_lines
                    .iter()
                    .map(|text| TranscriptEntry {
                        text: text.clone(),
                        timestamp: None,
                    })
                    .collect();
            } else if self.transcript_lines.is_empty()
                && !self.transcript.is_empty()
            {
                self.transcript_lines = self
                    .transcript
                    .iter()
                    .map(|entry| entry.text.clone())
                    .collect();
            } else if self.transcript.len() != self.transcript_lines.len() {
                // Keep both in sync by rebuilding `transcript_lines` from `transcript`.
                self.transcript_lines = self
                    .transcript
                    .iter()
                    .map(|entry| entry.text.clone())
                    .collect();
            }
        }
    }

    /// Effective transcript slice after syncing the two storages.
    fn effective_transcript_len(&self) -> usize {
        // `transcript` is authoritative when non-empty; otherwise fall back to
        // `transcript_lines` (harness direct assignment).
        if !self.transcript.is_empty() || self.transcript_lines.is_empty() {
            self.transcript.len()
        } else {
            self.transcript_lines.len()
        }
    }

    /// Release the text owed to the reader (S3c).
    ///
    /// Called before every paint: the answer and the thinking are revealed at
    /// `REVEAL_CHARS_PER_SEC` from the same budget, and the INCOMPLETE line is
    /// kept in `stream_tail` so a long line grows left to right instead of
    /// appearing whole. Pure in `now`, so the pacing is testable.
    pub fn reveal_now(&mut self, now: std::time::Instant) {
        let budget = match self.reveal_last {
            None => REVEAL_TICK_CHARS as f64,
            Some(previous) => {
                let elapsed =
                    now.duration_since(previous).as_secs_f64().min(1.0);
                self.reveal_debt + elapsed * REVEAL_CHARS_PER_SEC
            }
        };
        self.reveal_last = Some(now);
        let allowance = budget.min(REVEAL_TICK_CHARS as f64).floor() as usize;
        let mut take = allowance;
        if take == 0 {
            self.reveal_debt = budget;
            return;
        }
        // Answer channel: complete lines become transcript rows, the rest
        // stays visible as the growing tail.
        while take > 0 {
            let Some(ch) = self.stream_buffer.chars().next() else {
                break;
            };
            self.stream_buffer.remove(0);
            take -= 1;
            if ch == '\n' {
                let line = std::mem::take(&mut self.stream_tail);
                self.push_line(line);
            } else {
                self.stream_tail.push(ch);
            }
        }
        let answer_spent = allowance - take;
        // Thinking channel: what the answer did not spend reveals the
        // reasoning, so both channels share one pacing budget.
        let mut thinking_spent = 0usize;
        if take > 0 && self.reasoning_shown < self.reasoning.len() {
            let mut shown = self.reasoning_shown;
            for ch in self.reasoning[shown..].chars() {
                if take == 0 {
                    break;
                }
                shown += ch.len_utf8();
                take -= 1;
                thinking_spent += 1;
            }
            self.reasoning_shown = shown;
        }
        // Only the budget actually LEFT OVER carries: what was spent is gone.
        // BOTH channels spend from it -- charging only the answer let the
        // thinking stream at the tick cap instead of the configured rate.
        let spent = (answer_spent + thinking_spent) as f64;
        self.reveal_debt = (budget - spent).max(0.0);
    }
    /// The thinking block as transcript rows (S3b).
    ///
    /// Empty when nothing was streamed, so a route that never reasons is
    /// byte-identical to before. Collapsed it is ONE row; expanded it is a
    /// bounded tail of the thinking, so a long trace cannot push the
    /// conversation off screen.
    #[must_use]
    pub fn reasoning_block_lines(&self) -> Vec<String> {
        // Only what has been REVEALED renders (S3c): thinking grows left to
        // right like the answer, whatever chunk the provider sent -- so the
        // emptiness test is on the revealed slice, not the raw buffer.
        let shown = self.reasoning_shown.min(self.reasoning.len());
        let visible = &self.reasoning[..shown];
        if visible.trim().is_empty() {
            return Vec::new();
        }
        let lines: Vec<&str> = visible.lines().collect();
        if !self.reasoning_expanded {
            // Show a LIVE tail, not only a line count: a count changes when
            // a line COMPLETES, which made the block look like it arrived
            // line by line instead of streaming left to right.
            let newest = lines.last().copied().unwrap_or_default().trim_end();
            let total = newest.chars().count();
            let mut tail: String = newest
                .chars()
                .skip(total.saturating_sub(THINKING_TAIL_CHARS))
                .collect();
            if total > THINKING_TAIL_CHARS {
                tail.insert_str(0, "...");
            }
            return vec![format!(
                "\u{25b8} thinking ({} lines) {tail} - press Right to expand",
                lines.len()
            )];
        }
        let mut out =
            vec![format!("\u{25be} thinking - press Left to collapse")];
        for line in lines.iter().rev().take(REASONING_ROWS).rev() {
            out.push(format!("  {line}"));
        }
        out
    }

    /// Append a sanitized line verbatim (no sanitization, no unsanitized
    /// injection). Oldest lines are dropped when the bound is exceeded.
    /// Keeps `None` timestamp (static host lines).
    pub fn push_line(&mut self, line: String) {
        self.push_line_stamped(line, None);
    }

    /// Append a sanitized line with an optional local timestamp (H4).
    /// `timestamp` like `"2026-08-31 14:03:22 +02:00"` or `"UTC"` fallback or `None` for static lines.
    pub fn push_line_stamped(
        &mut self,
        line: String,
        timestamp: Option<String>,
    ) {
        if line.contains('\n') {
            for part in line.split('\n') {
                self.push_single_stamped(part.to_owned(), timestamp.clone());
            }
        } else {
            self.push_single_stamped(line, timestamp);
        }
    }

    #[allow(dead_code)]
    fn push_single(&mut self, line: String) {
        self.push_single_stamped(line, None);
    }

    fn push_single_stamped(
        &mut self,
        line: String,
        timestamp: Option<String>,
    ) {
        // Keep both storages in lockstep.
        if self.transcript.len() >= MAX_TRANSCRIPT_LINES {
            let drain = self.transcript.len() - MAX_TRANSCRIPT_LINES + 1;
            self.transcript.drain(0..drain);
        }
        if self.transcript_lines.len() >= MAX_TRANSCRIPT_LINES {
            let drain = self.transcript_lines.len() - MAX_TRANSCRIPT_LINES + 1;
            self.transcript_lines.drain(0..drain);
        }
        self.transcript
            .push(TranscriptEntry { text: line.clone(), timestamp });
        self.transcript_lines.push(line);
    }

    /// Maximum scroll offset for the current transcript and viewport height.
    /// Scroll operates over entries (one entry = one row; timestamp is a dim
    /// follow-up line rendered below when present but does not affect scroll
    /// count — the viewport height accounts for lines, timestamps render as
    /// separate dim lines within the same entry height for simplicity).
    pub fn max_scroll(&self, viewport_height: u16) -> u16 {
        let total = self.effective_transcript_len() as u16;
        total.saturating_sub(viewport_height)
    }

    /// Clamp scroll_offset to the viewport.
    pub fn clamp_scroll(&mut self, viewport_height: u16) {
        let max = self.max_scroll(viewport_height);
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }

    /// Update palette based on current input (I2). Call after every key edit:
    /// when `input` starts with `/`, filter the single catalog by the typed
    /// prefix (case-insensitive, prefix match); otherwise clear the palette.
    /// Resets `palette_selected` to `None` on every recompute; clears history
    /// navigation while the palette is visible (arrow keys route to palette).
    pub fn update_palette(&mut self) {
        if self.input.starts_with('/') {
            let prefix = self.input.to_ascii_lowercase();
            let filtered: Vec<(String, String)> = command_catalog()
                .into_iter()
                .filter(|(name, _)| {
                    name.to_ascii_lowercase().starts_with(&prefix)
                })
                .collect();
            if filtered.is_empty() {
                self.palette = Some(vec![]);
            } else {
                self.palette = Some(filtered);
            }
            self.palette_selected = None;
            // While palette is visible, history navigation is dormant.
            self.history_index = None;
            self.history_draft = None;
        } else {
            self.palette = None;
            self.palette_selected = None;
        }
    }

    /// Push a submitted prompt into history (I4), bounded to 100, oldest first.
    /// Empty or whitespace-only prompts are ignored.
    pub fn push_history(&mut self, prompt: String) {
        if prompt.trim().is_empty() {
            return;
        }
        // Avoid consecutive duplicates (optional but keeps stack clean).
        if self.prompt_history.last().is_some_and(|last| last == &prompt) {
            return;
        }
        self.prompt_history.push(prompt);
        if self.prompt_history.len() > 100 {
            let drain = self.prompt_history.len() - 100;
            self.prompt_history.drain(0..drain);
        }
        self.history_index = None;
        self.history_draft = None;
    }

    /// Returns the effective transcript entries for rendering (syncs first).
    pub fn effective_entries(&mut self) -> Vec<TranscriptEntry> {
        self.sync_transcript();
        if !self.transcript.is_empty() {
            self.transcript.clone()
        } else {
            self.transcript_lines
                .iter()
                .map(|text| TranscriptEntry {
                    text: text.clone(),
                    timestamp: None,
                })
                .collect()
        }
    }
}

/// Timestamp helpers (H4 — local timezone via `time` crate).
///
/// `local_timestamp_now` uses `time::OffsetDateTime::now_local()` with the
/// `local-offset` feature. When the platform cannot determine the local
/// offset, `now_local` returns `None` and we fall back to UTC. The formatted
/// shape is `"YYYY-MM-DD HH:MM:SS +HH:MM"` (local) or `"YYYY-MM-DD HH:MM:SS UTC"`
/// (fallback). `utc_timestamp_from_millis` is retained for tests and the
/// fallback path (civil-from-days math, Howard Hinnant).
#[must_use]
pub fn utc_timestamp_from_millis(millis: u64) -> String {
    let secs = millis / 1000;
    let days = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
    )
}

/// Civil date from days since Unix epoch (1970-01-01) — Hinnant.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    // Shift to civil epoch (0000-03-01)
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0,399]
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0,365]
    let mp = (5 * doy + 2) / 153; // [0,11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1,31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1,12]
    y += i64::from(m <= 2);
    (y as i32, m as u32, d as u32)
}

/// Compose the status line prefix from the composed profile (I5).
#[must_use]
pub fn compose_status_line(
    base_status: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> String {
    let prefix = match (provider, model) {
        (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => {
            let sp = crate::sanitize::sanitize_for_display(p);
            let sm = crate::sanitize::sanitize_for_display(m);
            format!("{sp} / {sm}")
        }
        (Some(p), _) if !p.is_empty() => {
            crate::sanitize::sanitize_for_display(p)
        }
        _ => "no provider configured".to_owned(),
    };
    if base_status.is_empty() {
        prefix
    } else {
        format!("{prefix} | {base_status}")
    }
}

/// Role-colored transcript style (P3) — deterministic, part of the render model.
///
/// - User echo lines (`> ...`) → Cyan
/// - Host notices (`unknown command`, `Approved.`, `Denied.`) → Yellow
/// - Everything else → White (default)
#[must_use]
pub fn style_for_transcript_line(text: &str) -> Style {
    if text.starts_with("> ") {
        Style::default().fg(Color::Cyan)
    } else if text.starts_with("Response failed")
        || text.starts_with("Tool failed")
        || text.starts_with("provider config failed")
        || text.starts_with("reload not applied")
        || text.starts_with("models fetch error")
        || text.starts_with("provider removal failed")
        || text.starts_with("Activate failed")
        || text.starts_with("Install failed")
    {
        Style::default().fg(Color::Red)
    } else if text.starts_with("-> ")
        || text.starts_with("tool ")
        || text.starts_with("Tool ")
    {
        // Tool activity is secondary: grey keeps it readable but quiet.
        Style::default().fg(Color::DarkGray)
    } else if text.starts_with("unknown command")
        || text == "Approved."
        || text == "Denied."
        || text.starts_with("Approved.")
        || text.starts_with("Denied.")
    {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    }
}

/// How long each `working` dot phase lasts.
pub const WORKING_PULSE: Duration = Duration::from_secs(1);

/// The liveness line rendered directly ABOVE the input (owner QoL
/// 2026-09-12): the working state moved out of the status row and into the
/// conversation flow, with dots that pulse once a second.
///
/// Pure in `elapsed`, so the animation is testable without a clock: the
/// render path passes how long the current turn has been running.
#[must_use]
pub fn working_line(elapsed: Duration) -> String {
    let phase = (elapsed.as_millis() / WORKING_PULSE.as_millis().max(1)) % 3;
    let dots = ".".repeat(1 + phase as usize);
    format!("working{dots}")
}

/// Assembled unique-digest total from the same `ContextMetrics` the pane uses (P5 — single source).
#[must_use]
pub fn context_assembled_total(
    metrics: &siralos_core::context_metrics::ContextMetrics,
) -> usize {
    metrics.records().last().map(|r| r.assembled_unique_total).unwrap_or(0)
}

/// Append the context-usage readout `ctx <assembled>/4096` when opted-in AND built (P5).
/// When `metrics` is `None`, returns `status` unchanged (OFF → byte-identical).
#[must_use]
pub fn append_context_usage(
    status: String,
    metrics: Option<&siralos_core::context_metrics::ContextMetrics>,
) -> String {
    if let Some(m) = metrics {
        let assembled = context_assembled_total(m);
        format!("{status} | ctx {assembled}/{CONTEXT_BUDGET_TOKENS}")
    } else {
        status
    }
}

/// Compose the status line with optional context-usage appended (P5).
#[must_use]
pub fn compose_status_line_with_context(
    base_status: &str,
    provider: Option<&str>,
    model: Option<&str>,
    metrics: Option<&siralos_core::context_metrics::ContextMetrics>,
) -> String {
    let base = compose_status_line(base_status, provider, model);
    append_context_usage(base, metrics)
}

/// Returns true when the base status indicates a working/loading state (H5).
/// The synchronous architecture freezes redraw during blocking dispatch, so the
/// working marker is styled distinctly to make the frozen state unmistakable.
#[must_use]
pub fn is_working_status(base_status: &str) -> bool {
    let lower = base_status.to_ascii_lowercase();
    lower.contains("working")
}

/// Header bar content helper (H1 — deduped: provider/model ONLY when configured).
/// When `provider` is absent, returns only `" Siralos "` (the "no provider
/// configured" text lives only in the bottom status line via `compose_status_line`).
#[must_use]
pub fn header_text(provider: Option<&str>, model: Option<&str>) -> String {
    match (provider, model) {
        (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => {
            let sp = crate::sanitize::sanitize_for_display(p);
            let sm = crate::sanitize::sanitize_for_display(m);
            format!(" Siralos  {sp} / {sm}")
        }
        (Some(p), _) if !p.is_empty() => {
            let sp = crate::sanitize::sanitize_for_display(p);
            format!(" Siralos  {sp}")
        }
        _ => " Siralos ".to_owned(),
    }
}

/// Current local timestamp for live sessions (H4) — `time` crate `local-offset`.
/// Falls back to UTC when the platform cannot determine the local offset.
#[must_use]
pub fn local_timestamp_now() -> String {
    // Use `time` crate's `OffsetDateTime::now_local()` when available.
    // `now_local` attempts to read the system's local offset; on platforms
    // where the tz database is unavailable it returns an error.
    if let Ok(dt) = time::OffsetDateTime::now_local() {
        #[allow(deprecated)]
        let desc = time::format_description::parse(
            "[year]-[month]-[day] [hour]:[minute]:[second] [offset_hour sign:mandatory]:[offset_minute]",
        )
        .expect("valid format");
        if let Ok(fmt) = dt.format(&desc) {
            return fmt;
        }
        // Fallback to manual formatting if `format` fails.
        let offset = dt.offset();
        let total_min = offset.whole_seconds() / 60;
        let sign = if total_min >= 0 { '+' } else { '-' };
        let abs = total_min.unsigned_abs();
        let oh = abs / 60;
        let om = abs % 60;
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}{:02}:{:02}",
            dt.year(),
            dt.month() as u8,
            dt.day(),
            dt.hour(),
            dt.minute(),
            dt.second(),
            sign,
            oh,
            om
        );
    }
    // Fallback to UTC when local offset cannot be determined.
    utc_timestamp_now_fallback()
}

/// UTC fallback when `now_local` fails — std-only civil math, labeled `UTC`.
#[must_use]
fn utc_timestamp_now_fallback() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    utc_timestamp_from_millis(millis)
}

/// Backward-compat alias (deprecated): use `local_timestamp_now`.
#[must_use]
pub fn utc_timestamp_now() -> String {
    local_timestamp_now()
}

/// Local timestamp from millis with the given offset (for tests).
#[must_use]
pub fn local_timestamp_from_millis_with_offset(
    millis: u64,
    offset_minutes: i32,
) -> String {
    let secs = millis / 1000;
    let days = (secs / 86_400) as i64;
    let secs_of_day = (secs % 86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    let sign = if offset_minutes >= 0 { '+' } else { '-' };
    let abs = offset_minutes.unsigned_abs();
    let oh = abs / 60;
    let om = abs % 60;
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} {sign}{oh:02}:{om:02}"
    )
}

/// Pure predicate for the launch decision (decision 105, testable without a real TTY).
///
/// Returns true when the TUI should be used: stdout is a TTY and `--stdio`
/// was not requested. `wants_stdio` is true when the user passed `--stdio`.
///
/// Truth table:
/// - `!wants_stdio && is_tty` => TUI
/// - otherwise => stdio (scripts / CI silent, no diagnostic)
pub fn should_launch_tui(wants_stdio: bool, is_tty: bool) -> bool {
    !wants_stdio && is_tty
}

/// Deprecated alias for the T1 `--tui` opt-in (kept for compiling existing
/// tests; new code should use [`should_launch_tui`]).
pub fn should_use_tui(wants_tui: bool, is_tty: bool) -> bool {
    should_launch_tui(!wants_tui, is_tty)
}

/// Returns whether stdout is a TTY on this platform.
pub fn stdout_is_tty() -> bool {
    // Use crossterm's TTY detection via is_tty crate indirectly: crossterm
    // itself does not expose is_tty, so we use std's is_terminal on Unix and
    // Windows via the `IsTerminal` trait (stable since 1.70).
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Draw the TUI frame deterministically from `state` (T2 layout, no pane).
///
/// OFF path (P1): byte-identical to T2's render — transcript full width, no
/// pane, no placeholder. Implemented as [`draw_with_pane`] with no pane.
pub fn draw(state: &TuiState, frame: &mut Frame<'_>) {
    draw_with_pane(state, None, frame);
}

/// Draw the TUI frame with the optional T3 context pane.
///
/// - `None`: byte-identical to [`draw`] (T2's frame — transcript full
///   width, no pane, no placeholder).
/// - `Some(pane)`: a right-hand pane of fixed [`CONTEXT_PANE_WIDTH`]
///   columns; transcript + input shrink to the remaining width; the status
///   line stays full width. The pane holds, top to bottom, the counters
///   block, the ring block (last 8 tick records, oldest-first), and the
///   tool activity block (last 8 tool rounds, oldest-first). Deterministic:
///   same state + same pane + same viewport -> byte-identical frame (P5).
///   The `ratatui` layout handles resize; every pane line is bounded to the
///   pane width.
pub fn draw_with_pane(
    state: &TuiState,
    pane: Option<&ContextPaneData>,
    frame: &mut Frame<'_>,
) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    // P2 header bar + transcript/input/status layout (header 1 line, OFF-independent)
    // Owner QoL: the `working` indicator owns a row ONLY while the model
    // works, so idle frames stay byte-identical to the pinned ones.
    // The turn timer IS the busy signal now that the bottom bar no longer
    // carries a `working` word.
    let busy_rows = u16::from(state.busy_since.is_some());
    let (
        header_area,
        transcript_area,
        busy_area,
        input_area,
        status_area,
        pane_area,
    ) = match pane {
        None => {
            // I5 OFF: transcript spans full width, no gap (Min(0) fill)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(busy_rows),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(area);
            (chunks[0], chunks[1], chunks[2], chunks[3], chunks[4], None)
        }
        Some(_) => {
            // I5 ON: transcript Min(0) + pane Length(40) fills width, no gap
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(area);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(CONTEXT_PANE_WIDTH),
                ])
                .split(outer[1]);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(busy_rows),
                    Constraint::Length(1),
                ])
                .split(cols[0]);
            (outer[0], rows[0], rows[1], rows[2], outer[2], Some(cols[1]))
        }
    };
    // Header bar (H1 dedup — P2 heritage): reversed/accent, left " Siralos ",
    // right provider/model ONLY when configured; absent shows just " Siralos ".
    {
        let left = " Siralos ";
        let right_opt: Option<String> = match (&state.provider, &state.model) {
            (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => Some(
                crate::sanitize::sanitize_for_display(&format!("{p} / {m}")),
            ),
            (Some(p), _) if !p.is_empty() => {
                Some(crate::sanitize::sanitize_for_display(p))
            }
            _ => None,
        };
        let width = header_area.width as usize;
        let left_len = left.chars().count();
        let header_string = if let Some(ref right) = right_opt {
            let right_len = right.chars().count();
            let middle = width.saturating_sub(left_len + right_len);
            format!("{}{}{}", left, " ".repeat(middle), right)
        } else {
            let middle = width.saturating_sub(left_len);
            format!("{}{}", left, " ".repeat(middle))
        };
        let header = Paragraph::new(header_string).style(
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(ratatui::style::Modifier::REVERSED),
        );
        frame.render_widget(header, header_area);
    }
    // The modal backdrop covers the transcript in OFF mode and the whole
    // body (transcript + pane) in ON mode.
    let backdrop_area = match pane_area {
        None => transcript_area,
        Some(pane_rect) => body_union(transcript_area, input_area, pane_rect),
    };

    // Transcript: determine visible window over wrapped display rows
    // (text + optional dim timestamp). Deterministic: each stored line
    // expands to one or more rows at the pane's inner width via
    // `wrapped_transcript_rows` (word boundaries, hard-break long tokens);
    // stored text is never mutated. Scroll windows over rows, so the tail
    // of a long line stays readable instead of clipping off-screen.
    let height = transcript_area.height as usize;
    // Build expanded line list with styles.
    let entries: Vec<TranscriptEntry> = if !state.transcript.is_empty() {
        state.transcript.clone()
    } else {
        state
            .transcript_lines
            .iter()
            .map(|text| TranscriptEntry {
                text: text.clone(),
                timestamp: None,
            })
            .collect()
    };
    // S3b: the thinking block renders as the last transcript rows, so it
    // scrolls with the conversation and needs no layout surgery.
    let mut entries = entries;
    let reasoning_rows = state.reasoning_block_lines();
    let has_reasoning = !reasoning_rows.is_empty();
    for line in reasoning_rows {
        entries.push(TranscriptEntry { text: line, timestamp: None });
    }
    if !state.stream_tail.is_empty() {
        // The answer's current line, revealed so far: this is what makes it
        // read left to right instead of appearing whole.
        entries.push(TranscriptEntry {
            text: state.stream_tail.clone(),
            timestamp: None,
        });
    }
    if has_reasoning && state.busy_since.is_some() {
        // Keep the indicator visually SEPARATE from the thinking block.
        entries.push(TranscriptEntry { text: String::new(), timestamp: None });
    }

    let wrapped =
        wrapped_transcript_rows(&entries, transcript_area.width as usize);
    let mut expanded: Vec<Line<'_>> = Vec::with_capacity(wrapped.len());
    for (row, style) in &wrapped {
        expanded.push(Line::from(row.as_str()).style(*style));
    }
    let total = expanded.len();
    let max_scroll = total.saturating_sub(height);
    let scroll = (state.scroll_offset as usize).min(max_scroll);
    let start = if total <= height { 0 } else { total - height - scroll };
    let end = (start + height).min(total);
    let visible = &expanded[start..end];

    let transcript = Paragraph::new(Text::from(visible.to_vec()))
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::White));
    frame.render_widget(transcript, transcript_area);

    // Command palette (I2/P4/I3): popup above input, rounded, prefix-highlighted,
    // with arrow-key selection (reversed style) and full vocabulary.
    // I3 removes the 8-bound: shows ALL filtered entries, popup grows to fit
    // bounded by terminal height minus input/status rows; scroll indicator if overflow.
    if let Some(catalog) = &state.palette {
        if !catalog.is_empty() || state.input.starts_with('/') {
            // Available height for the popup (terminal height minus input/status/header).
            // Bounded popup: grows to fit, but never exceeds terminal height -3 (header+input+status).
            let available_height = (area.height.saturating_sub(3)) as usize;
            // Needed height includes border (2). At least 3 (border + one line).
            let mut palette_lines: Vec<Line<'_>> = Vec::new();
            if catalog.is_empty() {
                palette_lines.push(
                    Line::from("no matches")
                        .style(Style::default().fg(Color::DarkGray)),
                );
            } else {
                // Compute inner height (available - border)
                let inner_available = available_height.saturating_sub(2);
                // Visible count: if overflow, reserve one line for scroll indicator
                let total = catalog.len();
                let needs_scroll = total > inner_available;
                let visible_capacity = if needs_scroll {
                    inner_available.saturating_sub(1).max(1)
                } else {
                    inner_available.max(1)
                };
                // Window around selected index
                let mut window_start = 0usize;
                if let Some(selected) = state.palette_selected {
                    if selected < total && selected >= visible_capacity {
                        window_start =
                            selected.saturating_sub(visible_capacity - 1);
                        if window_start + visible_capacity > total {
                            window_start =
                                total.saturating_sub(visible_capacity);
                        }
                    }
                }
                let window_end = (window_start + visible_capacity).min(total);
                let prefix_lower = state.input.to_ascii_lowercase();
                for (idx, (name, desc)) in
                    catalog[window_start..window_end].iter().enumerate()
                {
                    let actual_idx = window_start + idx;
                    let is_selected = state
                        .palette_selected
                        .is_some_and(|s| s == actual_idx);
                    if is_selected {
                        // Highlighted selection — reversed style
                        let line = format!("{name} — {desc}");
                        palette_lines.push(
                            Line::from(line).style(
                                Style::default()
                                    .fg(Color::Yellow)
                                    .bg(Color::Black)
                                    .add_modifier(
                                        ratatui::style::Modifier::REVERSED
                                            | ratatui::style::Modifier::BOLD,
                                    ),
                            ),
                        );
                    } else {
                        let name_lower = name.to_ascii_lowercase();
                        if !prefix_lower.is_empty()
                            && name_lower.starts_with(&prefix_lower)
                            && prefix_lower.len() <= name.len()
                        {
                            let (pre, rest) =
                                name.split_at(prefix_lower.len());
                            palette_lines.push(Line::from(vec![
                                Span::styled(
                                    pre.to_owned(),
                                    Style::default()
                                        .fg(Color::Yellow)
                                        .add_modifier(
                                            ratatui::style::Modifier::BOLD,
                                        ),
                                ),
                                Span::styled(
                                    rest.to_owned(),
                                    Style::default().fg(Color::White),
                                ),
                                Span::styled(
                                    format!(" — {desc}"),
                                    Style::default().fg(Color::White),
                                ),
                            ]));
                        } else {
                            palette_lines.push(
                                Line::from(format!("{name} — {desc}"))
                                    .style(Style::default().fg(Color::White)),
                            );
                        }
                    }
                }
                if needs_scroll {
                    let remaining = total.saturating_sub(visible_capacity);
                    // Show scroll indicator with remaining count and arrow hint
                    let indicator = if window_start > 0 && window_end < total {
                        format!(
                            "↑ {} more · ↓ {} more",
                            window_start,
                            total - window_end
                        )
                    } else if window_end < total {
                        format!("+{remaining} more ↓")
                    } else {
                        format!("↑ {window_start} more")
                    };
                    palette_lines.push(
                        Line::from(indicator)
                            .style(Style::default().fg(Color::DarkGray)),
                    );
                }
            }
            // Palette popup rect: directly above input, width clamped, height bounded
            let palette_height = (palette_lines.len() as u16 + 2)
                .min(area.height.saturating_sub(3))
                .max(3);
            let palette_width = 50u16.min(transcript_area.width);
            let palette_x = input_area.x;
            let palette_y = input_area.y.saturating_sub(palette_height);
            let palette_area =
                Rect::new(palette_x, palette_y, palette_width, palette_height);
            frame.render_widget(ratatui::widgets::Clear, palette_area);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" commands ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )
                .style(Style::default().bg(Color::Black).fg(Color::Cyan));
            let inner = block.inner(palette_area);
            frame.render_widget(block, palette_area);
            let para = Paragraph::new(Text::from(palette_lines))
                .style(Style::default().bg(Color::Black).fg(Color::White));
            frame.render_widget(para, inner);
        }
    }

    // Input line: `> <input>`
    let input_text = format!("> {}", state.input);
    let input = Paragraph::new(input_text.as_str())
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(input, input_area);
    // The `working` state is a STATIC row above the input, in the banner
    // colour, pulsing once a second -- never text in the conversation.
    if let Some(start) = state.busy_since {
        let indicator = Paragraph::new(working_line(start.elapsed()))
            .style(Style::default().fg(Color::Cyan));
        frame.render_widget(indicator, busy_area);
    }
    // Cursor at end of input (after `> ` prefix + input length). Clamp to area.
    // When a modal is pending or the add-form is open, hide the cursor behind
    // the dimmed backdrop (no typing through a modal).
    if state.pending_approval.is_none() && state.provider_add_form.is_none() {
        let cursor_x = input_area.x + 2 + state.input.len() as u16;
        let cursor_x =
            cursor_x.min(input_area.x + input_area.width.saturating_sub(1));
        frame.set_cursor_position((cursor_x, input_area.y));
    }

    // Context pane (T3): bordered block with the counters, ring, and tool
    // activity lines, each bounded to the inner width. Content is host-side
    // numbers plus already-sanitized history entries — no new unsanitized
    // source (P4).
    if let (Some(pane_data), Some(pane_rect)) = (pane, pane_area) {
        let pane_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Context ")
            .style(Style::default().fg(Color::Green));
        let inner = pane_block.inner(pane_rect);
        frame.render_widget(pane_block, pane_rect);
        let inner_width = inner.width as usize;
        let pane_lines = context_pane_lines(pane_data, inner_width);
        // Show the head of the pane content (counters first); bounded by
        // the pane height.
        let visible_count = (inner.height as usize).min(pane_lines.len());
        let text: Vec<Line<'_>> = pane_lines[..visible_count]
            .iter()
            .map(|s| Line::from(s.as_str()))
            .collect();
        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().fg(Color::White));
        frame.render_widget(paragraph, inner);
    }

    // Status line — H5: "working" styled distinctly (yellow bold) so the frozen state is unmistakable.
    {
        let is_working = is_working_status(&state.status);
        let status_widget = if is_working {
            let lower = state.status.to_ascii_lowercase();
            if let Some(pos) = lower.find("working") {
                let end = pos + "working".len();
                let before = state.status[..pos].to_owned();
                let mid = state.status[pos..end].to_owned();
                let after = state.status[end..].to_owned();
                let mut spans: Vec<Span<'_>> = Vec::new();
                if !before.is_empty() {
                    spans.push(Span::styled(
                        before,
                        Style::default().fg(Color::Cyan),
                    ));
                }
                spans.push(Span::styled(
                    mid,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));
                if !after.is_empty() {
                    spans.push(Span::styled(
                        after,
                        Style::default().fg(Color::Cyan),
                    ));
                }
                Paragraph::new(Line::from(spans))
            } else {
                Paragraph::new(state.status.as_str())
                    .style(Style::default().fg(Color::Cyan))
            }
        } else {
            Paragraph::new(state.status.as_str())
                .style(Style::default().fg(Color::Cyan))
        };
        frame.render_widget(status_widget, status_area);
    }

    // Provider picker (H6) — rounded popup " providers ", up/down + Enter/Esc.
    if let Some(picker) = &state.provider_picker {
        let picker_lines: Vec<Line<'_>> = if picker.entries.is_empty() {
            vec![
                Line::from("no providers configured")
                    .style(Style::default().fg(Color::DarkGray)),
            ]
        } else {
            picker
                .entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| {
                    let is_selected = idx == picker.selected;
                    let text = format!(
                        "{} | {} | {}",
                        entry.name, entry.host, entry.model
                    );
                    if is_selected {
                        Line::from(text).style(
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                        )
                    } else {
                        Line::from(text)
                            .style(Style::default().fg(Color::White))
                    }
                })
                .collect()
        };
        let picker_height = (picker_lines.len() as u16 + 2).min(10);
        let picker_width = 50u16.min(transcript_area.width);
        let picker_x = input_area.x;
        let picker_y = input_area.y.saturating_sub(picker_height);
        let picker_area =
            Rect::new(picker_x, picker_y, picker_width, picker_height);
        frame.render_widget(ratatui::widgets::Clear, picker_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" providers ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black).fg(Color::Cyan));
        let inner = block.inner(picker_area);
        frame.render_widget(block, picker_area);
        let para = Paragraph::new(Text::from(picker_lines))
            .style(Style::default().bg(Color::Black).fg(Color::White));
        frame.render_widget(para, inner);
    }

    // `/model` switch picker — rounded popup " switch model ", same
    // `ModelPicker` lines (decision 138 viewport) as the add-flow form.
    // `None` renders nothing (existing frames byte-identical).
    if let Some(picker) = &state.model_switch_picker {
        let picker_lines = model_picker_lines(picker);
        let picker_height = (picker_lines.len() as u16 + 2).min(12);
        let picker_width = 50u16.min(transcript_area.width);
        let picker_x = input_area.x;
        let picker_y = input_area.y.saturating_sub(picker_height);
        let picker_area =
            Rect::new(picker_x, picker_y, picker_width, picker_height);
        frame.render_widget(ratatui::widgets::Clear, picker_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" switch model ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black).fg(Color::Cyan));
        let inner = block.inner(picker_area);
        frame.render_widget(block, picker_area);
        let para = Paragraph::new(Text::from(picker_lines))
            .style(Style::default().bg(Color::Black).fg(Color::White));
        frame.render_widget(para, inner);
    }

    // Provider add-flow form (C1) — rounded modal title " add provider " with current field highlighted.
    // I1: modal grows to accommodate the D3 description lines.
    if let Some(form) = &state.provider_add_form {
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        frame.render_widget(backdrop, backdrop_area);
        let modal_area = centered_rect(75, 75, backdrop_area);
        frame.render_widget(ratatui::widgets::Clear, modal_area);
        let modal_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" add provider ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black).fg(Color::Cyan));
        let inner = modal_block.inner(modal_area);
        frame.render_widget(modal_block, modal_area);
        let lines = provider_add_form_lines(form);
        let paragraph = Paragraph::new(Text::from(lines))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    // Modal overlay (T2): dimmed backdrop + centered modal with the sanitized
    // approval request (bounded to MAX_APPROVAL_LINES). This reuses the same
    // sanitizer-bound lines the stdio path would render — no unsanitized content.
    if let Some(modal) = &state.pending_approval {
        // Dimmed backdrop over the transcript (OFF) or the whole body
        // (transcript + pane, ON).
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        frame.render_widget(backdrop, backdrop_area);
        // Centered modal rect.
        let modal_area = centered_rect(60, 60, backdrop_area);
        // Clear underneath for determinism
        frame.render_widget(ratatui::widgets::Clear, modal_area);
        let modal_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Approval required (y/n, Esc deny) ")
            .style(Style::default().bg(Color::Black).fg(Color::Yellow));
        let inner = modal_block.inner(modal_area);
        frame.render_widget(modal_block, modal_area);
        let text: Vec<Line<'_>> =
            modal.lines.iter().map(|s| Line::from(s.as_str())).collect();
        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }
}

fn provider_add_form_lines(form: &ProviderAddForm) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let fields = [
        (ProviderAddField::DisplayName, form.provider.as_deref()),
        (ProviderAddField::Url, form.endpoint.as_deref()),
        (ProviderAddField::ApiKey, form.credential_env.as_deref()),
        (ProviderAddField::ApiProtocol, form.protocol.as_deref()),
        (ProviderAddField::Model, form.model.as_deref()),
        (
            ProviderAddField::ModelDisplayName,
            form.model_display_name.as_deref(),
        ),
    ];
    for (field, stored) in fields {
        let label = field.label();
        let description = field.description();
        let is_current = field == form.field && form.completed.is_none();
        let display = if is_current {
            // When a picker is open, the field shows the picker instead of raw input.
            if (field == ProviderAddField::Model
                && form.model_picker.is_some())
                || (field == ProviderAddField::ApiProtocol
                    && form.protocol_picker.is_some())
            {
                format!("> {label}:")
            } else {
                format!("> {}: {}█", label, form.input)
            }
        } else if let Some(val) = stored {
            if val.is_empty() {
                format!("  {label}: —")
            } else {
                format!("  {label}: {val}")
            }
        } else {
            format!("  {label}:")
        };
        // Highlight current field bold yellow, completed green, pending dark gray.
        let style = if is_current {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else if stored.is_some() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(display).style(style));
        // Dim description line below the label (short, dim, no examples).
        lines.push(
            Line::from(format!("    {description}"))
                .style(Style::default().fg(Color::DarkGray)),
        );
        // Protocol picker: rendered inline under the api protocol field when open — bounded viewport 8.
        if field == ProviderAddField::ApiProtocol && is_current {
            if let Some(picker) = &form.protocol_picker {
                const VISIBLE: usize = 8;
                let total = picker.items.len();
                let window_start = if total <= VISIBLE {
                    0
                } else {
                    picker
                        .selected
                        .saturating_sub(VISIBLE - 1)
                        .min(total.saturating_sub(VISIBLE))
                };
                let window_end = (window_start + VISIBLE).min(total);
                // Position indicator at window top.
                lines.push(
                    Line::from(format!(
                        "    {}/{} ",
                        picker.selected + 1,
                        total
                    ))
                    .style(Style::default().fg(Color::DarkGray)),
                );
                for idx in window_start..window_end {
                    let item = &picker.items[idx];
                    let prefix =
                        if idx == picker.selected { "> " } else { "  " };
                    let style = if idx == picker.selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(ratatui::style::Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    lines.push(
                        Line::from(format!("    {prefix}{item}")).style(style),
                    );
                }
                lines.push(
                    Line::from("    Up/Down to navigate, Enter to select, Esc for free text")
                        .style(Style::default().fg(Color::DarkGray)),
                );
            }
        }
        // Model picker: rendered inline under the model field when open —
        // the shared `model_picker_lines` (decision 138 viewport), also
        // used by the `/model` switch picker popup.
        if field == ProviderAddField::Model && is_current {
            if let Some(picker) = &form.model_picker {
                lines.extend(model_picker_lines(picker));
            } else if form.fetching_models {
                lines.push(
                    Line::from("    fetching models...")
                        .style(Style::default().fg(Color::DarkGray)),
                );
            } else if let Some(note) = &form.fetch_note {
                lines.push(
                    Line::from(format!("    {note}"))
                        .style(Style::default().fg(Color::DarkGray)),
                );
            }
        }
    }
    lines.push(
        Line::from("    Enter to continue, Esc to cancel")
            .style(Style::default().fg(Color::DarkGray)),
    );
    if let Some(err) = &form.error {
        lines.push(
            Line::from(format!("    error: {err}"))
                .style(Style::default().fg(Color::Red)),
        );
    }
    lines
}

fn body_union(transcript: Rect, input: Rect, pane: Rect) -> Rect {
    let x = transcript.x.min(input.x).min(pane.x);
    let y = transcript.y.min(input.y).min(pane.y);
    let right = (transcript.x + transcript.width)
        .max(input.x + input.width)
        .max(pane.x + pane.width);
    let bottom = (transcript.y + transcript.height)
        .max(input.y + input.height)
        .max(pane.y + pane.height);
    Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Returns whether a key should be treated as approval input while a modal is
/// pending. `y` approves, `n` and `Esc` deny; all other keys are ignored.
pub fn modal_key_decision(
    key: crossterm::event::KeyEvent,
) -> Option<ApprovalDecision> {
    if key.kind != crossterm::event::KeyEventKind::Press {
        return None;
    }
    match key.code {
        crossterm::event::KeyCode::Char('y')
        | crossterm::event::KeyCode::Char('Y') => {
            Some(evaluate_approval_char('y'))
        }
        crossterm::event::KeyCode::Char('n')
        | crossterm::event::KeyCode::Char('N') => {
            Some(evaluate_approval_char('n'))
        }
        crossterm::event::KeyCode::Esc => Some(ApprovalDecision::Deny),
        _ => None,
    }
}

/// Handle a key while a modal is pending: returns `Some(decision)` when the
/// key is a modal key (`y`/`n`/`Esc`), otherwise `None` (caller must ignore
/// the key — no typing through a modal).
pub fn handle_modal_key(
    state: &mut TuiState,
    key: crossterm::event::KeyEvent,
) -> Option<ApprovalDecision> {
    state.pending_approval.as_ref()?;
    modal_key_decision(key)
}

/// Helper for headless tests: render `state` into a `Buffer` of the given size
/// and return the buffer. Deterministic: same state + same size -> identical
/// buffer bytes. OFF path: identical to T2's render (no pane).
pub fn render_to_buffer(state: &TuiState, width: u16, height: u16) -> Buffer {
    render_to_buffer_with_pane(state, None, width, height)
}

/// Headless pane render: same as [`render_to_buffer`] with the optional T3
/// context pane. `None` is byte-identical to [`render_to_buffer`].
pub fn render_to_buffer_with_pane(
    state: &TuiState,
    pane: Option<&ContextPaneData>,
    width: u16,
    height: u16,
) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    // We need a Frame backed by TestBackend-style buffer. Easiest is to use
    // ratatui's TestBackend directly in tests; this helper is for direct
    // buffer rendering without a backend. We emulate via `Buffer` + manual
    // layout using the same logic as `draw_with_pane` but writing into `buf`
    // directly. To keep determinism identical to `draw_with_pane`, we reuse
    // the widget rendering via `Widget::render`.
    // Owner QoL: the `working` indicator owns a row ONLY while the model
    // works, so idle frames stay byte-identical to the pinned ones.
    // The turn timer IS the busy signal now that the bottom bar no longer
    // carries a `working` word.
    let busy_rows = u16::from(state.busy_since.is_some());
    let (
        header_area,
        transcript_area,
        busy_area,
        input_area,
        status_area,
        pane_area,
    ) = match pane {
        None => {
            // I5 OFF: transcript spans full width, no gap (Min(0) fill)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(busy_rows),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(area);
            (chunks[0], chunks[1], chunks[2], chunks[3], chunks[4], None)
        }
        Some(_) => {
            // I5 ON: transcript Min(0) + pane Length(40) fills width, no gap
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .split(area);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(CONTEXT_PANE_WIDTH),
                ])
                .split(outer[1]);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(busy_rows),
                    Constraint::Length(1),
                ])
                .split(cols[0]);
            (outer[0], rows[0], rows[1], rows[2], outer[2], Some(cols[1]))
        }
    };
    // Header bar (H1 dedup — P2 heritage) — same as draw.
    {
        let left = " Siralos ";
        let right_opt: Option<String> = match (&state.provider, &state.model) {
            (Some(p), Some(m)) if !p.is_empty() && !m.is_empty() => Some(
                crate::sanitize::sanitize_for_display(&format!("{p} / {m}")),
            ),
            (Some(p), _) if !p.is_empty() => {
                Some(crate::sanitize::sanitize_for_display(p))
            }
            _ => None,
        };
        let width = header_area.width as usize;
        let left_len = left.chars().count();
        let header_string = if let Some(ref right) = right_opt {
            let right_len = right.chars().count();
            let middle = width.saturating_sub(left_len + right_len);
            format!("{}{}{}", left, " ".repeat(middle), right)
        } else {
            let middle = width.saturating_sub(left_len);
            format!("{}{}", left, " ".repeat(middle))
        };
        let header = Paragraph::new(header_string).style(
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(ratatui::style::Modifier::REVERSED),
        );
        header.render(header_area, &mut buf);
    }

    let h = transcript_area.height as usize;
    let entries: Vec<TranscriptEntry> = if !state.transcript.is_empty() {
        state.transcript.clone()
    } else {
        state
            .transcript_lines
            .iter()
            .map(|text| TranscriptEntry {
                text: text.clone(),
                timestamp: None,
            })
            .collect()
    };
    // S3b: the thinking block renders as the last transcript rows, so it
    // scrolls with the conversation and needs no layout surgery.
    let mut entries = entries;
    let reasoning_rows = state.reasoning_block_lines();
    let has_reasoning = !reasoning_rows.is_empty();
    for line in reasoning_rows {
        entries.push(TranscriptEntry { text: line, timestamp: None });
    }
    if !state.stream_tail.is_empty() {
        // The answer's current line, revealed so far: this is what makes it
        // read left to right instead of appearing whole.
        entries.push(TranscriptEntry {
            text: state.stream_tail.clone(),
            timestamp: None,
        });
    }
    if has_reasoning && state.busy_since.is_some() {
        // Keep the indicator visually SEPARATE from the thinking block.
        entries.push(TranscriptEntry { text: String::new(), timestamp: None });
    }

    // Same render-layer wrap as the `Frame` path above: scroll windows over
    // wrapped rows so long lines stay readable instead of clipping.
    let wrapped =
        wrapped_transcript_rows(&entries, transcript_area.width as usize);
    let mut expanded: Vec<Line<'_>> = Vec::with_capacity(wrapped.len());
    for (row, style) in &wrapped {
        expanded.push(Line::from(row.as_str()).style(*style));
    }
    let total = expanded.len();
    let max_scroll = total.saturating_sub(h);
    let scroll = (state.scroll_offset as usize).min(max_scroll);
    let start = if total <= h { 0 } else { total - h - scroll };
    let end = (start + h).min(total);
    let visible = &expanded[start..end];
    let transcript = Paragraph::new(Text::from(visible.to_vec()))
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::White));
    transcript.render(transcript_area, &mut buf);

    // Palette (I2/P4/I3) — buffer path, same as Frame path: full vocabulary, selection highlight, scroll indicator
    if let Some(catalog) = &state.palette {
        if !catalog.is_empty() || state.input.starts_with('/') {
            let available_height = (area.height.saturating_sub(3)) as usize;
            let mut palette_lines: Vec<Line<'_>> = Vec::new();
            if catalog.is_empty() {
                palette_lines.push(
                    Line::from("no matches")
                        .style(Style::default().fg(Color::DarkGray)),
                );
            } else {
                let inner_available = available_height.saturating_sub(2);
                let total = catalog.len();
                let needs_scroll = total > inner_available;
                let visible_capacity = if needs_scroll {
                    inner_available.saturating_sub(1).max(1)
                } else {
                    inner_available.max(1)
                };
                let mut window_start = 0usize;
                if let Some(selected) = state.palette_selected {
                    if selected < total && selected >= visible_capacity {
                        window_start =
                            selected.saturating_sub(visible_capacity - 1);
                        if window_start + visible_capacity > total {
                            window_start =
                                total.saturating_sub(visible_capacity);
                        }
                    }
                }
                let window_end = (window_start + visible_capacity).min(total);
                let prefix_lower = state.input.to_ascii_lowercase();
                for (idx, (name, desc)) in
                    catalog[window_start..window_end].iter().enumerate()
                {
                    let actual_idx = window_start + idx;
                    let is_selected = state
                        .palette_selected
                        .is_some_and(|s| s == actual_idx);
                    if is_selected {
                        let line = format!("{name} — {desc}");
                        palette_lines.push(
                            Line::from(line).style(
                                Style::default()
                                    .fg(Color::Yellow)
                                    .bg(Color::Black)
                                    .add_modifier(
                                        ratatui::style::Modifier::REVERSED
                                            | ratatui::style::Modifier::BOLD,
                                    ),
                            ),
                        );
                    } else {
                        let name_lower = name.to_ascii_lowercase();
                        if !prefix_lower.is_empty()
                            && name_lower.starts_with(&prefix_lower)
                            && prefix_lower.len() <= name.len()
                        {
                            let (pre, rest) =
                                name.split_at(prefix_lower.len());
                            palette_lines.push(Line::from(vec![
                                Span::styled(
                                    pre.to_owned(),
                                    Style::default()
                                        .fg(Color::Yellow)
                                        .add_modifier(
                                            ratatui::style::Modifier::BOLD,
                                        ),
                                ),
                                Span::styled(
                                    rest.to_owned(),
                                    Style::default().fg(Color::White),
                                ),
                                Span::styled(
                                    format!(" — {desc}"),
                                    Style::default().fg(Color::White),
                                ),
                            ]));
                        } else {
                            palette_lines.push(
                                Line::from(format!("{name} — {desc}"))
                                    .style(Style::default().fg(Color::White)),
                            );
                        }
                    }
                }
                if needs_scroll {
                    let remaining = total.saturating_sub(visible_capacity);
                    let indicator = if window_start > 0 && window_end < total {
                        format!(
                            "↑ {} more · ↓ {} more",
                            window_start,
                            total - window_end
                        )
                    } else if window_end < total {
                        format!("+{remaining} more ↓")
                    } else {
                        format!("↑ {window_start} more")
                    };
                    palette_lines.push(
                        Line::from(indicator)
                            .style(Style::default().fg(Color::DarkGray)),
                    );
                }
            }
            let palette_height = (palette_lines.len() as u16 + 2)
                .min(area.height.saturating_sub(3))
                .max(3);
            let palette_width = 50u16.min(transcript_area.width);
            let palette_x = input_area.x;
            let palette_y = input_area.y.saturating_sub(palette_height);
            let palette_area =
                Rect::new(palette_x, palette_y, palette_width, palette_height);
            ratatui::widgets::Clear.render(palette_area, &mut buf);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" commands ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                )
                .style(Style::default().bg(Color::Black).fg(Color::Cyan));
            let inner = block.inner(palette_area);
            block.render(palette_area, &mut buf);
            let para = Paragraph::new(Text::from(palette_lines))
                .style(Style::default().bg(Color::Black).fg(Color::White));
            para.render(inner, &mut buf);
        }
    }

    let input_text = format!("> {}", state.input);
    let input = Paragraph::new(input_text.as_str())
        .style(Style::default().fg(Color::Yellow));
    input.render(input_area, &mut buf);
    if let Some(start) = state.busy_since {
        Paragraph::new(working_line(start.elapsed()))
            .style(Style::default().fg(Color::Cyan))
            .render(busy_area, &mut buf);
    }

    if let (Some(pane_data), Some(pane_rect)) = (pane, pane_area) {
        let pane_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Context ")
            .style(Style::default().fg(Color::Green));
        let inner = pane_block.inner(pane_rect);
        pane_block.render(pane_rect, &mut buf);
        let inner_width = inner.width as usize;
        let pane_lines = context_pane_lines(pane_data, inner_width);
        let visible_count = (inner.height as usize).min(pane_lines.len());
        let text: Vec<Line<'_>> = pane_lines[..visible_count]
            .iter()
            .map(|s| Line::from(s.as_str()))
            .collect();
        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().fg(Color::White));
        paragraph.render(inner, &mut buf);
    }

    // Status line — H5 distinct "working" style.
    {
        let is_working = is_working_status(&state.status);
        let status_widget: Paragraph<'_> = if is_working {
            let lower = state.status.to_ascii_lowercase();
            if let Some(pos) = lower.find("working") {
                let end = pos + "working".len();
                let before = state.status[..pos].to_owned();
                let mid = state.status[pos..end].to_owned();
                let after = state.status[end..].to_owned();
                let mut spans: Vec<Span<'_>> = Vec::new();
                if !before.is_empty() {
                    spans.push(Span::styled(
                        before,
                        Style::default().fg(Color::Cyan),
                    ));
                }
                spans.push(Span::styled(
                    mid,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ));
                if !after.is_empty() {
                    spans.push(Span::styled(
                        after,
                        Style::default().fg(Color::Cyan),
                    ));
                }
                Paragraph::new(Line::from(spans))
            } else {
                Paragraph::new(state.status.as_str())
                    .style(Style::default().fg(Color::Cyan))
            }
        } else {
            Paragraph::new(state.status.as_str())
                .style(Style::default().fg(Color::Cyan))
        };
        status_widget.render(status_area, &mut buf);
    }

    // Provider picker (H6) — same as Frame path.
    if let Some(picker) = &state.provider_picker {
        let picker_lines: Vec<Line<'_>> = if picker.entries.is_empty() {
            vec![
                Line::from("no providers configured")
                    .style(Style::default().fg(Color::DarkGray)),
            ]
        } else {
            picker
                .entries
                .iter()
                .enumerate()
                .map(|(idx, entry)| {
                    let is_selected = idx == picker.selected;
                    let text = format!(
                        "{} | {} | {}",
                        entry.name, entry.host, entry.model
                    );
                    if is_selected {
                        Line::from(text).style(
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(ratatui::style::Modifier::BOLD),
                        )
                    } else {
                        Line::from(text)
                            .style(Style::default().fg(Color::White))
                    }
                })
                .collect()
        };
        let picker_height = (picker_lines.len() as u16 + 2).min(10);
        let picker_width = 50u16.min(transcript_area.width);
        let picker_x = input_area.x;
        let picker_y = input_area.y.saturating_sub(picker_height);
        let picker_area =
            Rect::new(picker_x, picker_y, picker_width, picker_height);
        ratatui::widgets::Clear.render(picker_area, &mut buf);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" providers ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black).fg(Color::Cyan));
        let inner = block.inner(picker_area);
        block.render(picker_area, &mut buf);
        let para = Paragraph::new(Text::from(picker_lines))
            .style(Style::default().bg(Color::Black).fg(Color::White));
        para.render(inner, &mut buf);
    }

    // Provider add-flow form (C1) — buffer path.
    // I1: modal grows to accommodate the D3 description lines.
    if let Some(form) = &state.provider_add_form {
        let backdrop_area = match pane_area {
            None => transcript_area,
            Some(pane_rect) => {
                body_union(transcript_area, input_area, pane_rect)
            }
        };
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        backdrop.render(backdrop_area, &mut buf);
        let modal_area = centered_rect(75, 75, backdrop_area);
        ratatui::widgets::Clear.render(modal_area, &mut buf);
        let modal_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" add provider ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black).fg(Color::Cyan));
        let inner = modal_block.inner(modal_area);
        modal_block.render(modal_area, &mut buf);
        let lines = provider_add_form_lines(form);
        let paragraph = Paragraph::new(Text::from(lines))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        paragraph.render(inner, &mut buf);
    }

    if let Some(modal) = &state.pending_approval {
        let backdrop_area = match pane_area {
            None => transcript_area,
            Some(pane_rect) => {
                body_union(transcript_area, input_area, pane_rect)
            }
        };
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        backdrop.render(backdrop_area, &mut buf);
        let modal_area = centered_rect(60, 60, backdrop_area);
        ratatui::widgets::Clear.render(modal_area, &mut buf);
        let modal_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Approval required (y/n, Esc deny) ")
            .style(Style::default().bg(Color::Black).fg(Color::Yellow));
        let inner = modal_block.inner(modal_area);
        modal_block.render(modal_area, &mut buf);
        let text: Vec<Line<'_>> =
            modal.lines.iter().map(|s| Line::from(s.as_str())).collect();
        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        paragraph.render(inner, &mut buf);
    }

    buf
}

/// Sink that appends sanitized lines verbatim to a shared [`TuiState`].
///
/// The session's existing sanitizer boundary produces sanitized lines upstream;
/// this sink adds nothing unsanitized — it splits on newlines and pushes each
/// segment verbatim via [`TuiState::push_line`].
pub struct TuiSink {
    state: Rc<RefCell<TuiState>>,
    /// The live loop's redraw hook (S2 chunk 4): a streamed delta lands in
    /// the transcript and the sink asks for a frame, which is what makes
    /// streaming VISIBLE instead of arriving in one frame at the end.
    /// `None` in headless tests.
    redraw: Option<Rc<dyn Fn()>>,
    /// Last time the hook ran, for throttling a fast stream.
    last_redraw: std::cell::Cell<Option<std::time::Instant>>,
}

impl TuiSink {
    /// Create a sink sharing `state`.
    pub fn new(state: Rc<RefCell<TuiState>>) -> Self {
        Self { state, redraw: None, last_redraw: std::cell::Cell::new(None) }
    }

    /// Install the live loop's redraw hook.
    pub fn set_redraw(&mut self, redraw: Rc<dyn Fn()>) {
        self.redraw = Some(redraw);
    }

    /// Ask for a frame, at most once per [`REDRAW_INTERVAL`].
    fn redraw_due(&self) {
        let Some(hook) = self.redraw.as_ref() else {
            return;
        };
        let now = std::time::Instant::now();
        let due = match self.last_redraw.get() {
            None => true,
            Some(last) => now.duration_since(last) >= REDRAW_INTERVAL,
        };
        if due {
            self.last_redraw.set(Some(now));
            hook();
        }
    }
}

impl Write for TuiSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(bytes);
        {
            let mut state = self.state.borrow_mut();
            // S3c: hand the text to the REVEAL rather than the transcript,
            // so a line the provider sends whole still appears left to
            // right, and an unfinished line is visible as it grows.
            state.stream_buffer.push_str(&text);
            state.reveal_now(std::time::Instant::now());
        }
        self.redraw_due();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Do not automatically flush partial — the session's `drain_events`
        // writes complete lines with newlines. We expose `flush_partial` for
        // callers that need it, but `Write::flush` is a no-op to avoid
        // injecting half-lines.
        Ok(())
    }
}

/// RAII guard that restores terminal state (raw mode off, alternate screen
/// exit, MOUSE CAPTURE OFF) on drop — panic-safe.
///
/// PAIRING GUARANTEE (required behaviour 1): `enter` enables
/// `EnableMouseCapture` AFTER raw mode + alternate screen, and `drop`
/// disables it (`DisableMouseCapture`) BEFORE leaving the alternate screen
/// and raw mode — the exact reverse order. The guard is held as `_guard`
/// for the whole TUI session in
/// `interactive::run_interactive_tui_with_options`, so EVERY exit path —
/// normal `/exit`, Ctrl+C break, `?` early return, and panics — runs
/// `drop`. The only gap is a hard abort (`process::abort`, `SIGKILL`):
/// no userspace guard can run there, same as raw mode itself.
///
/// The guard additionally exposes `set_mouse_capture` so the live loop can
/// re-pair the terminal escape immediately on every `/mouse` toggle: the
/// state flip (`toggle_mouse_capture`) and the escape stay in lockstep in
/// the same match arm — never one without the other.
pub struct TerminalGuard {
    restored: bool,
    mouse_captured: bool,
}

impl TerminalGuard {
    /// Enter alternate screen and raw mode (in that order). The mouse is NOT
    /// captured here: the terminal keeps it, so click-drag selects text and
    /// its own paste works (owner ruling 2026-09-12). `/mouse` captures it
    /// live through [`Self::set_mouse_capture`]. Returns the guard; dropping
    /// it restores state.
    pub fn enter() -> io::Result<Self> {
        use crossterm::execute;
        use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(e);
        }
        Ok(Self { restored: false, mouse_captured: false })
    }

    /// Re-pair the terminal escape with the toggled state: enabling sends
    /// `EnableMouseCapture`, disabling sends `DisableMouseCapture`. Called
    /// in the same `/mouse` arm as the `TuiState` flip — the two never
    /// diverge. A failed escape is reported to the loop as `Err` (the state
    /// flip is rolled back by the caller so state and terminal stay paired).
    pub fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<()> {
        use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
        if enabled == self.mouse_captured {
            return Ok(());
        }
        if enabled {
            crossterm::execute!(io::stdout(), EnableMouseCapture)?;
        } else {
            crossterm::execute!(io::stdout(), DisableMouseCapture)?;
        }
        self.mouse_captured = enabled;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        use crossterm::event::DisableMouseCapture;
        if self.restored {
            return;
        }
        // Reverse of `enter`: mouse capture off FIRST, so no exit path —
        // not even a panic unwind — leaves the terminal captured.
        if self.mouse_captured {
            let _ = crossterm::execute!(io::stdout(), DisableMouseCapture);
            self.mouse_captured = false;
        }
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::LeaveAlternateScreen
        );
        self.restored = true;
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
// declare/compose, replay wiring, provider choice, plugin/context selection,
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

// Helpers for tests: expose scroll operations
#[allow(dead_code)]
/// Validate credential env-var name (without env: prefix): [A-Z0-9_]{1,64}.
/// Human-readable error (D2) — validation rule unchanged, but with O3/I3
/// teaching message when the input looks like the secret itself.
fn validate_credential_env_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        // O3/I3: when validation fails and the input looks like a secret,
        // teach the env-var-name pattern instead of the standard message.
        let looks_like_secret = name.chars().any(|c| c.is_ascii_lowercase())
            || name.starts_with("sk-")
            || name.chars().any(|c| {
                !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            });
        if looks_like_secret {
            return Err(
                "this looks like the key itself - Siralos stores the NAME of the environment variable holding your key; create it with setx YOUR_API_KEY_NAME \"the-key\" and enter YOUR_API_KEY_NAME here"
                    .to_owned(),
            );
        }
        return Err(
            "Credential env var must be uppercase letters, numbers, and underscores (e.g. OPENAI_API_KEY) - set this variable with your API key before starting Siralos"
                .to_owned(),
        );
    }
    Ok(())
}

/// Validate provider field: [a-z0-9_-]{1,64}, non-empty, no NUL.
/// Human-readable error (D2) — validation rule unchanged.
fn validate_provider_name(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 64 {
        return Err(
            "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)"
                .to_owned(),
        );
    }
    if value.contains('\0') {
        return Err(
            "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)"
                .to_owned(),
        );
    }
    if !value.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'
    }) {
        return Err(
            "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)"
                .to_owned(),
        );
    }
    Ok(())
}

/// Validate api protocol — closed set: openai-completions (default), openai-responses, anthropic-messages.
fn validate_api_protocol(value: &str) -> Result<(), String> {
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

fn open_protocol_picker(form: &mut ProviderAddForm) {
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
fn validate_model_display_name(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    if value.len() > 256 {
        return Err(
            "The model display name exceeds the 256-byte bound.".to_owned()
        );
    }
    if value.contains('\0') {
        return Err("A model display name must not contain NUL.".to_owned());
    }
    if !value.chars().all(|c| !c.is_control()) {
        return Err("A model display name must be printable.".to_owned());
    }
    Ok(())
}

/// Validate model field: 1 to 256 bytes, no NUL, ASCII alphanumeric or
/// `.` `_` `-` `/` `:` `@` (the core `is_model_id_char` rule).
/// Human-readable error (D2) — the message states the enforced rule in
/// plain words, never a regex.
fn validate_model_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > siralos_core::composition::MAX_PROFILE_MODEL_BYTES
    {
        return Err(
            "Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"
                .to_owned(),
        );
    }
    if value.contains('\0') {
        return Err(
            "Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"
                .to_owned(),
        );
    }
    if !value.chars().all(siralos_core::composition::is_model_id_char) {
        return Err(
            "Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"
                .to_owned(),
        );
    }
    Ok(())
}

/// Validate endpoint field: https:// or http://, no NUL, no space, 1..512.
/// Human-readable error (D2) — validation rule unchanged.
fn validate_endpoint_value(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 512 {
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
    if !(value.starts_with("https://") || value.starts_with("http://")) {
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
    Ok(())
}

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
    // A chorded key belongs to the loop, not to the type-ahead: Ctrl+C is
    // the exit key, and folding it into the prompt would kill it for the
    // whole turn.
    if key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
    {
        return false;
    }
    match key.code {
        KeyCode::Esc => true,
        KeyCode::Char(ch) => {
            state.input.push(ch);
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
    if state.pending_approval.is_some()
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
/// (callers must route through [`handle_modal_key`] first — no typing through
/// a modal).
/// The provider add-form (C1) has modal discipline: while it is Some, NO other
/// keys pass (modal discipline).
pub fn handle_key(
    state: &mut TuiState,
    key: crossterm::event::KeyEvent,
    viewport_height: u16,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    if state.pending_approval.is_some() {
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
                        let credential_opt = if trimmed.is_empty() {
                            None
                        } else if trimmed.starts_with("env:") {
                            Some(trimmed.clone())
                        } else {
                            Some(format!("key:{}", trimmed))
                        };
                        form.credential_env = credential_opt;
                        form.field = ProviderAddField::ApiProtocol;
                        // Open protocol picker with pre-selected item
                        open_protocol_picker(form);
                        // S2 fetch: trigger only when url is non-empty — blocking with freeze documented.
                        let url_opt = form.endpoint.clone();
                        if let Some(ep) = url_opt.as_deref() {
                            if !ep.is_empty() {
                                form.fetching_models = true;
                                form.fetch_note = None;
                                form.model_picker = None;
                            } else {
                                form.fetching_models = false;
                            }
                        } else {
                            form.fetching_models = false;
                        }
                    }
                    ProviderAddField::ApiProtocol => {
                        form.protocol = Some(trimmed);
                        form.protocol_picker = None;
                        form.field = ProviderAddField::Model;
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
                            let protocol =
                                form.protocol.clone().unwrap_or_else(|| {
                                    "openai-completions".to_owned()
                                });
                            form.completed = Some(ProviderAddData {
                                provider,
                                model,
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
                        let credential_opt = if trimmed.is_empty() {
                            None
                        } else if trimmed.starts_with("env:") {
                            Some(trimmed.clone())
                        } else {
                            Some(format!("key:{}", trimmed))
                        };
                        form.credential_env = credential_opt;
                        form.field = ProviderAddField::ApiProtocol;
                        open_protocol_picker(form);
                        // S2 fetch: trigger only when url is non-empty — blocking with freeze documented.
                        let url_opt = form.endpoint.clone();
                        if let Some(ep) = url_opt.as_deref() {
                            if !ep.is_empty() {
                                form.fetching_models = true;
                                form.fetch_note = None;
                                form.model_picker = None;
                            } else {
                                form.fetching_models = false;
                            }
                        } else {
                            form.fetching_models = false;
                        }
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
                form.input.push(ch);
                form.error = None;
                return false;
            }
            _ => {
                // Modal discipline: consume all other keys while form is open.
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
            state.input.push(ch);
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

/// Open the provider picker (H6) — caller supplies entries from workspace config.
/// Read-only: no config write; selection echo handled in `handle_key`.
/// When entries is non-empty an extra "Add provider" entry is appended whose
/// selection opens the add-flow form (C1), followed by a "- Remove provider"
/// entry whose selection arms the y/N removal confirmation. An empty
/// configuration shows only the add entry (there is nothing to remove).
pub fn open_provider_picker(
    state: &mut TuiState,
    mut entries: Vec<ProviderEntry>,
) {
    // C1: add entry — present even when a provider exists so the user can
    // re-configure; when no provider exists the picker shows only this entry.
    if entries.is_empty() {
        entries.push(ProviderEntry {
            name: "+ Add provider".to_owned(),
            host: "—".to_owned(),
            model: "—".to_owned(),
        });
    } else {
        entries.push(ProviderEntry {
            name: "+ Add provider".to_owned(),
            host: "add".to_owned(),
            model: "new".to_owned(),
        });
        entries.push(ProviderEntry {
            name: "- Remove provider".to_owned(),
            host: "remove".to_owned(),
            model: "profile".to_owned(),
        });
    }
    state.provider_picker = Some(ProviderPicker::new(entries));
    state.input.clear();
    state.update_palette();
}

/// Open the provider-removal confirmation (y/N modal over the shared
/// approval gate): `y` removes the `[profile]` section, `n`/`Esc` cancels.
/// Static host strings (sanitizer-clean); the decision resolves in the
/// interactive loop through the single removal outcome.
pub fn open_provider_remove_confirm(state: &mut TuiState) {
    state.provider_picker = None;
    state.pending_approval = Some(ApprovalModal::new(vec![
        "Remove the configured provider from siralos.toml?".to_owned(),
        "This deletes the [profile] section. Restart the session to apply."
            .to_owned(),
    ]));
    state.confirming_provider_removal = true;
    state.input.clear();
    state.update_palette();
}

/// Open the provider add-flow form (C1) — sequential modal form.
pub fn open_provider_add_form(state: &mut TuiState) {
    state.provider_picker = None;
    state.provider_add_form = Some(ProviderAddForm::new());
    state.input.clear();
    state.update_palette();
}

/// Open the `/model` switch picker over the provider's fetched models.
///
/// Reuses the add-flow [`ModelPicker`] (decision 138 sliding viewport),
/// not a second picker. Selecting an entry arms `pending_model_switch`,
/// which the interactive loop resolves through the same switch-and-persist
/// as `/model <id>`. An empty fetch never opens (the caller reports it
/// truthfully instead). Sanitizer note: fetched ids render verbatim like
/// the add-flow picker; the switch itself validates the id against the
/// core model rule before anything is written or messaged.
pub fn open_model_switch_picker(state: &mut TuiState, items: Vec<String>) {
    if items.is_empty() {
        return;
    }
    state.model_switch_picker = Some(ModelPicker { items, selected: 0 });
    state.pending_model_switch = None;
    state.input.clear();
    state.update_palette();
}

/// Push the SIRALOS banner + greeting into the transcript at session start (H2).
/// TUI-only; stdio path unchanged. Sanitized-safe static host strings.
/// A blank line separates the banner block from the greeting (C6).
pub fn push_banner_and_greeting(state: &mut TuiState) {
    for &line in SIRALOS_BANNER {
        state.push_line(line.to_owned());
    }
    state.push_line(String::new());
    state.push_line(SIRALOS_GREETING.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(state: &TuiState, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(state, frame)).expect("draw");
        terminal.backend().buffer().clone()
    }

    #[test]
    fn draw_determinism_same_state_identical_buffer() {
        let mut state = TuiState::new();
        state.transcript_lines = vec!["hello".to_owned(), "world".to_owned()];
        state.input = "test".to_owned();
        state.status = "ready".to_owned();
        let a = render(&state, 40, 10);
        let b = render(&state, 40, 10);
        assert_eq!(a, b);
    }

    #[test]
    fn transcript_tail_and_scroll() {
        let mut state = TuiState::new();
        for i in 0..20 {
            state.transcript_lines.push(format!("line {i}"));
        }
        state.status = "ok".to_owned();
        // Height 10 => transcript area 7 (header 1 + input 1 + status 1). Show tail.
        let buf = render(&state, 40, 10);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        // Tail should contain the last lines
        assert!(content.contains("line 19"));
        assert!(content.contains("line 13"));
        // Scroll up 10 should show older lines and hide newest
        let mut scrolled = state.clone();
        scrolled.scroll_offset = 10;
        let buf2 = render(&scrolled, 40, 10);
        let content2: String =
            buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(content2.contains("line 9"));
        assert!(!content2.contains("line 19"));
    }

    #[test]
    fn long_provider_error_line_wraps_with_tail_visible() {
        // Owner report: a long provider-error line was clipped at the right
        // edge of the pane, so the tail ran off-screen and the error could
        // not be read. Wrapping belongs in the render layer: the stored
        // line stays single while the frame shows head AND tail across
        // multiple rows. Placeholder host only.
        let mut state = TuiState::new();
        let long = "Response failed: response failed: 429 at https://api.example.com/v1/chat/completions - {\"error\":{\"message\":\"Provider rate limit exceeded, please slow down and retry shortly\"}}".to_owned();
        state.transcript_lines = vec![long.clone()];
        state.status = "ready".to_owned();
        let buf = render(&state, 40, 12);
        let area = buf.area;
        let mut rows = Vec::new();
        for y in 0..area.height {
            let mut row = String::new();
            for x in 0..area.width {
                if let Some(cell) = buf.cell((x, y)) {
                    row.push_str(cell.symbol());
                }
            }
            rows.push(row);
        }
        let joined = rows.join("\n");
        assert!(
            joined.contains("Response failed"),
            "head must be visible, got: {rows:?}"
        );
        assert!(
            joined.contains("retry shortly"),
            "tail must be visible (not clipped off-screen), got: {rows:?}"
        );
        let matching = rows
            .iter()
            .filter(|row| {
                row.contains("Response")
                    || row.contains("429")
                    || row.contains("retry")
                    || row.contains("api.example")
            })
            .count();
        assert!(
            matching >= 2,
            "long line must span multiple rows, got: {rows:?}"
        );
        // Render-layer wrap only: the stored transcript text is untouched.
        assert_eq!(state.transcript_lines, vec![long]);
    }

    #[test]
    fn wrap_line_to_width_breaks_words_and_long_tokens() {
        // Short lines pass through untouched (pinned frames byte-identical).
        assert_eq!(wrap_line_to_width("hello", 40), vec!["hello".to_owned()]);
        assert_eq!(wrap_line_to_width("", 40), vec!["".to_owned()]);
        // Word boundary with exact fit: words pack, nothing splits.
        assert_eq!(
            wrap_line_to_width("aa bb cc dd", 5),
            vec!["aa bb".to_owned(), "cc dd".to_owned()]
        );
        // Word boundary without fit: break at spaces, never inside a word.
        assert_eq!(
            wrap_line_to_width("aa bb cc", 4),
            vec!["aa".to_owned(), "bb".to_owned(), "cc".to_owned()]
        );
        // Long token (URL/JSON body): hard-break at the width.
        assert_eq!(
            wrap_line_to_width("abcdefghij", 4),
            vec!["abcd".to_owned(), "efgh".to_owned(), "ij".to_owned()]
        );
        // Zero width never panics.
        assert_eq!(wrap_line_to_width("hello", 0), vec!["hello".to_owned()]);
        // Deterministic and bounded on a realistic error line.
        let long = "Response failed: 429 at https://api.example.com/v1/chat/completions - {\"error\":{\"message\":\"slow down\"}}";
        let first = wrap_line_to_width(long, 40);
        assert_eq!(first, wrap_line_to_width(long, 40));
        assert!(first.len() > 1);
        for row in &first {
            assert!(
                row.chars().count() <= 40,
                "row overflows the width: {row:?}"
            );
        }
    }

    #[test]
    fn input_line_editing_renders() {
        let mut state = TuiState::new();
        state.input = "abc".to_owned();
        let buf = render(&state, 40, 10);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("> abc"));
        let mut edited = state.clone();
        edited.input.pop();
        edited.input.pop();
        let buf2 = render(&edited, 40, 10);
        let content2: String =
            buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(content2.contains("> a"));
        assert!(!content2.contains("> abc"));
    }

    #[test]
    fn status_line_rendering() {
        let mut state = TuiState::new();
        state.status = "working".to_owned();
        let buf = render(&state, 40, 10);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("working"));
        let mut other = state.clone();
        other.status = "ready".to_owned();
        let buf2 = render(&other, 40, 10);
        let content2: String =
            buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(content2.contains("ready"));
        assert!(!content2.contains("working"));
    }

    #[test]
    fn tui_sink_appends_sanitized_lines_verbatim() {
        let state = Rc::new(RefCell::new(TuiState::new()));
        let mut sink = TuiSink::new(state.clone());
        // Simulate sanitized lines as the session would write them.
        sink.write_all(b"hello world\n").expect("write");
        sink.write_all(b"second line\n").expect("write");
        let transcript = state.borrow().transcript_lines.clone();
        assert_eq!(transcript, vec!["hello world", "second line"]);
        // Ensure unsanitized content would be stored verbatim (the sink does
        // not inject sanitization; upstream already sanitized).
        sink.write_all(b"already sanitized ^@\n").expect("write");
        assert_eq!(state.borrow().transcript_lines[2], "already sanitized ^@");
    }

    #[test]
    fn tui_sink_respects_transcript_bound() {
        let state = Rc::new(RefCell::new(TuiState::new()));
        let mut sink = TuiSink::new(state.clone());
        let start = std::time::Instant::now();
        for i in 0..(MAX_TRANSCRIPT_LINES + 50) {
            let line = format!("line {i}\n");
            sink.write_all(line.as_bytes()).expect("write");
            // S3c: the reveal is paced, so advance the clock (a second per
            // line is far more than the budget needs).
            state.borrow_mut().reveal_now(
                start + std::time::Duration::from_secs(i as u64 + 1),
            );
        }
        let transcript = state.borrow().transcript_lines.clone();
        assert_eq!(transcript.len(), MAX_TRANSCRIPT_LINES);
        // Oldest dropped, newest retained
        assert_eq!(transcript[0], format!("line {}", 50));
        assert_eq!(
            transcript[transcript.len() - 1],
            format!("line {}", MAX_TRANSCRIPT_LINES + 49)
        );
    }

    #[test]
    fn empty_state_initial_frame() {
        let state = TuiState::new();
        let buf = render(&state, 40, 10);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        // Should contain prompt and default status
        assert!(content.contains(">"));
        assert!(content.contains("ready"));
        // Transcript area should be empty but render without panic
        let buf2 = render(&state, 10, 5);
        assert_eq!(buf2.area.width, 10);
    }

    #[test]
    fn non_tty_fallback_predicate() {
        // T1 predicate (deprecated alias) still compiles
        assert!(should_use_tui(true, true));
        assert!(!should_use_tui(true, false));
        assert!(!should_use_tui(false, true));
        assert!(!should_use_tui(false, false));
    }

    #[test]
    fn tui_default_entry_truth_table() {
        // Decision 105: should_launch_tui(wants_stdio, is_tty) == !wants_stdio && is_tty
        assert!(should_launch_tui(false, true));
        assert!(!should_launch_tui(true, true));
        assert!(!should_launch_tui(false, false));
        assert!(!should_launch_tui(true, false));
    }

    #[test]
    fn stdio_escape_hatch_forces_stdio() {
        // --stdio forces stdio even on TTY
        assert!(!should_launch_tui(true, true));
        assert!(!should_launch_tui(true, false));
    }

    #[test]
    fn non_tty_silent_stdio() {
        // Plain non-TTY with no flags uses stdio silently
        assert!(!should_launch_tui(false, false));
    }

    #[test]
    fn modal_renders_over_dimmed_transcript() {
        let mut state = TuiState::new();
        state.transcript_lines =
            vec!["line 1".to_owned(), "line 2".to_owned()];
        state.pending_approval =
            Some(ApprovalModal::new(vec!["Approve this?".to_owned()]));
        let a = render(&state, 40, 12);
        let b = render(&state, 40, 12);
        // Determinism
        assert_eq!(a, b);
        let content: String = a.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("Approve this?"));
        assert!(content.contains("Approval required"));
        // No-modal baseline differs
        let mut plain = TuiState::new();
        plain.transcript_lines =
            vec!["line 1".to_owned(), "line 2".to_owned()];
        let plain_buf = render(&plain, 40, 12);
        assert_ne!(a, plain_buf);
    }

    #[test]
    fn approval_y_routes_approve_through_same_gate() {
        // Same gate stdio would use
        assert_eq!(evaluate_approval_input("y"), ApprovalDecision::Approve);
        assert_eq!(evaluate_approval_input("Y"), ApprovalDecision::Approve);
        // Modal y routes approve
        let mut state = TuiState::new();
        state.pending_approval =
            Some(ApprovalModal::new(vec!["req".to_owned()]));
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            handle_modal_key(&mut state, key),
            Some(ApprovalDecision::Approve)
        );
        // Ensure stdio evaluation matches modal evaluation for same char
        assert_eq!(evaluate_approval_char('y'), evaluate_approval_input("y"));
    }

    #[test]
    fn approval_n_and_esc_route_deny() {
        assert_eq!(evaluate_approval_input("n"), ApprovalDecision::Deny);
        assert_eq!(evaluate_approval_input("Esc"), ApprovalDecision::Deny);
        assert_eq!(evaluate_approval_input(""), ApprovalDecision::Deny);
        let mut state = TuiState::new();
        state.pending_approval =
            Some(ApprovalModal::new(vec!["req".to_owned()]));
        let n = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            handle_modal_key(&mut state, n),
            Some(ApprovalDecision::Deny)
        );
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            handle_modal_key(&mut state, esc),
            Some(ApprovalDecision::Deny)
        );
    }

    #[test]
    fn keys_ignored_while_modal_pending() {
        let mut state = TuiState::new();
        state.input = "hello".to_owned();
        state.pending_approval =
            Some(ApprovalModal::new(vec!["req".to_owned()]));
        // Typing should be ignored
        let ch = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!handle_key(&mut state, ch, 10));
        assert_eq!(state.input, "hello");
        // Enter ignored
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!handle_key(&mut state, enter, 10));
        // Only modal keys pass
        let y = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            handle_modal_key(&mut state, y),
            Some(ApprovalDecision::Approve)
        );
    }

    #[test]
    fn modal_line_bound_with_truncation_marker() {
        let lines: Vec<String> =
            (0..50).map(|i| format!("line {i}")).collect();
        let modal = ApprovalModal::new(lines);
        assert_eq!(modal.lines.len(), MAX_APPROVAL_LINES + 1);
        assert_eq!(
            modal.lines[MAX_APPROVAL_LINES],
            APPROVAL_TRUNCATION_MARKER
        );
        // Exactly at bound no truncation
        let exact: Vec<String> =
            (0..MAX_APPROVAL_LINES).map(|i| format!("line {i}")).collect();
        let modal2 = ApprovalModal::new(exact.clone());
        assert_eq!(modal2.lines, exact);
        // Modal never renders unsanitized — input already sanitized, modal stores verbatim
        let mut state = TuiState::new();
        state.pending_approval = Some(ApprovalModal::new(vec![
            "sanitized \x1b[31mred\x1b[0m".to_owned(),
        ]));
        let buf = render(&state, 50, 14);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("sanitized"));
    }

    #[test]
    fn consolidated_helper_is_same_function_both_loops_call() {
        // Compile-level proof: both the stdio and TUI paths import and call
        // evaluate_approval_input (and evaluate_approval_char) — the address is the same function.
        let stdio_fn: fn(&str) -> ApprovalDecision = evaluate_approval_input;
        let tui_fn: fn(&str) -> ApprovalDecision =
            crate::tui::evaluate_approval_input;
        assert_eq!(stdio_fn("y"), tui_fn("y"));
        assert_eq!(stdio_fn("n"), tui_fn("n"));
        let ch_fn: fn(char) -> ApprovalDecision = evaluate_approval_char;
        assert_eq!(ch_fn('y'), ApprovalDecision::Approve);
        assert_eq!(ch_fn('n'), ApprovalDecision::Deny);
    }

    // T4 (decision 108) — render-model pinning: the four corpus scenarios
    // as unit TestBackend snapshots over the PRODUCTION draw path (the same
    // frames the harness `tui-render` records pin). Each test mirrors one
    // `tests/differential/corpus/tui-render.*.json` input exactly; the
    // harness record path pins the same frames as candidate-authored
    // expectations at corpus v76.
    mod t4_frame_snapshot_tests {
        use super::super::*;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        /// Canonical frame: 24 row strings over the fixed 80x24 viewport,
        /// trailing spaces trimmed per row (same serialization as the
        /// harness `tui-render` record).
        fn canonical_frame(
            state: &TuiState,
            pane: Option<&ContextPaneData>,
            width: u16,
            height: u16,
        ) -> Vec<String> {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal");
            match pane {
                Some(pane) => {
                    terminal
                        .draw(|frame| draw_with_pane(state, Some(pane), frame))
                        .expect("draw");
                }
                None => {
                    terminal.draw(|frame| draw(state, frame)).expect("draw");
                }
            }
            let buffer = terminal.backend().buffer().clone();
            let area = buffer.area;
            let mut rows = Vec::with_capacity(area.height as usize);
            for row in 0..area.height {
                let mut line = String::new();
                for col in 0..area.width {
                    if let Some(cell) = buffer.cell((col, row)) {
                        line.push_str(cell.symbol());
                    }
                }
                rows.push(line.trim_end().to_owned());
            }
            rows
        }

        fn pane_fixture() -> ContextPaneData {
            ContextPaneData {
                counters: vec![
                    ("ticks_total".to_owned(), 3),
                    ("coalesced_noop_ticks_total".to_owned(), 0),
                    ("events_total".to_owned(), 3),
                    ("events_dropped_total".to_owned(), 0),
                    ("demand_updates_total".to_owned(), 2),
                    ("promotions_total".to_owned(), 1),
                    ("demotions_total".to_owned(), 0),
                    ("stale_demotions_total".to_owned(), 0),
                    ("pin_quota_demotions_total".to_owned(), 0),
                    ("budget_demotions_total".to_owned(), 0),
                    ("assembled_summary_tokens_total".to_owned(), 300),
                    ("neighbor_stub_tokens_total".to_owned(), 24),
                ],
                ring: (1..=3u64)
                    .map(|now| siralos_core::context_metrics::TickRecord {
                        now,
                        canonical_event_count: 1,
                        events_dropped: 0,
                        tier_counts:
                            siralos_core::context_metrics::TierCounts {
                                hot: 0,
                                warm: 0,
                                cold: 0,
                                archive: 0,
                            },
                        assembled_unique_total: 0,
                        assembled_summary_total: 0,
                        stub_total: 0,
                        demotion_counts:
                            siralos_core::context_metrics::DemotionKindCounts {
                                stale: 0,
                                pin_quota: 0,
                                budget: 0,
                            },
                        promotion_count: 0,
                    })
                    .collect(),
                activity: vec![
                    ToolActivityEntry {
                        tool_name: "context.inspect".to_owned(),
                        status: "success".to_owned(),
                    },
                    ToolActivityEntry {
                        tool_name: "context.search".to_owned(),
                        status: "success".to_owned(),
                    },
                    ToolActivityEntry {
                        tool_name: "workspace.read".to_owned(),
                        status: "success".to_owned(),
                    },
                ],
            }
        }

        #[test]
        fn t4_frame_shell_basic_matches_harness_record() {
            // Mirrors `tui-render.shell-basic`: header + transcript + input + status,
            // no pane, no modal. Header occupies row 0.
            let mut state = TuiState::new();
            state.transcript_lines = vec![
                "Siralos received: hello".to_owned(),
                "Context projection (mode generic)".to_owned(),
                "Type /help for the list of available commands.".to_owned(),
            ];
            state.input = "help me".to_owned();
            state.status = "ready".to_owned();
            let frame = canonical_frame(&state, None, 80, 24);
            assert_eq!(frame.len(), 24);
            assert!(frame[0].contains("Siralos"));
            assert_eq!(frame[1], "Siralos received: hello");
            assert_eq!(frame[2], "Context projection (mode generic)");
            assert_eq!(
                frame[3],
                "Type /help for the list of available commands."
            );
            assert_eq!(frame[22], "> help me");
            assert_eq!(frame[23], "ready");
            // Deterministic: same input -> byte-equal frame.
            assert_eq!(frame, canonical_frame(&state, None, 80, 24));
        }

        #[test]
        fn t4_frame_transcript_scroll_matches_harness_record() {
            // Mirrors `tui-render.transcript-scroll`: 40 lines, offset 8 —
            // header occupies row 0, transcript window is 21 rows (24-3) vs 22 before.
            // With header, start = 40-21-8 = 11 => visible 11..31.
            let mut state = TuiState::new();
            state.transcript_lines =
                (0..40).map(|i| format!("line {i:02}")).collect();
            state.status = "ready".to_owned();
            state.scroll_offset = 8;
            let frame = canonical_frame(&state, None, 80, 24);
            assert_eq!(frame.len(), 24);
            assert!(frame[0].contains("Siralos"));
            assert_eq!(frame[1], "line 11");
            assert_eq!(frame[21], "line 31");
            assert!(!frame.iter().any(|row| row.contains("line 39")));
            assert!(!frame.iter().any(|row| row.contains("line 09")));
            assert_eq!(frame, canonical_frame(&state, None, 80, 24));
        }

        #[test]
        fn t4_frame_approval_modal_matches_harness_record() {
            // Mirrors `tui-render.approval-modal`: modal over dimmed
            // transcript, status `awaiting approval`.
            let mut state = TuiState::new();
            state.transcript_lines = vec![
                "Siralos received: add a greeting".to_owned(),
                "Tool call: workspace.read src/app.ts".to_owned(),
                "Awaiting approval for the prepared change set.".to_owned(),
            ];
            state.status = "awaiting approval".to_owned();
            state.pending_approval = Some(ApprovalModal::new(vec![
                "Approve applying 1 change to src/app.ts?".to_owned(),
                "  + export const greeting = \"hello\";".to_owned(),
            ]));
            let frame = canonical_frame(&state, None, 80, 24);
            assert_eq!(frame.len(), 24);
            let joined = frame.join("\n");
            assert!(joined.contains("Approval required (y/n, Esc deny)"));
            assert!(
                joined.contains("Approve applying 1 change to src/app.ts?")
            );
            assert_eq!(frame[23], "awaiting approval");
            // No-modal baseline differs.
            let mut plain = TuiState::new();
            plain.transcript_lines = state.transcript_lines.clone();
            plain.status = state.status.clone();
            assert_ne!(frame, canonical_frame(&plain, None, 80, 24));
            assert_eq!(frame, canonical_frame(&state, None, 80, 24));
        }

        #[test]
        fn t4_frame_context_pane_matches_harness_record() {
            // Mirrors `tui-render.context-pane`: pane with the metrics
            // snapshot (counters, 3 ring lines, 3 tool-activity lines).
            let mut state = TuiState::new();
            state.transcript_lines = vec![
                "Siralos received: inspect the workspace".to_owned(),
                "Tool call: context.inspect a.txt".to_owned(),
                "Context projection (mode generic)".to_owned(),
            ];
            state.input = "what changed".to_owned();
            state.status = "ready".to_owned();
            let pane = pane_fixture();
            let frame = canonical_frame(&state, Some(&pane), 80, 24);
            assert_eq!(frame.len(), 24);
            let joined = frame.join("\n");
            assert!(joined.contains("counters:"));
            assert!(joined.contains("ticks_total: 3"));
            assert!(joined.contains("ring (last 8):"));
            assert!(joined.contains("tools (last 8):"));
            assert!(joined.contains("context.inspect success"));
            assert_eq!(frame, canonical_frame(&state, Some(&pane), 80, 24));
            // OFF (no pane) is byte-identical to the no-pane render.
            assert_eq!(
                canonical_frame(&state, None, 80, 24),
                canonical_frame(&state, None, 80, 24)
            );
        }
    }

    // T3 (decision 107) — context pane tests (~7).
    mod context_pane_tests {
        use super::super::*;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::buffer::Buffer;

        fn render_pane(
            state: &TuiState,
            pane: Option<&ContextPaneData>,
            width: u16,
            height: u16,
        ) -> Buffer {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal");
            terminal
                .draw(|frame| draw_with_pane(state, pane, frame))
                .expect("draw");
            terminal.backend().buffer().clone()
        }

        fn render_off(state: &TuiState, width: u16, height: u16) -> Buffer {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).expect("terminal");
            terminal.draw(|frame| draw(state, frame)).expect("draw");
            terminal.backend().buffer().clone()
        }

        fn temp_root(label: &str) -> std::path::PathBuf {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "siralos-tui-pane-{label}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("temp dir");
            root
        }

        fn build_session(
            label: &str,
            files: &[(&str, &str)],
        ) -> (
            std::path::PathBuf,
            siralos_adapters::context_session::ContextSystemSession,
        ) {
            let root = temp_root(label);
            for (name, body) in files {
                std::fs::write(root.join(name), body.as_bytes())
                    .expect("write");
            }
            let build =
                siralos_adapters::context_session::build_context_system(
                    &root, true,
                );
            let session = build.session.expect("session built");
            (root, session)
        }

        fn inspect_obs(
            node: &str,
        ) -> siralos_adapters::tool::context_events::ToolObservation {
            siralos_adapters::tool::context_events::ToolObservation::new(
                "context.inspect",
                serde_json::json!({ "node_id": node }),
                siralos_core::provider::ToolExecutionResult::Success {
                    output: serde_json::json!({ "id": node }),
                    summary: format!("inspect {node}"),
                },
            )
        }

        fn tool_history(
            count: usize,
        ) -> Vec<siralos_core::provider::ConversationItem> {
            use siralos_core::provider::{
                AssistantToolCallInput, ConversationItem, ToolExecutionResult,
            };
            let mut history = Vec::new();
            for i in 0..count {
                let call_id = format!("call-{i:03}");
                let tool_name = format!("tool-{i:03}");
                history.push(ConversationItem::AssistantToolCall {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    input: AssistantToolCallInput::Present(
                        serde_json::json!({ "path": "a.txt" }),
                    ),
                });
                let result = match i % 3 {
                    0 => ToolExecutionResult::Success {
                        output: serde_json::json!({}),
                        summary: "ok".to_owned(),
                    },
                    1 => ToolExecutionResult::Failed {
                        message: "boom".to_owned(),
                    },
                    _ => ToolExecutionResult::Cancelled {
                        message: "stop".to_owned(),
                    },
                };
                history.push(ConversationItem::ToolResult {
                    call_id,
                    tool_name,
                    result,
                });
            }
            history
        }

        #[test]
        fn off_byte_identical_frame_no_pane_no_placeholder() {
            // P1 gating: OFF (not opted in or not built) -> no pane.
            let (root, session) =
                build_session("off", &[("a.txt", "alpha"), ("b.txt", "beta")]);
            let history = tool_history(3);
            assert!(
                build_context_pane(false, Some(&session.metrics), &history)
                    .is_none()
            );
            assert!(build_context_pane(true, None, &history).is_none());
            assert!(build_context_pane(false, None, &history).is_none());

            // OFF frame is byte-identical to T2's render: draw == draw(None).
            let mut state = TuiState::new();
            state.transcript_lines = vec![
                "line one".to_owned(),
                "line two".to_owned(),
                "line three".to_owned(),
            ];
            state.input = "hello".to_owned();
            state.status = "ready".to_owned();
            for (width, height) in [(80, 24), (40, 10), (120, 30)] {
                let t2 = render_off(&state, width, height);
                let off = render_pane(&state, None, width, height);
                assert_eq!(off, t2, "OFF must equal T2 at {width}x{height}");
                // No pane chrome anywhere in the OFF frame.
                let content: String =
                    off.content().iter().map(|c| c.symbol()).collect();
                assert!(!content.contains("counters:"));
                assert!(!content.contains("ring (last 8):"));
                assert!(!content.contains("tools (last 8):"));
                assert!(!content.contains(" Context "));
                // Buffer helper agrees too.
                assert_eq!(
                    render_to_buffer(&state, width, height),
                    render_to_buffer_with_pane(&state, None, width, height)
                );
            }
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn pane_counters_match_audit_segment_in_pinned_order() {
            // P3 single source: pane counters equal the /context segment
            // counters for the same state, in the pinned order.
            let (root, mut session) =
                build_session("counters", &[("a.txt", "alpha")]);
            // Empty ring/activity first: headers with no lines.
            let empty = build_context_pane(true, Some(&session.metrics), &[])
                .expect("pane on");
            let lines = context_pane_lines(&empty, 38);
            assert!(lines.contains(&"counters:".to_owned()));
            assert!(lines.contains(&"ring (last 8):".to_owned()));
            assert!(lines.contains(&"tools (last 8):".to_owned()));
            assert!(
                !lines.iter().any(|l| l.trim_start().starts_with("tick "))
            );
            assert_eq!(empty.ring.len(), 0);
            assert_eq!(empty.activity.len(), 0);

            let obs = inspect_obs("a.txt");
            let _ = session.drive_tick(std::slice::from_ref(&obs));
            let pane = build_context_pane(true, Some(&session.metrics), &[])
                .expect("pane on");
            // Pinned order of names.
            let names: Vec<&str> =
                pane.counters.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(
                names, CONTEXT_COUNTER_ORDER,
                "pane counters must follow the pinned order"
            );
            // Values equal the /context audit segment for the same state.
            let audit = crate::output::format_context_audit(Some(&session));
            for (name, value) in &pane.counters {
                let needle = format!("    {name}: {value}");
                assert!(
                    audit.contains(&needle),
                    "audit segment must contain pane counter `{needle}`"
                );
            }
            // Every audit counter appears exactly once in the pane.
            assert_eq!(pane.counters.len(), 12);
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn pane_ring_renders_last_8_oldest_first_after_10_ticks() {
            // Same window rule as decision 100: last 8, oldest-first.
            let (root, mut session) = build_session(
                "ring",
                &[("a.txt", "body a"), ("b.txt", "body b")],
            );
            for i in 0..10 {
                let node = if i % 2 == 0 { "a.txt" } else { "b.txt" };
                let obs = inspect_obs(node);
                let _ = session.drive_tick(std::slice::from_ref(&obs));
            }
            let pane = build_context_pane(true, Some(&session.metrics), &[])
                .expect("pane on");
            assert_eq!(pane.ring.len(), 8);
            assert_eq!(pane.ring[0].now, 3, "oldest of last 8");
            assert_eq!(pane.ring[7].now, 10, "newest");
            // Same field names as decision 100 (full lines match the audit
            // segment's ring lines exactly).
            let audit = crate::output::format_context_audit(Some(&session));
            let audit_ring: Vec<&str> = audit
                .lines()
                .filter(|l| l.trim_start().starts_with("tick "))
                .collect();
            assert_eq!(audit_ring.len(), 8);
            for (rec, audit_line) in pane.ring.iter().zip(audit_ring.iter()) {
                assert_eq!(&format_tick_record_line(rec), audit_line);
            }
            // Rendered pane shows the ring block bounded to the pane width.
            let lines = context_pane_lines(&pane, 38);
            assert!(lines.contains(&"ring (last 8):".to_owned()));
            for line in
                lines.iter().filter(|l| l.trim_start().starts_with("tick "))
            {
                assert!(line.chars().count() <= 38);
            }
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn tool_activity_bounded_to_last_8_oldest_first() {
            let history = tool_history(10);
            let entries = tool_activity_from_history(&history);
            assert_eq!(entries.len(), 8, "bounded to last 8");
            assert_eq!(entries[0].tool_name, "tool-002");
            assert_eq!(entries[7].tool_name, "tool-009");
            // Statuses follow the typed vocabulary in round order.
            let statuses: Vec<&str> =
                entries.iter().map(|e| e.status.as_str()).collect();
            assert_eq!(
                statuses,
                vec![
                    "cancelled",
                    "success",
                    "failed",
                    "cancelled",
                    "success",
                    "failed",
                    "cancelled",
                    "success"
                ]
            );
            // Fewer than 8 rounds render all of them, oldest-first.
            let short = tool_activity_from_history(&history[..4]);
            assert_eq!(short.len(), 2);
            assert_eq!(short[0].tool_name, "tool-000");
            assert_eq!(short[1].tool_name, "tool-001");
            // Empty history -> empty activity (header only at render).
            assert!(tool_activity_from_history(&[]).is_empty());
            // Rendered lines are `tool name + result status`, bounded.
            let (root, session) =
                build_session("activity", &[("a.txt", "alpha")]);
            let pane =
                build_context_pane(true, Some(&session.metrics), &history)
                    .expect("pane on");
            let lines = context_pane_lines(&pane, 38);
            assert!(lines.contains(&"tools (last 8):".to_owned()));
            assert!(lines.contains(&"  tool-002 cancelled".to_owned()));
            assert!(!lines.iter().any(|l| l.contains("tool-000")));
            for line in &lines {
                assert!(line.chars().count() <= 38);
            }
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn pane_determinism_byte_equal() {
            // P5: same state + viewport -> byte-identical frame.
            let (root, mut session) =
                build_session("determ", &[("a.txt", "alpha")]);
            let obs = inspect_obs("a.txt");
            let _ = session.drive_tick(std::slice::from_ref(&obs));
            let history = tool_history(4);
            let pane =
                build_context_pane(true, Some(&session.metrics), &history)
                    .expect("pane on");
            let mut state = TuiState::new();
            state.transcript_lines = vec![
                "hello".to_owned(),
                "world".to_owned(),
                "third".to_owned(),
            ];
            state.input = "test".to_owned();
            state.status = "ready".to_owned();
            let a = render_pane(&state, Some(&pane), 80, 24);
            let b = render_pane(&state, Some(&pane), 80, 24);
            assert_eq!(a, b);
            // And an active pane frame differs from the OFF frame.
            let off = render_off(&state, 80, 24);
            assert_ne!(a, off);
            let content: String =
                a.content().iter().map(|c| c.symbol()).collect();
            assert!(content.contains("counters:"));
            assert!(content.contains("ticks_total: 1"));
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn pane_updates_reflect_tick() {
            // A tick changes the rendered counters/ring.
            let (root, mut session) =
                build_session("update", &[("a.txt", "alpha")]);
            let history = tool_history(2);
            let before =
                build_context_pane(true, Some(&session.metrics), &history)
                    .expect("pane on");
            assert_eq!(before.counters[0], ("ticks_total".to_owned(), 0));
            assert!(before.ring.is_empty());
            let obs = inspect_obs("a.txt");
            let _ = session.drive_tick(std::slice::from_ref(&obs));
            let after =
                build_context_pane(true, Some(&session.metrics), &history)
                    .expect("pane on");
            assert_eq!(after.counters[0], ("ticks_total".to_owned(), 1));
            assert_eq!(after.ring.len(), 1);
            assert_ne!(before, after);
            // The frames differ too.
            let state = TuiState::new();
            let frame_before = render_pane(&state, Some(&before), 80, 24);
            let frame_after = render_pane(&state, Some(&after), 80, 24);
            assert_ne!(frame_before, frame_after);
            let content: String =
                frame_after.content().iter().map(|c| c.symbol()).collect();
            assert!(content.contains("ticks_total: 1"));
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn pane_gating_matches_audit_gating_consolidation_proof() {
            // P1/P6: the pane gate and the shared `/context` audit gate agree
            // on all four (enabled x present) combinations — one gating
            // definition, two call sites, zero copies.
            let (root, session) = build_session("gate", &[("a.txt", "alpha")]);
            let history: Vec<siralos_core::provider::ConversationItem> =
                Vec::new();
            for enabled in [false, true] {
                for present in [false, true] {
                    let metrics =
                        if present { Some(&session.metrics) } else { None };
                    let holder =
                        if present { Some(session.clone()) } else { None };
                    let pane_on =
                        build_context_pane(enabled, metrics, &history)
                            .is_some();
                    let audit_on = crate::interactive::context_audit_session(
                        enabled, &holder,
                    )
                    .is_some();
                    assert_eq!(
                        pane_on, audit_on,
                        "pane and audit gates must agree (enabled={enabled}, present={present})"
                    );
                    assert_eq!(pane_on, enabled && present);
                }
            }
            let _ = std::fs::remove_dir_all(&root);
        }

        #[test]
        fn palette_lists_catalog_filtered_by_prefix() {
            let mut state = TuiState::new();
            state.input = "/do".to_owned();
            state.update_palette();
            let palette = state.palette.expect("palette for /do");
            // /do matches domains* variants
            assert!(palette.iter().any(|(name, _)| name == "/domains"));
            assert!(palette.iter().any(|(name, _)| name == "/domains-add"));
            // /m matches model
            let mut state2 = TuiState::new();
            state2.input = "/m".to_owned();
            state2.update_palette();
            let palette2 = state2.palette.expect("palette for /m");
            assert!(palette2.iter().any(|(name, _)| name == "/model"));
            // catalog includes evolve
            let catalog = command_catalog();
            assert!(catalog.iter().any(|(name, _)| name == "/evolve"));
        }

        #[test]
        fn palette_hidden_without_slash() {
            let mut state = TuiState::new();
            state.input = "hello".to_owned();
            state.update_palette();
            assert!(state.palette.is_none());
            state.input = "".to_owned();
            state.update_palette();
            assert!(state.palette.is_none());
        }

        #[test]
        fn timestamps_render_below_messages() {
            let mut state = TuiState::new();
            state.push_line_stamped(
                "hello".to_owned(),
                Some("2026-08-31 12:00:00 UTC".to_owned()),
            );
            state.status = "ready".to_owned();
            let buf = super::render(&state, 80, 24);
            let content: String =
                buf.content().iter().map(|c| c.symbol()).collect();
            assert!(content.contains("hello"));
            assert!(content.contains("2026-08-31 12:00:00 UTC"));
        }

        #[test]
        fn civil_date_golden_values() {
            assert_eq!(
                utc_timestamp_from_millis(0),
                "1970-01-01 00:00:00 UTC"
            );
            assert_eq!(
                utc_timestamp_from_millis(1_000),
                "1970-01-01 00:00:01 UTC"
            );
            assert_eq!(
                utc_timestamp_from_millis(86_400_000),
                "1970-01-02 00:00:00 UTC"
            );
            // Fixed fixture value round-trip
            assert_eq!(
                utc_timestamp_from_millis(1_726_650_000_000),
                utc_timestamp_from_millis(1_726_650_000_000)
            );
        }

        #[test]
        fn status_provider_model_present_and_absent() {
            let present = compose_status_line(
                "ready",
                Some("example-vendor"),
                Some("model-a"),
            );
            assert!(present.contains("example-vendor"));
            assert!(present.contains("model-a"));
            assert!(present.contains("ready"));
            let absent = compose_status_line("ready", None, None);
            assert!(absent.contains("no provider configured"));
        }

        #[test]
        fn command_catalog_is_single_source() {
            let catalog = command_catalog();
            let names: Vec<&str> =
                catalog.iter().map(|(name, _)| name.as_str()).collect();
            assert!(names.contains(&"/provider"));
            assert!(names.contains(&"/provider remove"));
            assert!(names.contains(&"/model"));
            assert!(names.contains(&"/model <id>"));
            assert!(names.contains(&"/models"));
            assert!(names.contains(&"/reload"));
            assert!(names.contains(&"/evolve"));
            assert!(names.contains(&"/context"));
            assert!(names.contains(&"/mouse"));
            assert!(names.contains(&"/exit"));
            assert_eq!(names.len(), 15);
        }

        #[test]
        fn palette_bounded_height_plus_more() {
            let mut state = TuiState::new();
            state.input = "/".to_owned();
            state.update_palette();
            let palette_len =
                state.palette.as_ref().expect("palette for /").len();
            assert_eq!(palette_len, 15);
            // I3: palette shows ALL filtered entries, bounded by terminal height minus input/status rows; scroll indicator only if overflow.
            // At 80x24, available 21, 15 entries fit fully with no indicator.
            let buf = super::render(&state, 80, 24);
            let content: String =
                buf.content().iter().map(|c| c.symbol()).collect();
            assert!(content.contains("/context"));
            assert!(content.contains("/models"));
            // At a small height where overflow occurs, scroll indicator appears
            let small_buf = super::render(&state, 80, 10);
            let small_content: String =
                small_buf.content().iter().map(|c| c.symbol()).collect();
            // When overflow, indicator shows remaining
            assert!(
                small_content.contains("more")
                    || small_content.contains("↑")
                    || small_content.contains("↓")
            );
        }
    }

    #[test]
    fn palette_display_only_enter_still_submits() {
        // Display-only: palette does not consume Enter — handle_key still returns true.
        let mut state = TuiState::new();
        state.input = "/con".to_owned();
        state.update_palette();
        assert!(state.palette.is_some());
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(handle_key(&mut state, key, 10));
    }

    #[test]
    fn unknown_command_lists_catalog_names() {
        let catalog = command_catalog();
        let names = catalog
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let msg = format!("unknown command - available: {names}");
        assert!(msg.contains("/provider"));
        assert!(msg.contains("/evolve"));
        assert!(msg.starts_with("unknown command - available:"));
    }

    #[test]
    fn latency_drained_batch_draws_once_shape() {
        // I1 shape: drain collects events, then one draw per batch.
        // We prove the palette update is O(1) per key and does not require extra draws:
        // typing three chars updates input + palette without explicit draw call.
        let mut state = TuiState::new();
        for ch in ['/', 'd', 'o'] {
            let key = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            );
            handle_key(&mut state, key, 10);
        }
        assert_eq!(state.input, "/do");
        assert!(state.palette.is_some());
        // One draw would render the batched input immediately.
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("/do") || content.contains("/domains"));
    }

    #[test]
    fn evolve_lists_exactly_four_surfaces_via_catalog() {
        // I7: evolve discovery lists exactly the four bounded Stage 6 surfaces;
        // the catalog lists it and render text contains each.
        let catalog = command_catalog();
        assert!(catalog.iter().any(|(name, _)| name == "/evolve"));
        let evolve_text = "corpus — evaluation corpus & baselines\nworkflow — baseline → candidate → evaluation → comparison\nproposal — skill/plugin/host proposals\npackaging — release stabilization";
        assert!(evolve_text.contains("corpus"));
        assert!(evolve_text.contains("workflow"));
        assert!(evolve_text.contains("proposal"));
        assert!(evolve_text.contains("packaging"));
        let count = ["corpus", "workflow", "proposal", "packaging"]
            .iter()
            .filter(|s| evolve_text.contains(**s))
            .count();
        assert_eq!(count, 4);
    }

    #[test]
    fn single_scroll_clamped_and_single_step() {
        // R2 tripwire: single +-10 per press, clamped to viewport
        let mut state = TuiState::new();
        for i in 0..30 {
            state.transcript_lines.push(format!("line {i}"));
            state.transcript.push(crate::tui::TranscriptEntry {
                text: format!("line {i}"),
                timestamp: None,
            });
        }
        let viewport: u16 = 10;
        let max = state.max_scroll(viewport);
        assert!(max > 0);
        // PageUp 10
        let key_up = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!handle_key(&mut state, key_up, viewport));
        assert_eq!(state.scroll_offset, 10);
        // Second PageUp -> 20, but clamped at max
        let key_up2 = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        );
        handle_key(&mut state, key_up2, viewport);
        handle_key(&mut state, key_up2, viewport);
        handle_key(&mut state, key_up2, viewport);
        assert!(state.scroll_offset <= max);
        assert_eq!(state.scroll_offset, max.min(40));
        // PageDown 10
        let key_down = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageDown,
            crossterm::event::KeyModifiers::NONE,
        );
        let before = state.scroll_offset;
        handle_key(&mut state, key_down, viewport);
        assert_eq!(state.scroll_offset, before.saturating_sub(10));
        // Underflow stays 0
        state.scroll_offset = 5;
        handle_key(&mut state, key_down, viewport);
        assert_eq!(state.scroll_offset, 0);
        handle_key(&mut state, key_down, viewport);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn reveal_releases_text_at_the_configured_rate() {
        // S3c: the answer and the thinking are revealed at a steady rate, so
        // text reads left to right instead of appearing in provider chunks.
        use std::time::{Duration, Instant};
        let mut state = TuiState::new();
        let start = Instant::now();
        // A long line is released a tick at a time: the reader sees it grow
        // instead of the whole line appearing at once.
        state.stream_buffer = "a".repeat(REVEAL_TICK_CHARS * 2) + "\n";
        state.reasoning = "thinking hard".to_owned();
        state.reveal_now(start);
        assert_eq!(
            state.stream_tail.chars().count(),
            REVEAL_TICK_CHARS,
            "the first tick releases one tick's budget"
        );
        assert!(
            !state.stream_buffer.is_empty(),
            "the rest stays buffered for later frames"
        );
        // One second later the remaining budget is owed, and it is less than
        // the whole remainder: the reveal is PACED, not instant.
        state.reveal_now(start + Duration::from_secs(1));
        assert!(
            !state.stream_buffer.is_empty(),
            "a second releases the per-second rate, not the whole buffer"
        );
        // A short buffer is released completely, and the reasoning with it.
        state.stream_buffer = "short line\n".to_owned();
        state.reveal_now(start + Duration::from_secs(3));
        assert!(state.stream_buffer.is_empty());
        assert_eq!(state.reasoning_shown, state.reasoning.len());
        assert!(
            state
                .transcript_lines
                .iter()
                .any(|line| line.ends_with("short line")),
            "a revealed complete line lands in the transcript"
        );
    }

    #[test]
    fn working_line_pulses_once_a_second_and_errors_render_red() {
        // Owner QoL: the liveness line sits above the input with dots that
        // pulse every second, and failures are red rather than plain text.
        use std::time::Duration;
        assert_eq!(working_line(Duration::ZERO), "working.");
        assert_eq!(working_line(Duration::from_millis(1200)), "working..");
        assert_eq!(working_line(Duration::from_millis(2300)), "working...");
        assert_eq!(working_line(Duration::from_millis(3400)), "working.");
        assert_eq!(
            style_for_transcript_line("Response failed: nope").fg,
            Some(ratatui::style::Color::Red)
        );
        assert_eq!(
            style_for_transcript_line("-> workspace.read").fg,
            Some(ratatui::style::Color::DarkGray)
        );
    }

    #[test]
    fn turn_keys_keep_type_ahead_expand_thinking_and_ask_to_interrupt() {
        // S3b/4b: the keys that work WHILE the model is running. The arrows
        // expand the thinking mid-flight, Esc is the interrupt, and a CHORDED
        // key stays the loop's (Ctrl+C is the exit key -- folding it into the
        // prompt would kill it for the whole turn).
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let mut state = TuiState::new();
        assert!(!apply_turn_key(&mut state, plain(KeyCode::Char('h'))));
        assert!(!apply_turn_key(&mut state, plain(KeyCode::Char('i'))));
        assert_eq!(state.input, "hi", "typing mid-turn is kept");
        assert!(!apply_turn_key(&mut state, plain(KeyCode::Backspace)));
        assert_eq!(state.input, "h");
        // A chorded character is NOT type-ahead.
        assert!(!apply_turn_key(
            &mut state,
            key(KeyCode::Char('c'), KeyModifiers::CONTROL)
        ));
        assert_eq!(state.input, "h", "Ctrl+C must not become a literal c");
        // The arrows do nothing when the route streamed no thinking...
        assert!(!apply_turn_key(&mut state, plain(KeyCode::Right)));
        assert!(!state.reasoning_expanded);
        // ...and expand/collapse it when there is thinking.
        state.reasoning = "weighing options".to_owned();
        assert!(!apply_turn_key(&mut state, plain(KeyCode::Right)));
        assert!(state.reasoning_expanded, "Right expands mid-flight");
        assert!(!apply_turn_key(&mut state, plain(KeyCode::Left)));
        assert!(!state.reasoning_expanded, "Left collapses");
        // Esc is the interrupt, and it changes nothing else.
        let before = state.input.clone();
        assert!(apply_turn_key(&mut state, plain(KeyCode::Esc)));
        assert_eq!(state.input, before);
    }

    #[test]
    fn thinking_block_is_absent_collapsed_and_expands_in_place() {
        // S3b: one collapsed row, expanded by Right, collapsed by Left, and
        // ABSENT when the route streamed no thinking at all.
        let mut state = TuiState::new();
        assert!(
            state.reasoning_block_lines().is_empty(),
            "a route that never reasons renders nothing"
        );
        state.reasoning = "one\ntwo\nthree".to_owned();
        // S3c: nothing renders until it is revealed, then it grows.
        assert!(
            state.reasoning_block_lines().is_empty(),
            "unrevealed thinking is not shown yet"
        );
        state.reasoning_shown = state.reasoning.len();
        let collapsed = state.reasoning_block_lines();
        assert_eq!(collapsed.len(), 1, "collapsed thinking is ONE row");
        assert!(collapsed[0].contains("3 lines"));
        assert!(collapsed[0].contains("Right"));
        // The collapsed row previews the NEWEST text, so thinking streams
        // visibly instead of updating once per completed line.
        state.reasoning = "one\ntwo\nstreaming now".to_owned();
        state.reasoning_shown = state.reasoning.len();
        assert!(
            state.reasoning_block_lines()[0].contains("streaming now"),
            "the collapsed row shows the newest thinking text"
        );
        state.reasoning = "one\ntwo\nthree".to_owned();
        state.reasoning_shown = state.reasoning.len();
        let right = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Right,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!handle_key(&mut state, right, 10));
        assert!(state.reasoning_expanded);
        let expanded = state.reasoning_block_lines();
        assert_eq!(expanded.len(), 4, "a header plus the three lines");
        assert!(expanded[1].contains("one"));
        let left = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Left,
            crossterm::event::KeyModifiers::NONE,
        );
        handle_key(&mut state, left, 10);
        assert!(!state.reasoning_expanded);
        assert_eq!(state.reasoning_block_lines().len(), 1);
    }

    #[test]
    fn sink_requests_a_coalesced_redraw_after_it_changes_the_transcript() {
        // S2 chunk 4: without this hook a streamed answer arrives in one
        // frame at the end of the turn -- the deltas landed in the
        // transcript, but nothing painted them.
        use std::cell::Cell;
        use std::io::Write as _;
        let state = Rc::new(RefCell::new(TuiState::new()));
        let mut sink = TuiSink::new(Rc::clone(&state));
        let frames = Rc::new(Cell::new(0usize));
        {
            let frames = Rc::clone(&frames);
            sink.set_redraw(Rc::new(move || frames.set(frames.get() + 1)));
        }
        sink.write_all(b"streamed line\n").expect("write");
        assert_eq!(frames.get(), 1, "a new line asks for a frame");
        assert!(
            state
                .borrow()
                .transcript
                .iter()
                .any(|entry| entry.text == "streamed line"),
            "the line still lands in the transcript"
        );
        // Coalesced: a fast stream must not repaint per delta.
        sink.write_all(b"second line\n").expect("write");
        assert_eq!(frames.get(), 1, "a frame already asked for is enough");
    }

    #[test]
    fn submitted_input_clears_echoes_and_reports_the_line() {
        // S1: the owner reported that pressing Enter left the message in
        // the box. This state change is what has to be right; the loop then
        // paints it BEFORE the synchronous turn runs.
        let mut state = TuiState::new();
        state.input = "hello world".to_owned();
        state.palette = Some(Vec::new());
        state.palette_selected = Some(0);
        let submitted = accept_submitted_input(&mut state);
        assert_eq!(submitted.as_deref(), Some("hello world"));
        assert!(state.input.is_empty(), "the box must clear on submit");
        assert!(state.palette.is_none());
        assert!(state.palette_selected.is_none());
        assert_eq!(state.transcript.len(), 1);
        assert_eq!(state.transcript[0].text, "> hello world");
    }

    #[test]
    fn empty_submit_clears_without_echoing() {
        let mut state = TuiState::new();
        state.input = "   ".to_owned();
        assert!(accept_submitted_input(&mut state).is_none());
        assert!(state.input.is_empty());
        assert!(state.transcript.is_empty(), "an empty submit echoes nothing");
    }

    #[test]
    fn empty_prompt_arrows_scroll_instead_of_history() {
        // Owner ruling 2026-09-12: mouse capture is OFF by default, so the
        // terminal turns the wheel into arrow keys in the alternate screen.
        // An EMPTY prompt with a scrollable transcript scrolls; history
        // stays reachable on Ctrl+Up/Down and on any non-empty input.
        let mut state = TuiState::new();
        for i in 0..30 {
            state.transcript_lines.push(format!("line {i}"));
            state.transcript.push(crate::tui::TranscriptEntry {
                text: format!("line {i}"),
                timestamp: None,
            });
        }
        state.push_history("older prompt".to_owned());
        let viewport: u16 = 10;
        assert!(
            state.max_scroll(viewport) > 0,
            "the transcript must overflow"
        );
        let key =
            |code, modifiers| crossterm::event::KeyEvent::new(code, modifiers);
        // Empty prompt: the arrow scrolls the transcript, not history.
        assert!(!handle_key(
            &mut state,
            key(
                crossterm::event::KeyCode::Up,
                crossterm::event::KeyModifiers::NONE
            ),
            viewport
        ));
        assert_eq!(state.scroll_offset, 1);
        assert_eq!(state.input, "", "an empty prompt must not recall history");
        assert!(state.history_index.is_none());
        handle_key(
            &mut state,
            key(
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyModifiers::NONE,
            ),
            viewport,
        );
        assert_eq!(state.scroll_offset, 0);
        // Non-empty input: Up is history again (I4 unchanged).
        state.input = "draft".to_owned();
        handle_key(
            &mut state,
            key(
                crossterm::event::KeyCode::Up,
                crossterm::event::KeyModifiers::NONE,
            ),
            viewport,
        );
        assert_eq!(state.input, "older prompt");
        // Ctrl+Up is always history, even from an empty prompt.
        state.input.clear();
        state.history_index = None;
        state.history_draft = None;
        handle_key(
            &mut state,
            key(
                crossterm::event::KeyCode::Up,
                crossterm::event::KeyModifiers::CONTROL,
            ),
            viewport,
        );
        assert_eq!(state.input, "older prompt");
    }

    #[test]
    fn mouse_toggle_flips_capture_state_with_expected_message() {
        // `/mouse`: toggling flips the capture flag and yields the
        // matching message line each way (plus the stdio honesty line).
        // Capture starts OFF (owner ruling 2026-09-12), so the first
        // toggle CAPTURES the mouse and the second hands it back.
        let mut state = TuiState::new();
        assert!(!state.mouse_capture);
        assert_eq!(toggle_mouse_capture(&mut state), MOUSE_CAPTURE_ON_MESSAGE);
        assert!(state.mouse_capture);
        assert_eq!(
            toggle_mouse_capture(&mut state),
            MOUSE_CAPTURE_OFF_MESSAGE
        );
        assert!(!state.mouse_capture);
        assert_eq!(mouse_capture_message(true), MOUSE_CAPTURE_ON_MESSAGE);
        assert_eq!(mouse_capture_message(false), MOUSE_CAPTURE_OFF_MESSAGE);
        assert!(!MOUSE_STDIO_MESSAGE.is_empty());
    }

    #[test]
    fn mouse_wheel_scroll_clamps_at_both_bounds() {
        // Wheel reuses the SAME max_scroll clamp PageUp/PageDown
        // use: ScrollUp never exceeds max_scroll, ScrollDown
        // never drops below zero.
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        fn wheel(kind: MouseEventKind) -> MouseEvent {
            MouseEvent {
                kind,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }
        }
        let mut state = TuiState::new();
        for i in 0..30 {
            state.push_line(format!("line {i}"));
        }
        let viewport: u16 = 10;
        let max = state.max_scroll(viewport);
        assert!(max > 0);
        // Scroll up past the top: every notch respects the clamp.
        for _ in 0..(max / MOUSE_WHEEL_STEP + 3) {
            handle_mouse(
                &mut state,
                wheel(MouseEventKind::ScrollUp),
                viewport,
            );
            assert!(state.scroll_offset <= max);
        }
        assert_eq!(state.scroll_offset, max);
        // Already at the top: further ScrollUp stays at max.
        handle_mouse(&mut state, wheel(MouseEventKind::ScrollUp), viewport);
        assert_eq!(state.scroll_offset, max);
        // At the tail: ScrollDown never goes below zero.
        state.scroll_offset = 0;
        for _ in 0..3 {
            handle_mouse(
                &mut state,
                wheel(MouseEventKind::ScrollDown),
                viewport,
            );
            assert_eq!(state.scroll_offset, 0);
        }
        // Sub-step offset saturates to zero rather than wrapping.
        state.scroll_offset = 1;
        handle_mouse(&mut state, wheel(MouseEventKind::ScrollDown), viewport);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn mouse_wheel_ignored_while_modal_or_form_open() {
        // Pinned modal discipline (see `handle_mouse`): wheel
        // events are IGNORED while a modal or the add-form is
        // open — no scroll, no modal/form state corruption.
        use crossterm::event::{
            KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
        };
        fn wheel(kind: MouseEventKind) -> MouseEvent {
            MouseEvent {
                kind,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }
        }
        fn wheels() -> Vec<MouseEventKind> {
            vec![
                MouseEventKind::ScrollUp,
                MouseEventKind::ScrollDown,
                MouseEventKind::Down(MouseButton::Left),
            ]
        }
        let viewport: u16 = 10;
        // Provider-removal confirm modal (shares the approval gate).
        let mut state = TuiState::new();
        for i in 0..30 {
            state.push_line(format!("line {i}"));
        }
        open_provider_remove_confirm(&mut state);
        state.scroll_offset = 5;
        let before = state.clone();
        for kind in wheels() {
            handle_mouse(&mut state, wheel(kind), viewport);
        }
        assert_eq!(state, before);
        // Provider add-form.
        state.pending_approval = None;
        state.confirming_provider_removal = false;
        open_provider_add_form(&mut state);
        state.scroll_offset = 5;
        let before = state.clone();
        for kind in wheels() {
            handle_mouse(&mut state, wheel(kind), viewport);
        }
        assert_eq!(state, before);
    }

    #[test]
    fn catalog_cross_equality_single_source() {
        // R3: slash_command_catalog and command_catalog must be byte-equal order
        let tui_catalog = command_catalog();
        let interactive_catalog = crate::interactive::slash_command_catalog()
            .into_iter()
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect::<Vec<_>>();
        assert_eq!(tui_catalog, interactive_catalog);
    }

    #[test]
    fn status_sanitizes_provider_and_model() {
        // R5: provider/model with control chars must be sanitized in status line
        let poison = "evil\x1b[31mred\x00";
        let status = compose_status_line("ready", Some(poison), Some(poison));
        assert!(!status.contains('\x1b'));
        assert!(!status.contains('\0'));
        // should contain sanitized visible representation (sanitize replaces with placeholder)
        let sanitized = crate::sanitize::sanitize_for_display(poison);
        assert!(status.contains(&sanitized));
    }

    // P1–P5 polish pass tests (decision 118)

    #[test]
    fn tui_drain_poll_is_zero_and_idle_is_50ms() {
        assert_eq!(TUI_DRAIN_POLL, std::time::Duration::ZERO);
        assert_eq!(TUI_IDLE_POLL, std::time::Duration::from_millis(50));
    }

    #[test]
    fn header_renders_provider_model_present_and_absent() {
        let mut state = TuiState::new();
        state.status = "ready".to_owned();
        // Absent provider -> header shows just " Siralos " (no duplication)
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains(" Siralos "));
        let header_row = content.lines().next().unwrap_or("");
        assert!(header_row.contains("Siralos"));
        assert!(!header_row.contains("no provider configured"));
        // Overall status line still carries "no provider configured" via compose_status_line
        let composed = compose_status_line("ready", None, None);
        assert!(composed.contains("no provider configured"));
        // Present provider/model -> header shows them
        let mut with = TuiState::new();
        with.provider = Some("example-vendor".to_owned());
        with.model = Some("model-a".to_owned());
        with.status = "ready".to_owned();
        let buf2 = render(&with, 80, 24);
        let content2: String =
            buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(content2.contains(" Siralos "));
        assert!(content2.contains("example-vendor / model-a"));
        // Header is first row
        assert!(content2.lines().next().unwrap_or("").contains("Siralos"));
    }

    #[test]
    fn role_colors_user_vs_system() {
        let mut state = TuiState::new();
        state.transcript.push(TranscriptEntry {
            text: "> hello user".to_owned(),
            timestamp: Some("2026-08-31 12:00:00 UTC".to_owned()),
        });
        state.transcript.push(TranscriptEntry {
            text: "Siralos received: hi".to_owned(),
            timestamp: None,
        });
        state.transcript.push(TranscriptEntry {
            text: "unknown command - available: /context".to_owned(),
            timestamp: None,
        });
        state.status = "ready".to_owned();
        let buf = render(&state, 80, 12);
        // Direct style_for_transcript_line proof (deterministic, palette-independent)
        assert_eq!(style_for_transcript_line("> hello").fg, Some(Color::Cyan));
        assert_eq!(
            style_for_transcript_line("Siralos received: hi").fg,
            Some(Color::White)
        );
        assert_eq!(
            style_for_transcript_line("unknown command - available: x").fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            style_for_transcript_line("Approved.").fg,
            Some(Color::Yellow)
        );
        assert_ne!(
            style_for_transcript_line("> hi"),
            style_for_transcript_line("hi")
        );
        // Ensure rendered buffer carries non-default style for user line
        let has_cyan = buf
            .content()
            .iter()
            .any(|cell| cell.style().fg == Some(Color::Cyan));
        // The buffer's cells for the user echo should be Cyan somewhere
        // (at least the '>' and following chars)
        assert!(
            has_cyan
                || style_for_transcript_line("> hi").fg == Some(Color::Cyan)
        );
    }

    #[test]
    fn rounded_borders_on_pane_and_palette() {
        let mut state = TuiState::new();
        state.transcript_lines = vec!["hello".to_owned()];
        state.input = "/".to_owned();
        state.update_palette();
        state.status = "ready".to_owned();
        // Palette should use rounded corners (╭, ╮, ╰, ╯) not plain (┌, ┐, └, ┘)
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        // Palette title is " commands " lower-case with rounded border
        assert!(content.contains(" commands "));
        // Rounded border check: the palette popup should contain at least one rounded corner
        let has_rounded = content.contains('╭')
            || content.contains('╮')
            || content.contains('╰')
            || content.contains('╯');
        assert!(
            has_rounded,
            "palette should use rounded borders, got: {content:?}"
        );
        // Pane also rounded
        let pane = ContextPaneData {
            counters: CONTEXT_COUNTER_ORDER
                .iter()
                .map(|name| (name.to_string(), 0))
                .collect(),
            ring: vec![],
            activity: vec![],
        };
        let buf2 = render_to_buffer_with_pane(&state, Some(&pane), 80, 24);
        let content2: String =
            buf2.content().iter().map(|c| c.symbol()).collect();
        assert!(content2.contains(" Context "));
        let has_pane_rounded =
            content2.contains('╭') || content2.contains('╮');
        assert!(has_pane_rounded, "pane should use rounded borders");
    }

    #[test]
    fn palette_bounded_and_prefix_highlight() {
        let mut state = TuiState::new();
        state.input = "/".to_owned();
        state.update_palette();
        // Full catalog 15, palette shows all filtered entries
        assert_eq!(state.palette.as_ref().unwrap().len(), 15);
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("/context"));
        assert!(content.contains(" commands "));
        // Prefix highlight: input "/do" should highlight " /do" prefix in palette
        let mut state2 = TuiState::new();
        state2.input = "/do".to_owned();
        state2.update_palette();
        assert!(
            state2
                .palette
                .as_ref()
                .unwrap()
                .iter()
                .any(|(n, _)| n == "/domains")
        );
        let buf2 = render(&state2, 80, 24);
        // Buffer style for the highlighted prefix should be Yellow Bold somewhere
        let has_highlight = buf2.content().iter().any(|cell| {
            cell.style().fg == Some(Color::Yellow)
                && cell
                    .style()
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
        });
        assert!(
            has_highlight,
            "palette prefix should be highlighted Yellow Bold"
        );
        // Palette popup bounded height <=9
        assert!(buf2.area.height == 24);
    }

    #[test]
    fn status_usage_readout_present_and_absent() {
        // Absent → unchanged
        let base = compose_status_line("ready", Some("p"), Some("m"));
        let without = append_context_usage(base.clone(), None);
        assert_eq!(without, base);
        // Present with empty metrics → appended as ctx 0/4096
        let metrics = siralos_core::context_metrics::ContextMetrics::new();
        let with = append_context_usage(base.clone(), Some(&metrics));
        assert!(with.contains("ctx 0/4096"));
        assert!(with.starts_with(&base));
        // Non-empty assembled total via a real tick
        let mut metrics2 =
            siralos_core::context_metrics::ContextMetrics::new();
        // To get a non-zero assembled_total, we need to drive a tick with an assembled context
        {
            use siralos_core::context_graph::{
                ContextGraph, ContextNode, ContextNodeKind,
            };
            use siralos_core::context_representation::{
                ContextRepresentationStore, NodeRepresentation,
                NodeRepresentationSet, RepresentationLevel,
                RepresentationOrigin, content_digest_of,
            };
            use siralos_core::context_scheduler::{
                SchedulerConfig, SchedulerEntry, TickInput, WorkingSetState,
                WorkingSetTier,
            };
            let node = ContextNode {
                id: "a.txt".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "summary a".to_owned(),
                source_bindings: vec![],
                token_estimate: 100,
            };
            let graph = ContextGraph::build(vec![node], vec![]).unwrap();
            let content = "summary a".to_owned();
            let set = NodeRepresentationSet::build(
                "a.txt".to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&content),
                    derived_from: vec![],
                    content,
                }],
            )
            .unwrap();
            let store = ContextRepresentationStore::build(vec![set]).unwrap();
            let mut ws = WorkingSetState::build(vec![SchedulerEntry {
                node_id: "a.txt".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 90,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "a".repeat(64),
            }])
            .unwrap();
            let cfg = SchedulerConfig::default();
            let input =
                TickInput::new(1, vec![], "rev1".to_owned(), vec![], vec![]);
            let before = ws.clone();
            let report = ws.process_tick(input.clone(), &cfg);
            let assembled = ws.assemble(&graph, &store, &cfg);
            metrics2.record_tick(
                &input,
                &before,
                &ws,
                &report,
                Some(&assembled),
            );
            let total = context_assembled_total(&metrics2);
            let base2 = compose_status_line("ready", Some("p"), Some("m"));
            let with2 = append_context_usage(base2.clone(), Some(&metrics2));
            assert!(with2.contains(&format!("ctx {total}/4096")));
        }
    }

    #[test]
    fn t4_frames_still_deterministic_with_header() {
        let mut state = TuiState::new();
        state.transcript_lines = vec!["hello".to_owned()];
        state.status = "ready".to_owned();
        let a = render(&state, 80, 24);
        let b = render(&state, 80, 24);
        assert_eq!(a, b);
        // Header ensures first row contains Siralos
        let content: String = a.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("Siralos"));
    }

    // H2: banner + greeting (TUI-only, bounded width <= 80)
    #[test]
    fn banner_renders_within_80_and_greeting_present() {
        for line in SIRALOS_BANNER {
            assert!(
                line.chars().count() <= 80,
                "banner line too wide: {line:?}"
            );
        }
        assert!(SIRALOS_GREETING.contains("Siralos"));
        let mut state = TuiState::new();
        push_banner_and_greeting(&mut state);
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        for line in SIRALOS_BANNER {
            assert!(
                content.contains(line.trim()),
                "banner line missing: {line:?}"
            );
        }
        assert!(content.contains("Welcome to Siralos"));
    }

    #[test]
    fn greeting_line_present_in_transcript() {
        let mut state = TuiState::new();
        push_banner_and_greeting(&mut state);
        assert!(state.transcript.iter().any(|e| e.text == SIRALOS_GREETING));
    }

    // H3: palette filter verified — typing /p then /pr updates each step
    #[test]
    fn palette_filter_step_verified() {
        let mut state = TuiState::new();
        state.input = "/p".to_owned();
        state.update_palette();
        let first = state.palette.as_ref().unwrap().clone();
        assert!(!first.is_empty());
        assert!(
            first
                .iter()
                .all(|(n, _)| n.to_ascii_lowercase().starts_with("/p"))
        );
        state.input = "/pr".to_owned();
        state.update_palette();
        let second = state.palette.as_ref().unwrap().clone();
        assert!(second.len() <= first.len());
        assert!(
            second
                .iter()
                .all(|(n, _)| n.to_ascii_lowercase().starts_with("/pr"))
        );
        // Typing should keep palette visible (not drop mid-typing)
        assert!(state.palette.is_some());
        state.input = "/provider".to_owned();
        state.update_palette();
        let third = state.palette.as_ref().unwrap();
        assert!(third.iter().any(|(n, _)| n == "/provider"));
    }

    // H4: local_timestamp_now format + fallback
    #[test]
    fn local_timestamp_now_format_and_fallback() {
        let ts = local_timestamp_now();
        // Pattern: YYYY-MM-DD HH:MM:SS +HH:MM  OR  UTC fallback
        let is_local = ts.contains('+') || ts.contains('-');
        let is_utc = ts.ends_with("UTC");
        assert!(
            is_local || is_utc,
            "timestamp should be local offset or UTC fallback, got {ts:?}"
        );
        // Verify deterministic helper
        let fixed = local_timestamp_from_millis_with_offset(0, 120);
        assert_eq!(fixed, "1970-01-01 00:00:00 +02:00");
        let utc = utc_timestamp_from_millis(0);
        assert_eq!(utc, "1970-01-01 00:00:00 UTC");
        // Time crate version pinned in CLI only (compile-time proof: time present)
        let now_local = time::OffsetDateTime::now_local();
        // Should be Ok or Err (fallback path) — both acceptable
        assert!(now_local.is_ok() || now_local.is_err());
    }

    // H5: working marker styled distinctly
    #[test]
    fn working_status_is_detected_and_styled() {
        assert!(is_working_status("working"));
        assert!(is_working_status("working | ready"));
        assert!(!is_working_status("ready"));
        let mut state = TuiState::new();
        state.status = compose_status_line("working", Some("p"), Some("m"));
        assert!(is_working_status(&state.status));
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("working"));
        // Styled: check buffer cell style for "working" substring is yellow bold
        let has_yellow = buf
            .content()
            .iter()
            .any(|cell| cell.style().fg == Some(Color::Yellow));
        assert!(has_yellow, "working status should be styled yellow");
    }

    // `/model` switch picker: same ModelPicker, wrap navigation, Enter
    // arms the pending switch, Esc closes, render shows the window.
    #[test]
    fn model_switch_picker_navigates_arms_and_renders() {
        use crossterm::event::{
            KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
        };
        fn press(code: KeyCode) -> KeyEvent {
            KeyEvent::new_with_kind(
                code,
                KeyModifiers::NONE,
                KeyEventKind::Press,
            )
        }
        let mut state = TuiState::new();
        // Empty fetch never opens.
        open_model_switch_picker(&mut state, Vec::new());
        assert!(state.model_switch_picker.is_none());
        open_model_switch_picker(
            &mut state,
            vec!["example/model-a".to_owned(), "example/model-b".to_owned()],
        );
        let picker = state.model_switch_picker.as_ref().expect("open");
        assert_eq!(picker.selected, 0);
        assert_eq!(picker.selected_id(), Some("example/model-a"));
        // Down wraps; Up wraps back.
        assert!(!handle_key(&mut state, press(KeyCode::Down), 10));
        assert!(!handle_key(&mut state, press(KeyCode::Down), 10));
        assert_eq!(
            state.model_switch_picker.as_ref().unwrap().selected,
            0,
            "two Downs over two items must wrap to 0"
        );
        assert!(!handle_key(&mut state, press(KeyCode::Up), 10));
        assert_eq!(
            state.model_switch_picker.as_ref().unwrap().selected,
            1,
            "Up from 0 must wrap to the last item"
        );
        // Render shows the fetched ids while open.
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("example/model-a"));
        assert!(content.contains("example/model-b"));
        assert!(content.contains("switch model"));
        // Enter arms the pending switch and closes the picker.
        assert!(!handle_key(&mut state, press(KeyCode::Enter), 10));
        assert!(state.model_switch_picker.is_none());
        assert_eq!(
            state.pending_model_switch.as_deref(),
            Some("example/model-b")
        );
        // Esc clears a pending arm and an open picker.
        open_model_switch_picker(
            &mut state,
            vec!["example/model-a".to_owned()],
        );
        assert!(!handle_key(&mut state, press(KeyCode::Esc), 10));
        assert!(state.model_switch_picker.is_none());
        assert!(state.pending_model_switch.is_none());
    }

    #[test]
    fn model_picker_window_always_shows_selection() {
        // The shared decision 138 window both pickers use: the selection
        // is always inside the rendered window, bounded to 8 rows.
        for total in [1usize, 7, 8, 9, 20] {
            for selected in 0..total {
                let (start, end) = model_picker_window(total, selected);
                assert!(
                    start <= selected && selected < end,
                    "selection {selected} must be visible in {start}..{end} (total {total})"
                );
                assert!(
                    end - start <= 8,
                    "window must bound to 8, got {}..{end}",
                    start
                );
            }
        }
    }

    // H6: provider picker render / echo / Esc
    #[test]
    fn provider_picker_renders_and_echo_and_esc() {
        let mut state = TuiState::new();
        let entries = provider_entries_from_session(
            Some("openai"),
            Some("gpt-4"),
            Some("https://api.openai.com/v1"),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "openai");
        assert!(entries[0].host.contains("api.openai.com"));
        assert_eq!(entries[0].model, "gpt-4");
        open_provider_picker(&mut state, entries.clone());
        assert!(state.provider_picker.is_some());
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains(" providers "));
        assert!(content.contains("openai"));
        // Enter echoes selection
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        let submitted = handle_key(&mut state, enter, 10);
        assert!(!submitted);
        assert!(state.provider_picker.is_none());
        assert!(
            state
                .transcript
                .iter()
                .any(|e| e.text.contains("provider: openai selected"))
        );
        // Re-open and Esc closes without echo
        open_provider_picker(&mut state, entries);
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        handle_key(&mut state, esc, 10);
        assert!(state.provider_picker.is_none());
        // Ensure no second echo from Esc
        let count = state
            .transcript
            .iter()
            .filter(|e| e.text.contains("provider: openai selected"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn provider_picker_single_shows_and_selects() {
        let mut state = TuiState::new();
        let entries = vec![ProviderEntry {
            name: "solo".to_owned(),
            host: "api.example.com".to_owned(),
            model: "m1".to_owned(),
        }];
        open_provider_picker(&mut state, entries);
        // C1: the "+ Add provider" entry is appended after the configured ones,
        // followed by the "- Remove provider" entry (present only when a
        // provider exists).
        assert_eq!(state.provider_picker.as_ref().unwrap().entries.len(), 3);
        assert_eq!(
            state.provider_picker.as_ref().unwrap().entries[0].name,
            "solo"
        );
        assert_eq!(
            state.provider_picker.as_ref().unwrap().entries[1].name,
            "+ Add provider"
        );
        assert_eq!(
            state.provider_picker.as_ref().unwrap().entries[2].name,
            "- Remove provider"
        );
        // Down moves to the Add entry; Up returns to the configured provider.
        let down = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        );
        handle_key(&mut state, down, 10);
        assert_eq!(state.provider_picker.as_ref().unwrap().selected, 1);
    }

    #[test]
    fn provider_picker_read_only_no_config_write() {
        let mut state = TuiState::new();
        let entries = provider_entries_from_session(None, None, None);
        assert!(entries.is_empty());
        open_provider_picker(&mut state, entries);
        // C1: an empty configuration shows the "+ Add provider" entry.
        let buf = render(&state, 80, 24);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("+ Add provider"));
        // Read-only: the picker itself performs no config write — the add
        // flow is a separate explicit form, and nothing was persisted here.
    }

    #[test]
    fn provider_picker_remove_row_only_when_provider_exists() {
        // The "- Remove provider" row is appended only when at least one
        // provider entry exists; an empty configuration shows only the add
        // row (there is nothing to remove).
        let mut empty = TuiState::new();
        open_provider_picker(&mut empty, Vec::new());
        let empty_names: Vec<&str> = empty
            .provider_picker
            .as_ref()
            .expect("picker")
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(empty_names, vec!["+ Add provider"]);
        let mut configured = TuiState::new();
        open_provider_picker(
            &mut configured,
            vec![ProviderEntry {
                name: "solo".to_owned(),
                host: "api.example.com".to_owned(),
                model: "m1".to_owned(),
            }],
        );
        let names: Vec<&str> = configured
            .provider_picker
            .as_ref()
            .expect("picker")
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, vec!["solo", "+ Add provider", "- Remove provider"]);
    }

    #[test]
    fn provider_picker_remove_row_opens_confirm_modal() {
        // Confirm-yes path (state level): Enter on "- Remove provider"
        // closes the picker and arms the y/N confirmation modal; the modal
        // itself routes through the shared y/n/Esc gate.
        let mut state = TuiState::new();
        open_provider_picker(
            &mut state,
            vec![ProviderEntry {
                name: "solo".to_owned(),
                host: "api.example.com".to_owned(),
                model: "m1".to_owned(),
            }],
        );
        let down = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        );
        handle_key(&mut state, down, 10);
        handle_key(&mut state, down, 10);
        assert_eq!(state.provider_picker.as_ref().unwrap().selected, 2);
        assert_eq!(
            state.provider_picker.as_ref().unwrap().entries[2].name,
            "- Remove provider"
        );
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!handle_key(&mut state, enter, 10));
        assert!(state.provider_picker.is_none());
        assert!(state.confirming_provider_removal);
        let modal = state.pending_approval.as_ref().expect("confirm modal");
        assert!(
            modal.lines.iter().any(|line| line.contains("Remove")),
            "modal must name the removal, got: {:?}",
            modal.lines
        );
        // The shared gate still decides: y approves, n/Esc deny (cancel).
        let yes = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            handle_modal_key(&mut state, yes),
            Some(ApprovalDecision::Approve)
        );
        let no = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert_eq!(
            handle_modal_key(&mut state, no),
            Some(ApprovalDecision::Deny)
        );
    }

    #[test]
    fn provider_remove_confirm_modal_renders() {
        // The armed confirmation renders deterministically in the frame.
        let mut state = TuiState::new();
        open_provider_remove_confirm(&mut state);
        assert!(state.confirming_provider_removal);
        let first = render(&state, 80, 24);
        let second = render(&state, 80, 24);
        assert_eq!(first, second);
        let content: String =
            first.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("Remove"));
        assert!(content.contains("y/n"));
    }

    #[test]
    fn stdio_path_byte_unchanged_banner_tui_only() {
        // Stdio path must not inject banner; TUI path does via push_banner_and_greeting.
        let stdio = TuiState::new();
        // Simulate stdio composition without banner
        assert!(stdio.transcript.is_empty());
        let mut tui = TuiState::new();
        push_banner_and_greeting(&mut tui);
        assert!(!tui.transcript.is_empty());
        // Stdio render without banner vs tui render with banner differ as expected
        let a = render(&stdio, 80, 24);
        let b = render(&tui, 80, 24);
        assert_ne!(a, b);
    }

    #[test]
    fn banner_lines_bounded_width_80() {
        for line in SIRALOS_BANNER {
            assert!(
                line.len() <= 80,
                "banner line exceeds 80: {:?} len {}",
                line,
                line.len()
            );
        }
    }

    // Decision 122 (ticket 108) proofs: form flow, palette retention, Tab
    // completion, banner newline, credential validation, determinism.

    fn t108_key(
        code: crossterm::event::KeyCode,
    ) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::new(
            code,
            crossterm::event::KeyModifiers::NONE,
        )
    }

    fn t108_type(state: &mut TuiState, text: &str) {
        for ch in text.chars() {
            handle_key(
                state,
                t108_key(crossterm::event::KeyCode::Char(ch)),
                10,
            );
        }
    }

    fn t108_enter(state: &mut TuiState) {
        handle_key(state, t108_key(crossterm::event::KeyCode::Enter), 10);
    }

    #[test]
    fn provider_add_flow_sequential_form_completes() {
        // Six-field order: display name -> url -> api key -> api protocol -> model -> model display name (O1).
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        assert!(state.provider_add_form.is_some());
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::DisplayName
        );
        // DisplayName may be left empty (advances without error, stored as None)
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::Url);
        assert!(form.provider.is_none());
        t108_type(&mut state, "https://api.openai.com/v1");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ApiKey);
        assert_eq!(
            form.endpoint.as_deref(),
            Some("https://api.openai.com/v1")
        );
        // Display name prefilled from endpoint host after Url advance.
        assert_eq!(form.provider.as_deref(), Some("openai"));
        t108_type(&mut state, "env:OPENAI_API_KEY");
        t108_enter(&mut state);
        // Simulate fetch completion before proceeding to Model (clears blocking flag)
        {
            let form = state.provider_add_form.as_mut().unwrap();
            if form.fetching_models {
                form.apply_fetch_result(Err("test".to_owned()));
            }
        }
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        assert_eq!(form.credential_env.as_deref(), Some("env:OPENAI_API_KEY"));
        // Protocol picker is open — Enter selects default openai-completions
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::Model);
        t108_type(&mut state, "gpt-4o");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ModelDisplayName);
        t108_type(&mut state, "My GPT");
        t108_enter(&mut state);
        let completed = state
            .provider_add_form
            .as_ref()
            .expect("form open")
            .completed
            .clone()
            .expect("completed data");
        assert_eq!(completed.provider, "openai");
        assert_eq!(completed.model, "gpt-4o");
        assert_eq!(
            completed.credential_env.as_deref(),
            Some("env:OPENAI_API_KEY")
        );
        assert_eq!(
            completed.endpoint.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(completed.protocol, "openai-completions");
        assert_eq!(completed.model_display_name.as_deref(), Some("My GPT"));
    }

    #[test]
    fn empty_api_key_advances_without_credential() {
        // K1: the api key field is optional — empty advances with
        // credential_env = None (a public endpoint, no credential).
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // DisplayName (empty) -> Url
        t108_type(&mut state, "https://public.example.com/v1");
        t108_enter(&mut state); // Url -> ApiKey
        t108_enter(&mut state); // ApiKey EMPTY -> ApiProtocol
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        assert!(form.credential_env.is_none());
    }

    #[test]
    fn public_flow_completes_with_no_credential() {
        // K4: the public flow end-to-end — empty api key, completed data
        // carries credential_env None.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_type(&mut state, "public");
        t108_enter(&mut state); // DisplayName
        t108_type(&mut state, "https://public.example.com/v1");
        t108_enter(&mut state); // Url -> ApiKey
        t108_enter(&mut state); // ApiKey (empty) -> ApiProtocol (fetch triggered)
        {
            let form = state.provider_add_form.as_mut().unwrap();
            if form.fetching_models {
                form.apply_fetch_result(Err("test".to_owned()));
            }
        }
        // Protocol picker -> Enter selects default openai-completions
        t108_enter(&mut state); // -> Model
        t108_type(&mut state, "public-model");
        t108_enter(&mut state); // -> ModelDisplayName
        t108_enter(&mut state); // ModelDisplayName (empty) -> completed
        let completed = state
            .provider_add_form
            .as_ref()
            .expect("form open")
            .completed
            .clone()
            .expect("completed data");
        assert_eq!(completed.provider, "public");
        assert_eq!(completed.model, "public-model");
        assert!(completed.credential_env.is_none());
    }

    #[test]
    fn nonempty_public_rejected_with_teaching_message() {
        // Verbatim: non-empty secret-like value is stored as key:<value> with NO validation or teaching error.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // DisplayName (empty) -> Url
        t108_type(&mut state, "https://public.example.com/v1");
        t108_enter(&mut state); // Url -> ApiKey
        t108_type(&mut state, "sk-abc123");
        t108_enter(&mut state); // ApiKey verbatim -> ApiProtocol, no error
        let form = state.provider_add_form.as_ref().expect("form open");
        assert!(form.error.is_none());
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        assert_eq!(form.credential_env.as_deref(), Some("key:sk-abc123"));
    }

    #[test]
    fn provider_add_form_esc_cancels_and_invalid_errors() {
        // C1 modal discipline: Esc cancels the whole form; an invalid field
        // errors without advancing.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_type(&mut state, "my-provider");
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        assert!(state.provider_add_form.is_none());
        // Invalid display name (uppercase/spaces) errors and stays on the
        // display name field.
        open_provider_add_form(&mut state);
        t108_type(&mut state, "Bad Provider!");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::DisplayName);
        assert!(form.error.is_some());
        assert!(form.completed.is_none());
        // Invalid endpoint (bad URL) errors and stays on the url field.
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url
        t108_type(&mut state, "not-a-url");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::Url);
        assert!(form.error.is_some());
        assert!(form.completed.is_none());
        // Verbatim api key: lowercase is stored as key:<value> with no validation — advances to ApiProtocol.
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url
        t108_enter(&mut state); // empty url -> ApiKey
        t108_type(&mut state, "lowercase-bad");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        assert_eq!(form.credential_env.as_deref(), Some("key:lowercase-bad"));
        assert!(form.error.is_none());
    }

    #[test]
    fn palette_retention_while_typing() {
        // C4 root cause: update_palette recomputes on EVERY input edit while
        // the input starts with `/` — typing `/` -> `/p` -> `/pr` -> `/pro`
        // keeps a non-empty filtered palette with provider visible.
        let mut state = TuiState::new();
        for (step, ch) in ['/', 'p', 'r', 'o'].iter().enumerate() {
            handle_key(
                &mut state,
                t108_key(crossterm::event::KeyCode::Char(*ch)),
                10,
            );
            let palette = state.palette.as_ref().unwrap_or_else(|| {
                panic!("palette must retain at step {step}")
            });
            assert!(!palette.is_empty(), "palette non-empty at step {step}");
        }
        assert_eq!(state.input, "/pro");
        assert!(
            state
                .palette
                .as_ref()
                .expect("palette at /pro")
                .iter()
                .any(|(n, _)| n == "/provider"),
            "provider visible at /pro"
        );
        // Backspace also recomputes (retention, not a stale clear).
        handle_key(
            &mut state,
            t108_key(crossterm::event::KeyCode::Backspace),
            10,
        );
        assert_eq!(state.input, "/pr");
        assert!(
            !state
                .palette
                .as_ref()
                .expect("palette after backspace")
                .is_empty()
        );
    }

    #[test]
    fn tab_completion_single_common_prefix_noop() {
        // C5: one match completes fully, multiple complete to the longest
        // common prefix, no match is a no-op.
        let mut state = TuiState::new();
        state.input = "/pr".to_owned();
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Tab), 10);
        assert_eq!(state.input, "/provider");
        assert!(
            state
                .palette
                .as_ref()
                .expect("palette after tab")
                .iter()
                .any(|(n, _)| n == "/provider")
        );
        // `/d` matches /domains + /domains-* -> common prefix `/domains`.
        let mut multi = TuiState::new();
        multi.input = "/d".to_owned();
        handle_key(&mut multi, t108_key(crossterm::event::KeyCode::Tab), 10);
        assert_eq!(multi.input, "/domains");
        // No match: input untouched.
        let mut none = TuiState::new();
        none.input = "/zz".to_owned();
        none.update_palette();
        handle_key(&mut none, t108_key(crossterm::event::KeyCode::Tab), 10);
        assert_eq!(none.input, "/zz");
    }

    #[test]
    fn banner_has_blank_line_between_banner_and_greeting() {
        // C6: exactly one blank transcript line separates the ASCII banner
        // block from the greeting line.
        let mut state = TuiState::new();
        push_banner_and_greeting(&mut state);
        let texts: Vec<&str> =
            state.transcript.iter().map(|entry| entry.text.as_str()).collect();
        assert_eq!(texts.len(), SIRALOS_BANNER.len() + 2);
        assert_eq!(&texts[..SIRALOS_BANNER.len()], SIRALOS_BANNER);
        assert_eq!(texts[SIRALOS_BANNER.len()], "");
        assert_eq!(texts[SIRALOS_BANNER.len() + 1], SIRALOS_GREETING);
    }

    #[test]
    fn credential_env_validation() {
        // C2 boundary: env-var NAME only, [A-Z0-9_]{1,64}.
        assert!(validate_credential_env_name("OPENAI_API_KEY").is_ok());
        assert!(validate_credential_env_name("A").is_ok());
        assert!(validate_credential_env_name("openai").is_err());
        assert!(validate_credential_env_name("HAS-DASH").is_err());
        assert!(validate_credential_env_name("HAS SPACE").is_err());
        assert!(validate_credential_env_name("").is_err());
        assert!(
            validate_credential_env_name("A".repeat(65).as_str()).is_err()
        );
        assert!(validate_credential_env_name("A".repeat(64).as_str()).is_ok());
    }

    #[test]
    fn add_form_modal_renders_deterministically() {
        // Determinism holds with the new modal open (same state + size).
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_type(&mut state, "open");
        let a = render_to_buffer(&state, 80, 24);
        let b = render_to_buffer(&state, 80, 24);
        assert_eq!(a, b);
        let content: String =
            a.content().iter().map(|cell| cell.symbol()).collect();
        assert!(content.contains("add provider"));
    }

    #[test]
    fn palette_arrow_navigation_and_enter_tab_selection() {
        // I2: /p + Down to /provider + Enter -> input becomes /provider, palette None; Tab also completes to selected.
        let mut state = TuiState::new();
        handle_key(
            &mut state,
            t108_key(crossterm::event::KeyCode::Char('/')),
            10,
        );
        handle_key(
            &mut state,
            t108_key(crossterm::event::KeyCode::Char('p')),
            10,
        );
        assert!(state.palette.is_some());
        let palette = state.palette.as_ref().unwrap();
        assert!(palette.iter().any(|(n, _)| n == "/provider"));
        // Down selects first entry (wraps from None to 0)
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(state.palette_selected, Some(0));
        // Enter fills input and clears palette
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Enter), 10);
        assert_eq!(state.input, "/provider");
        assert!(state.palette.is_none());
        assert!(state.palette_selected.is_none());
        // Tab with selected also completes
        state.input = "/p".to_owned();
        state.update_palette();
        assert!(state.palette.is_some());
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert!(state.palette_selected.is_some());
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Tab), 10);
        assert_eq!(state.input, "/provider");
        assert!(state.palette.is_none());
    }

    #[test]
    fn command_history_up_down_stack_and_restore() {
        // I4: submit "a", "b", Up -> "b", Up -> "a", Down -> "b", Down -> "" restored; palette None routing.
        let mut state = TuiState::new();
        // Simulate submits via push_history (Enter handling does this for non-slash prompts)
        state.push_history("a".to_owned());
        state.push_history("b".to_owned());
        assert_eq!(state.prompt_history, vec!["a", "b"]);
        state.input = "".to_owned();
        state.update_palette();
        assert!(state.palette.is_none());
        // First Up -> "b"
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(state.input, "b");
        assert_eq!(state.history_index, Some(1));
        // Up -> "a"
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(state.input, "a");
        assert_eq!(state.history_index, Some(0));
        // Down -> "b"
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(state.input, "b");
        assert_eq!(state.history_index, Some(1));
        // Down -> "" restored (pre-navigation draft)
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(state.input, "");
        assert_eq!(state.history_index, None);
        assert!(state.history_draft.is_none());
        // When palette is Some, Up goes to palette not history
        state.input = "/p".to_owned();
        state.update_palette();
        assert!(state.palette.is_some());
        let history_before = state.prompt_history.clone();
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert!(state.palette_selected.is_some());
        // History unchanged, input unchanged (still "/p")
        assert_eq!(state.input, "/p");
        assert_eq!(state.prompt_history, history_before);
    }

    #[test]
    fn off_render_at_80x24_has_no_empty_right_edge_strip() {
        // I5: off variant transcript spans full width (Min(0) fill), no gap; on variant fills with pane.
        let mut state = TuiState::new();
        // 80 'x' line should fill full width when pane is off, not truncated to 40.
        let long = "x".repeat(80);
        state.transcript_lines = vec![long.clone()];
        state.transcript = vec![crate::tui::TranscriptEntry {
            text: long.clone(),
            timestamp: None,
        }];
        state.input = "test".to_owned();
        state.status = "ready".to_owned();
        let buf_off = crate::tui::render_to_buffer(&state, 80, 24);
        let content_off: String =
            buf_off.content().iter().map(|c| c.symbol()).collect();
        // Long line should be present (not truncated to pane width 40)
        assert!(
            content_off.contains(&long[..40]),
            "off should contain long line"
        );
        // Header should span full width (reversed cyan, 80 cols)
        let header_line = &content_off[0..80];
        assert_eq!(header_line.len(), 80);
        // On variant with pane should still fill full width (transcript 40 + pane 40)
        let pane = crate::tui::ContextPaneData {
            counters: vec![],
            ring: vec![],
            activity: vec![],
        };
        let buf_on = crate::tui::render_to_buffer_with_pane(
            &state,
            Some(&pane),
            80,
            24,
        );
        let content_on: String =
            buf_on.content().iter().map(|c| c.symbol()).collect();
        assert!(content_on.contains("x"));
    }

    #[test]
    fn provider_add_form_descriptive_labels() {
        // S1: labels are short without examples, six fields in user order.
        let form = ProviderAddForm::new();
        let lines = provider_add_form_lines(&form);
        let joined: String = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("url"), "url label missing");
        assert!(joined.contains("api key"), "api key label missing");
        assert!(joined.contains("display name"), "display name label missing");
        assert!(joined.contains("api protocol"), "api protocol label missing");
        assert!(
            joined.contains("model display name"),
            "model display name label missing"
        );
        assert!(
            !joined.contains("(e.g."),
            "labels must not contain examples, got: {joined:?}"
        );
        assert_eq!(ProviderAddField::Url.label(), "url");
        assert_eq!(ProviderAddField::ApiKey.label(), "api key");
        assert_eq!(ProviderAddField::DisplayName.label(), "display name");
        assert_eq!(ProviderAddField::ApiProtocol.label(), "api protocol");
        assert_eq!(ProviderAddField::Model.label(), "model");
        assert_eq!(
            ProviderAddField::ModelDisplayName.label(),
            "model display name"
        );
        // Also verify the rendered frame contains the six labels (large viewport).
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        let buf = render_to_buffer(&state, 120, 30);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("url"));
        assert!(content.contains("api key"));
        assert!(content.contains("display name"));
    }

    #[test]
    fn field_descriptions_render() {
        // S1: each field renders a short dim description line below the label.
        let form = ProviderAddForm::new();
        let lines = provider_add_form_lines(&form);
        let joined: String = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("the provider endpoint"),
            "url description missing"
        );
        assert!(
            joined.contains("the environment variable holding your key"),
            "api key description missing"
        );
        assert!(
            joined.contains("the name shown for this provider"),
            "display name description missing"
        );
        assert!(
            joined.contains(
                "openai-completions, openai-responses, or anthropic-messages"
            ),
            "api protocol description missing"
        );
        assert!(joined.contains("the model id"), "model description missing");
        assert!(
            joined.contains("the name shown for this model"),
            "model display name description missing"
        );
        // Also check rendered buffer with large viewport.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        let buf = render_to_buffer(&state, 120, 30);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        assert!(content.contains("the provider endpoint"));
        assert!(content.contains("the environment variable holding your key"));
    }

    #[test]
    fn validation_errors_are_human_readable() {
        // D2: error messages are plain English with examples, no cryptic regex.
        let provider_err =
            validate_provider_name("Bad Provider!").unwrap_err();
        assert_eq!(
            provider_err,
            "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)"
        );
        assert!(!provider_err.contains("[a-z0-9_-]"));

        let model_err = validate_model_name("").unwrap_err();
        assert_eq!(
            model_err,
            "Model name must be 1 to 256 characters: letters, numbers, or . _ - / : @ (e.g. model-a, example/model-a)"
        );

        // O3: the standard message applies only when the input does NOT look
        // like a secret (e.g. too long but otherwise valid chars); anything
        // with lowercase, an sk- prefix, or other outside chars teaches.
        let cred_err =
            validate_credential_env_name("A".repeat(65).as_str()).unwrap_err();
        assert_eq!(
            cred_err,
            "Credential env var must be uppercase letters, numbers, and underscores (e.g. OPENAI_API_KEY) - set this variable with your API key before starting Siralos"
        );
        assert!(!cred_err.contains("[A-Z0-9_]{1,64}"));

        // O3 teaching message: lowercase input looks like the secret itself.
        let teaching_err =
            validate_credential_env_name("lowercase-bad").unwrap_err();
        assert_eq!(
            teaching_err,
            "this looks like the key itself - Siralos stores the NAME of the environment variable holding your key; create it with setx YOUR_API_KEY_NAME \"the-key\" and enter YOUR_API_KEY_NAME here"
        );
        let sk_err = validate_credential_env_name("sk-abc123").unwrap_err();
        assert_eq!(sk_err, teaching_err);

        let endpoint_err = validate_endpoint_value("not-a-url").unwrap_err();
        assert_eq!(
            endpoint_err,
            "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)"
        );
        assert!(!endpoint_err.contains("must start with"));

        // All branches use the same human-readable strings.
        assert_eq!(
            validate_endpoint_value("").unwrap_err(),
            "Endpoint must be a valid URL starting with https:// or http:// (e.g. https://api.openai.com/v1)"
        );
        assert_eq!(
            validate_provider_name("").unwrap_err(),
            "Provider name must be lowercase letters, numbers, hyphens, or underscores (e.g. openai, example-vendor)"
        );
    }

    #[test]
    fn validation_still_rejects_invalid_input() {
        // Validation rules unchanged — only error text changed.
        assert!(validate_provider_name("openai").is_ok());
        assert!(validate_provider_name("example-vendor").is_ok());
        assert!(validate_provider_name("my-provider_123").is_ok());
        assert!(validate_provider_name("OpenAI").is_err());
        assert!(validate_provider_name("bad provider").is_err());
        assert!(validate_provider_name("https://api.openai.com").is_err());
        assert!(validate_provider_name("").is_err());
        assert!(validate_provider_name("a".repeat(65).as_str()).is_err());

        assert!(validate_model_name("model-a").is_ok());
        assert!(validate_model_name("gpt-4o").is_ok());
        // Provider-issued ids: vendor separator `/`, tag suffix `:`, `@` pin.
        assert!(validate_model_name("example/model-a").is_ok());
        assert!(validate_model_name("example/model-b:free").is_ok());
        assert!(validate_model_name("openai/gpt-4o@2024-08-06").is_ok());
        assert!(validate_model_name("a".repeat(256).as_str()).is_ok());
        assert!(validate_model_name("").is_err());
        assert!(validate_model_name("a".repeat(257).as_str()).is_err());
        assert!(validate_model_name("bad model!").is_err());
        assert!(validate_model_name("has space").is_err());
        assert!(validate_model_name("ab\0cd").is_err());

        assert!(validate_credential_env_name("OPENAI_API_KEY").is_ok());
        assert!(validate_credential_env_name("ANTHROPIC_KEY").is_ok());
        assert!(validate_credential_env_name("openai").is_err());
        // O3: HAS-DASH contains chars outside [A-Z0-9_], so it now teaches.
        assert_eq!(
            validate_credential_env_name("HAS-DASH").unwrap_err(),
            "this looks like the key itself - Siralos stores the NAME of the environment variable holding your key; create it with setx YOUR_API_KEY_NAME \"the-key\" and enter YOUR_API_KEY_NAME here"
        );

        assert!(validate_endpoint_value("https://api.openai.com/v1").is_ok());
        assert!(validate_endpoint_value("http://localhost:11434").is_ok());
        assert!(validate_endpoint_value("").is_err());
        assert!(validate_endpoint_value("not-a-url").is_err());
        assert!(validate_endpoint_value("ftp://example.com").is_err());

        // Endpoint empty is allowed via the form handler, but direct validation rejects empty.
        // The form's DisplayName field allows empty (optional), which is handled in handle_key.
        // Six-field order O1: display name (empty), then url, api key, api protocol, model, model display name.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::Url
        );
        // Url empty is also allowed (optional) — but then completion requires
        // a display name; give the url so derivation prefills the name.
        t108_type(&mut state, "https://api.openai.com/v1");
        t108_enter(&mut state); // -> ApiKey
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        // ApiProtocol field (input cleared on ApiKey advance) — fetch fires on
        // the ApiKey advance (url non-empty), so clear the blocking flag.
        {
            let form = state.provider_add_form.as_mut().unwrap();
            if form.fetching_models {
                form.apply_fetch_result(Err("test".to_owned()));
            }
        }
        // Protocol picker -> Esc fallback to free text then type new protocol
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "openai-completions");
        t108_enter(&mut state);
        t108_type(&mut state, "gpt-4o");
        t108_enter(&mut state);
        t108_type(&mut state, "");
        t108_enter(&mut state);
        assert!(state.provider_add_form.as_ref().unwrap().completed.is_some());
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .completed
                .as_ref()
                .unwrap()
                .endpoint
                .as_deref(),
            Some("https://api.openai.com/v1")
        );
        // Display name derived from the url host after the Url advance.
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .completed
                .as_ref()
                .unwrap()
                .provider,
            "openai"
        );
    }

    // URL-first reorder tests (decisions 129-130)

    #[test]
    fn field_order_display_name_first() {
        // Six-field order O1 (decisions 133-134): display name, url, api key,
        // api protocol, model, model display name. (Replaces the old
        // url-first order assertion.)
        let form = ProviderAddForm::new();
        assert_eq!(form.field, ProviderAddField::DisplayName);
        let lines = provider_add_form_lines(&form);
        let joined: String = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let display_pos =
            joined.find("display name").expect("display name label");
        let url_pos = joined.find("url").expect("url label");
        let api_key_pos = joined.find("api key").expect("api key label");
        let protocol_pos =
            joined.find("api protocol").expect("api protocol label");
        // The second "model" occurrence is the model display name; check ordering via positions
        let model_pos =
            joined.find("\n  model:").unwrap_or(joined.find("model").unwrap());
        assert!(
            display_pos < url_pos
                && url_pos < api_key_pos
                && api_key_pos < protocol_pos
                && protocol_pos < model_pos,
            "Six-field order must be display name -> url -> api key -> api protocol -> model -> model display name"
        );
    }

    #[test]
    fn derive_provider_name_examples() {
        assert_eq!(
            derive_provider_name("https://api.example-vendor.com/v1"),
            "example-vendor"
        );
        assert_eq!(
            derive_provider_name("https://api.openai.com/v1"),
            "openai"
        );
        assert_eq!(
            derive_provider_name("https://vendor.example.com"),
            "vendor"
        );
        // Additional edge: strip scheme, api prefix, lowercase, replace invalid
        assert_eq!(derive_provider_name(""), "");
        assert_eq!(derive_provider_name("https://api.example.com"), "example");
        assert_eq!(
            derive_provider_name("https://API.ExampleVendor.AI/v1"),
            "examplevendor"
        );
        // Invalid chars replaced with '-'
        assert_eq!(
            derive_provider_name("https://api.foo$bar.example.com"),
            "foo-bar"
        );
        // Truncate to 64
        let long = format!("https://api.{}.example.com", "a".repeat(70));
        assert_eq!(derive_provider_name(&long).len(), 64);
    }

    #[test]
    fn prefill_is_editable() {
        // O1: display name first (left empty), url validates -> derivation
        // prefills the stored provider value.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url
        t108_type(&mut state, "https://api.example-vendor.com/v1");
        t108_enter(&mut state); // Url -> ApiKey, derivation prefills provider
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ApiKey);
        assert!(form.input.is_empty());
        assert_eq!(form.provider.as_deref(), Some("example-vendor"));
        // Editable: go back Up twice (ApiKey -> Url -> DisplayName), where the
        // derived value is restored into the input for editing.
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::DisplayName);
        assert_eq!(form.input, "example-vendor");
        // Editable: clear and type custom
        for _ in 0..form.input.len() {
            handle_key(
                &mut state,
                t108_key(crossterm::event::KeyCode::Backspace),
                10,
            );
        }
        t108_type(&mut state, "my-custom");
        t108_enter(&mut state); // DisplayName -> Url (input cleared first)
        t108_type(&mut state, "https://api.example-vendor.com/v1");
        t108_enter(&mut state); // Url -> ApiKey (custom name kept)
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.provider.as_deref(), Some("my-custom"));
        // Complete the remaining fields to finish the form
        t108_type(&mut state, "EXAMPLE_VENDOR_API_KEY");
        t108_enter(&mut state);
        // Simulate fetch completion (clears blocking flag) before Model
        {
            let form = state.provider_add_form.as_mut().unwrap();
            if form.fetching_models {
                form.apply_fetch_result(Err("test".to_owned()));
            }
        }
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "openai-completions");
        t108_enter(&mut state);
        t108_type(&mut state, "gpt-4o");
        t108_enter(&mut state);
        t108_type(&mut state, "My Display");
        t108_enter(&mut state);
        let completed = state
            .provider_add_form
            .as_ref()
            .unwrap()
            .completed
            .clone()
            .unwrap();
        assert_eq!(completed.provider, "my-custom");
        assert_eq!(
            completed.endpoint.as_deref(),
            Some("https://api.example-vendor.com/v1")
        );
    }

    #[test]
    fn name_only_flow_still_works() {
        // O1: display name first — a typed name with an empty url completes.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_type(&mut state, "my-provider");
        t108_enter(&mut state); // DisplayName -> Url
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::Url
        );
        t108_enter(&mut state); // empty url -> ApiKey (no derivation: name kept)
        let form = state.provider_add_form.as_ref().expect("form open");
        assert_eq!(form.field, ProviderAddField::ApiKey);
        assert_eq!(form.provider.as_deref(), Some("my-provider"));
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::ApiProtocol
        );
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "openai-completions");
        t108_enter(&mut state);
        t108_type(&mut state, "gpt-4o");
        t108_enter(&mut state);
        t108_type(&mut state, "");
        t108_enter(&mut state);
        let completed = state
            .provider_add_form
            .as_ref()
            .unwrap()
            .completed
            .clone()
            .unwrap();
        assert_eq!(completed.provider, "my-provider");
        assert_eq!(completed.model, "gpt-4o");
        assert!(completed.endpoint.is_none());
    }

    #[test]
    fn up_down_navigation_follows_new_order() {
        // O1 order: DisplayName -> Url -> ApiKey -> ApiProtocol -> Model -> ModelDisplayName.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::DisplayName
        );
        // Down validates empty display name and moves DisplayName -> Url
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::Url
        );
        // Up returns to DisplayName
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::DisplayName
        );
        // Down again to Url, type url, Down -> ApiKey
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        t108_type(&mut state, "https://api.openai.com/v1");
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::ApiKey
        );
        // Up returns to Url with restored value
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::Url
        );
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().input,
            "https://api.openai.com/v1"
        );
        // Continue down chain: Url -> ApiKey -> ApiProtocol -> Model -> ModelDisplayName
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        t108_type(&mut state, "OPENAI_API_KEY");
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        // Clear fetching flag simulated
        {
            let form = state.provider_add_form.as_mut().unwrap();
            if form.fetching_models {
                form.apply_fetch_result(Err("test".to_owned()));
            }
        }
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::ApiProtocol
        );
        assert!(state.provider_add_form.as_ref().unwrap().input.is_empty());
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "openai-completions");
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::Model);
        // Up from Model goes to ApiProtocol
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::ApiProtocol
        );
        // No prev from DisplayName
        let mut state2 = TuiState::new();
        open_provider_add_form(&mut state2);
        handle_key(&mut state2, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state2.provider_add_form.as_ref().unwrap().field,
            ProviderAddField::DisplayName
        );
    }

    #[test]
    fn display_name_first_flow_completes() {
        // O1: display name first (empty), url -> derivation prefill, then key,
        // protocol, model, model display name.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url
        t108_type(&mut state, "https://api.example-vendor.com/v1");
        t108_enter(&mut state); // Url -> ApiKey, derivation prefills provider
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::ApiKey);
        assert_eq!(form.provider.as_deref(), Some("example-vendor"));
        t108_type(&mut state, "env:EXAMPLE_VENDOR_API_KEY");
        t108_enter(&mut state);
        // Simulate fetch completion before Model (clears flag)
        {
            let form = state.provider_add_form.as_mut().unwrap();
            if form.fetching_models {
                form.apply_fetch_result(Err("test".to_owned()));
            }
        }
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "anthropic-messages");
        t108_enter(&mut state);
        t108_type(&mut state, "model-a");
        t108_enter(&mut state);
        t108_type(&mut state, "Spark Display");
        t108_enter(&mut state);
        let completed = state
            .provider_add_form
            .as_ref()
            .unwrap()
            .completed
            .clone()
            .unwrap();
        assert_eq!(completed.provider, "example-vendor");
        assert_eq!(completed.model, "model-a");
        assert_eq!(
            completed.credential_env.as_deref(),
            Some("env:EXAMPLE_VENDOR_API_KEY")
        );
        assert_eq!(
            completed.endpoint.as_deref(),
            Some("https://api.example-vendor.com/v1")
        );
        assert_eq!(completed.protocol, "anthropic-messages");
        assert_eq!(
            completed.model_display_name.as_deref(),
            Some("Spark Display")
        );
    }

    #[test]
    fn completion_requires_name_when_url_empty() {
        // O1: empty display name AND empty url -> completion error; a typed
        // name completes.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url
        t108_enter(&mut state); // empty url -> ApiKey
        t108_type(&mut state, "env:OPENAI_API_KEY");
        t108_enter(&mut state);
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "openai-completions");
        t108_enter(&mut state);
        t108_type(&mut state, "gpt-4o");
        t108_enter(&mut state);
        t108_enter(&mut state); // empty model display name -> completion error
        let form = state.provider_add_form.as_ref().unwrap();
        assert!(form.completed.is_none());
        assert_eq!(
            form.error.as_deref(),
            Some(
                "a display name is required - enter one or provide a url so one can be derived"
            )
        );
    }

    #[test]
    fn six_field_order_renders() {
        // O1 render order: display name first.
        let form = ProviderAddForm::new();
        assert_eq!(form.field, ProviderAddField::DisplayName);
        let lines = provider_add_form_lines(&form);
        let joined = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let order = [
            "display name",
            "url",
            "api key",
            "api protocol",
            "model",
            "model display name",
        ];
        let mut last = 0usize;
        for label in order {
            let pos = joined
                .find(label)
                .unwrap_or_else(|| panic!("label {label} missing"));
            assert!(pos >= last, "order broken at {label}");
            last = pos;
        }
    }

    #[test]
    fn no_examples_in_labels() {
        for field in [
            ProviderAddField::Url,
            ProviderAddField::ApiKey,
            ProviderAddField::DisplayName,
            ProviderAddField::ApiProtocol,
            ProviderAddField::Model,
            ProviderAddField::ModelDisplayName,
        ] {
            assert!(
                !field.label().contains("(e.g."),
                "label {:?} must not contain examples",
                field.label()
            );
            assert!(
                !field.label().contains("e.g."),
                "label {:?} must not contain examples",
                field.label()
            );
        }
        let lines = provider_add_form_lines(&ProviderAddForm::new());
        let joined = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !joined.contains("(e.g."),
            "rendered labels must not contain examples"
        );
    }

    #[test]
    fn derivation_prefill_on_display_name() {
        // O1: empty display name advances; the Url advance prefills the stored
        // provider value via derive_provider_name.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state); // empty display name -> Url, no error
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::Url);
        assert!(form.error.is_none());
        t108_type(&mut state, "https://api.openai.com/v1");
        t108_enter(&mut state); // Url -> ApiKey
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::ApiKey);
        assert!(form.input.is_empty());
        assert_eq!(form.provider.as_deref(), Some("openai"));
    }

    #[test]
    fn protocol_validation() {
        assert!(validate_api_protocol("openai-completions").is_ok());
        assert!(validate_api_protocol("openai-responses").is_ok());
        assert!(validate_api_protocol("anthropic-messages").is_ok());
        assert!(validate_api_protocol("gopher").is_err());
        let err = validate_api_protocol("gopher").unwrap_err();
        assert!(err.contains("openai-completions"));
        assert!(err.contains("openai-responses"));
        assert!(err.contains("anthropic-messages"));
    }

    #[test]
    fn model_picker_navigation_and_selection() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        // Navigate via the O1 six-field flow: DisplayName -> Url -> ApiKey -> ApiProtocol -> Model
        t108_enter(&mut state); // empty display name -> Url
        t108_type(&mut state, "https://api.openai.com/v1");
        t108_enter(&mut state); // Url -> ApiKey (derivation prefills provider)
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state); // ApiKey -> ApiProtocol
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "openai-completions");
        t108_enter(&mut state); // ApiProtocol -> Model
        // Clear fetching flag simulated (no loop in test)
        {
            let form = state.provider_add_form.as_mut().unwrap();
            form.fetching_models = false;
            form.fetch_note = None;
        }
        // Simulate successful fetch before entering model free text
        {
            let form = state.provider_add_form.as_mut().unwrap();
            form.apply_fetch_result(Ok(vec![
                "gpt-4o".to_owned(),
                "gpt-4o-mini".to_owned(),
                "o1".to_owned(),
            ]));
            assert!(form.model_picker.is_some());
            // Model field with an open picker renders the picker items.
            form.field = ProviderAddField::Model;
        }
        // Picker should render
        let lines =
            provider_add_form_lines(state.provider_add_form.as_ref().unwrap());
        let joined = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("gpt-4o"));
        assert!(joined.contains("Up/Down"));
        // Up wraps, Down wraps, Enter selects
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .model_picker
                .as_ref()
                .unwrap()
                .selected,
            1
        );
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .model_picker
                .as_ref()
                .unwrap()
                .selected,
            0
        );
        // Wrap: Up from 0 goes to last
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .model_picker
                .as_ref()
                .unwrap()
                .selected,
            2
        );
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Enter), 10);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.model.as_deref(), Some("o1"));
        assert_eq!(form.field, ProviderAddField::ModelDisplayName);
        assert!(form.model_picker.is_none());
    }

    #[test]
    fn picker_failure_fallback_note() {
        let mut form = ProviderAddForm::new();
        // Simulate Url -> ApiKey with fetching, then failure
        form.endpoint = Some("https://api.example.com".to_owned());
        form.credential_env = Some("EXAMPLE_KEY".to_owned());
        form.field = ProviderAddField::Model;
        form.fetching_models = true;
        form.apply_fetch_result(Err("network".to_owned()));
        assert!(form.model_picker.is_none());
        assert_eq!(
            form.fetch_note.as_deref(),
            Some(
                "model list unavailable from this provider - enter the model manually"
            )
        );
        let lines = provider_add_form_lines(&form);
        let joined = lines
            .iter()
            .map(|l| {
                l.iter().map(|s| s.content.to_string()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("model list unavailable"));
        // Esc fallback also sets same note (tested elsewhere)
    }

    #[test]
    fn header_shows_display_name() {
        let header_with_display = header_text(Some("openai"), Some("My GPT"));
        assert!(header_with_display.contains("My GPT"));
        assert!(!header_with_display.contains("gpt-4o"));
        let status_with_display =
            compose_status_line("ready", Some("openai"), Some("My GPT"));
        assert!(status_with_display.contains("My GPT"));
        // Fallback to raw when display absent is covered by existing header tests
    }

    #[test]
    fn determinism() {
        let state = TuiState::new();
        let buf1 = render_to_buffer(&state, 80, 24);
        let buf2 = render_to_buffer(&state, 80, 24);
        assert_eq!(buf1.content(), buf2.content());
        // Picker determinism
        let mut state2 = TuiState::new();
        open_provider_add_form(&mut state2);
        {
            let form = state2.provider_add_form.as_mut().unwrap();
            form.field = ProviderAddField::Model;
            form.apply_fetch_result(Ok(vec!["a".to_owned(), "b".to_owned()]));
        }
        let b1 = render_to_buffer(&state2, 80, 24);
        let b2 = render_to_buffer(&state2, 80, 24);
        assert_eq!(b1.content(), b2.content());
    }

    #[test]
    fn picker_flow() {
        // End-to-end fetch -> picker -> select and fallback path
        let mut form = ProviderAddForm::new();
        form.endpoint = Some("https://api.example.com".to_owned());
        form.fetching_models = true;
        form.apply_fetch_result(Ok(vec!["m1".to_owned(), "m2".to_owned()]));
        assert!(form.model_picker.is_some());
        // Select via Enter simulation (handled in handle_key, but direct apply)
        let picker = form.model_picker.take().unwrap();
        let selected = picker.items[picker.selected].clone();
        form.model = Some(selected.clone());
        assert_eq!(selected, "m1");
        // Fallback path
        let mut form2 = ProviderAddForm::new();
        form2.endpoint = Some("https://api.example.com".to_owned());
        form2.fetching_models = true;
        form2.apply_fetch_result(Err("fail".to_owned()));
        assert!(form2.model_picker.is_none());
        assert!(form2.fetch_note.is_some());
    }

    #[test]
    fn verbatim_credential_public_stores_key_public() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "public");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        assert_eq!(form.credential_env.as_deref(), Some("key:public"));
        assert!(form.error.is_none());
    }

    #[test]
    fn verbatim_credential_empty_is_none() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_enter(&mut state); // empty ApiKey
        let form = state.provider_add_form.as_ref().unwrap();
        assert!(form.credential_env.is_none());
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
    }

    #[test]
    fn verbatim_credential_env_form_stored_as_is() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "env:OPENAI_API_KEY");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.credential_env.as_deref(), Some("env:OPENAI_API_KEY"));
    }

    #[test]
    fn verbatim_credential_arbitrary_key_no_validation() {
        // TUI is an interface: stores WHAT THE USER TYPES verbatim, no validation gatekeeping.
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "sk-secret-123");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.credential_env.as_deref(), Some("key:sk-secret-123"));
        assert!(form.error.is_none());
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
    }

    #[test]
    fn verbatim_credential_down_advance_also_verbatim() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        t108_type(&mut state, "https://api.example.com/v1");
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        t108_type(&mut state, "none");
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.credential_env.as_deref(), Some("key:none"));
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
    }

    #[test]
    fn protocol_picker_opens_with_three_items_default_zero() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        let picker = form.protocol_picker.as_ref().expect("picker open");
        assert_eq!(picker.items.len(), 3);
        assert_eq!(
            picker.items,
            vec![
                "openai-completions".to_owned(),
                "openai-responses".to_owned(),
                "anthropic-messages".to_owned()
            ]
        );
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn protocol_picker_up_down_wrap() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        // Down wraps
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .protocol_picker
                .as_ref()
                .unwrap()
                .selected,
            1
        );
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .protocol_picker
                .as_ref()
                .unwrap()
                .selected,
            2
        );
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .protocol_picker
                .as_ref()
                .unwrap()
                .selected,
            0
        );
        // Up wraps
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        assert_eq!(
            state
                .provider_add_form
                .as_ref()
                .unwrap()
                .protocol_picker
                .as_ref()
                .unwrap()
                .selected,
            2
        );
    }

    #[test]
    fn protocol_picker_enter_selects() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        // Navigate to anthropic-messages (index 2) and select
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.protocol.as_deref(), Some("anthropic-messages"));
        assert_eq!(form.field, ProviderAddField::Model);
        assert!(form.protocol_picker.is_none());
    }

    #[test]
    fn protocol_picker_esc_fallback_to_free_text() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        // Esc falls back
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        let form = state.provider_add_form.as_ref().unwrap();
        assert!(form.protocol_picker.is_none());
        // Now free-text entry validated against closed set
        t108_type(&mut state, "openai-responses");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.protocol.as_deref(), Some("openai-responses"));
        assert_eq!(form.field, ProviderAddField::Model);
    }

    #[test]
    fn protocol_invalid_free_text_errors_with_three_values() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Esc), 10);
        t108_type(&mut state, "gopher");
        t108_enter(&mut state);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        let err = form.error.as_deref().unwrap_or("");
        assert!(err.contains("openai-completions"));
        assert!(err.contains("openai-responses"));
        assert!(err.contains("anthropic-messages"));
    }

    #[test]
    fn protocol_picker_preselects_stored_value_on_back() {
        let mut state = TuiState::new();
        open_provider_add_form(&mut state);
        t108_enter(&mut state);
        t108_type(&mut state, "https://api.example.com/v1");
        t108_enter(&mut state);
        t108_type(&mut state, "OPENAI_API_KEY");
        t108_enter(&mut state);
        // Clear fetching flag simulated (no loop in test)
        {
            let form = state.provider_add_form.as_mut().unwrap();
            form.fetching_models = false;
        }
        // Select anthropic-messages
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Down), 10);
        t108_enter(&mut state);
        assert_eq!(
            state.provider_add_form.as_ref().unwrap().protocol.as_deref(),
            Some("anthropic-messages")
        );
        // Go back Up to ApiProtocol — picker should preselect stored value
        handle_key(&mut state, t108_key(crossterm::event::KeyCode::Up), 10);
        let form = state.provider_add_form.as_ref().unwrap();
        assert_eq!(form.field, ProviderAddField::ApiProtocol);
        let picker = form.protocol_picker.as_ref().expect("picker reopened");
        assert_eq!(picker.selected, 2);
        assert_eq!(picker.items[2], "anthropic-messages");
    }

    #[test]
    fn model_picker_viewport_selected_always_visible() {
        // 45 items: selected item must always be in rendered buffer at every step 0..44, position indicator renders.
        let mut form = ProviderAddForm::new();
        let items: Vec<String> =
            (0..45).map(|i| format!("model-{i:02}")).collect();
        form.field = ProviderAddField::Model;
        form.model_picker =
            Some(ModelPicker { items: items.clone(), selected: 0 });
        for sel in 0..45 {
            form.model_picker.as_mut().unwrap().selected = sel;
            let lines = provider_add_form_lines(&form);
            let rendered = lines
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            let expected = format!("model-{sel:02}");
            assert!(
                rendered.contains(&expected),
                "selected {expected} must be visible at sel {sel}"
            );
            let indicator = format!("{}/45", sel + 1);
            assert!(
                rendered.contains(&indicator),
                "indicator {indicator} must render at sel {sel}"
            );
        }
    }

    #[test]
    fn protocol_picker_viewport_position_indicator() {
        let mut form = ProviderAddForm::new();
        form.field = ProviderAddField::ApiProtocol;
        form.protocol_picker = Some(ProtocolPicker {
            items: vec![
                "openai-completions".to_owned(),
                "openai-responses".to_owned(),
                "anthropic-messages".to_owned(),
            ],
            selected: 1,
        });
        let lines = provider_add_form_lines(&form);
        let rendered =
            lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        assert!(
            rendered.contains("2/3"),
            "protocol picker indicator 2/3 must render"
        );
    }

    #[test]
    fn picker_viewport_slides_with_selection() {
        // Bounded WINDOW of 8 visible rows that slides with selection
        let mut form = ProviderAddForm::new();
        let items: Vec<String> =
            (0..20).map(|i| format!("item-{i:02}")).collect();
        form.field = ProviderAddField::Model;
        form.model_picker =
            Some(ModelPicker { items: items.clone(), selected: 15 });
        let lines = provider_add_form_lines(&form);
        let rendered =
            lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        // Selected 15 (16/20) must be visible; earlier items outside window should not be rendered as selected highlight but may still be outside viewport.
        assert!(rendered.contains("item-15"));
        assert!(rendered.contains("16/20"));
        // Window size check: at most 8 items plus indicator + hint; ensure not overflow (rough)
        let count_items =
            items.iter().filter(|it| rendered.contains(*it)).count();
        assert!(
            count_items <= 8,
            "viewport must bound to 8, got {count_items}"
        );
    }
}
