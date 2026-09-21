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

/// How long a painter waits before it paints the next frame.
///
/// While the reveal owes the reader text it waits NOTHING: the reveal renders a
/// character at a time (owner ruling), so matching the speed the model produces
/// means painting one frame per character as fast as frames can be painted.
/// The frame cost is then the only limiter -- measured at 2.2 ms in the
/// unoptimized build the dev flow runs (`cargo run`), which is a ceiling of
/// roughly 450 characters a second, several times higher than a reasoning
/// stream. Once nothing is owed, `idle` applies again, so a quiet UI does not
/// spin.
#[must_use]
pub const fn paint_interval(owed: bool, idle: Duration) -> Duration {
    if owed { Duration::ZERO } else { idle }
}

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

/// The rows a frame paints that are NOT stored transcript, and where they
/// belong (S3d).
///
/// Named fields because the placement IS the meaning: `above` renders above
/// the stored entry at `above_at` (the thinking block, which belongs above the
/// answer it explains), and `below` trails the last stored entry (the answer's
/// growing line, the gap that separates the indicator). Empty slices
/// reproduce the plain stored transcript exactly, which is what keeps every
/// reasoning-free frame byte-identical.
struct FrameRows<'a> {
    above: &'a [(&'a str, Option<&'a str>)],
    above_at: usize,
    below: &'a [(&'a str, Option<&'a str>)],
}

/// The transcript rows the viewport shows, wrapped from the TAIL.
///
/// Returns owned `(row, style)` pairs for exactly the rows a frame paints, in
/// order. One stored line expands to one or more rows via [`wrap_line_to_width`];
/// timestamps wrap the same way (short in practice, one row) and keep the dim
/// stamp style per row. `rows` carries the non-stored rows and their placement
/// ([`FrameRows`]).
///
/// This exists because the previous shape cloned and wrapped the WHOLE
/// transcript every frame, so a frame cost grew with the session -- measured at
/// 5 ms with 24 lines and 21 ms with 1200 in an unoptimized build -- which no
/// per-character reveal can afford. The work here is bounded by the viewport
/// plus whatever the reader scrolled past it, and the rows are the same ones the
/// whole-transcript version produced (the tests pin that equivalence).
fn visible_transcript_rows(
    transcript: &[TranscriptEntry],
    fallback_lines: &[String],
    rows: &FrameRows<'_>,
    inner_width: usize,
    height: usize,
    scroll_offset: u16,
) -> Vec<(String, Style)> {
    if height == 0 {
        return Vec::new();
    }
    // Everything the window needs: the viewport plus the rows scrolled past it.
    let want = height.saturating_add(scroll_offset as usize);
    // Collected BACKWARDS (tail first), so nothing before the window is wrapped.
    let mut tail: Vec<(String, Style)> = Vec::with_capacity(want);
    let push_entry = |text: &str,
                      timestamp: Option<&str>,
                      tail: &mut Vec<(String, Style)>| {
        if tail.len() >= want {
            return;
        }
        let text_style = style_for_transcript_line(text);
        let mut rows: Vec<(String, Style)> =
            wrap_line_to_width(text, inner_width)
                .into_iter()
                .map(|row| (row, text_style))
                .collect();
        if let Some(ts) = timestamp {
            let dim = Style::default().fg(Color::DarkGray);
            rows.extend(
                wrap_line_to_width(ts, inner_width)
                    .into_iter()
                    .map(|row| (row, dim)),
            );
        }
        for row in rows.into_iter().rev() {
            tail.push(row);
        }
    };
    // The harness and the older tests assign `transcript_lines` directly.
    let stored_len = if transcript.is_empty() {
        fallback_lines.len()
    } else {
        transcript.len()
    };
    // A stale anchor must never point past the end: the block then renders at
    // the tail, which is where it used to live, instead of panicking.
    let at = rows.above_at.min(stored_len);
    let stored = |index: usize| -> (&str, Option<&str>) {
        if transcript.is_empty() {
            (fallback_lines[index].as_str(), None)
        } else {
            let entry = &transcript[index];
            (entry.text.as_str(), entry.timestamp.as_deref())
        }
    };
    // Reading order: `below`, then the stored entries after the anchor, then
    // `above`, then everything the reader has already read. Collected in
    // reverse, so the loops run backwards from the last row painted.
    for (text, timestamp) in rows.below.iter().rev() {
        push_entry(text, *timestamp, &mut tail);
    }
    for index in (at..stored_len).rev() {
        let (text, timestamp) = stored(index);
        push_entry(text, timestamp, &mut tail);
    }
    for (text, timestamp) in rows.above.iter().rev() {
        push_entry(text, *timestamp, &mut tail);
    }
    for index in (0..at).rev() {
        let (text, timestamp) = stored(index);
        push_entry(text, timestamp, &mut tail);
    }
    // The same slice the whole-transcript version produced: skip what the
    // reader scrolled past (clamped exactly as that version clamped it), take
    // one viewport, and hand it back in reading order.
    let len = tail.len();
    let scroll = (scroll_offset as usize).min(len.saturating_sub(height));
    // The window ends `scroll` rows before the LAST row, so in this
    // tail-first collection it starts at `scroll` and runs one viewport.
    let start = scroll;
    let end = (scroll + height).min(len);
    tail[start..end].iter().rev().cloned().collect()
}

/// The rows of the transcript area for one frame (S3d).
///
/// The thinking block renders ABOVE the stored entry it is anchored to --
/// this turn's model output -- and the answer's in-flight line trails the
/// stored transcript. Both painters call this, so the two render paths cannot
/// disagree about where the block goes.
fn transcript_frame_rows(
    state: &TuiState,
    inner_width: usize,
    height: usize,
) -> Vec<(String, Style)> {
    let thinking = state.reasoning_block_lines();
    let above: Vec<(&str, Option<&str>)> =
        thinking.iter().map(|line| (line.as_str(), None)).collect();
    let mut below: Vec<(&str, Option<&str>)> = Vec::with_capacity(2);
    if !state.stream_tail.is_empty() {
        below.push((state.stream_tail.as_str(), None));
    }
    if !thinking.is_empty() && state.busy_since.is_some() {
        // Keep the indicator visually SEPARATE from the block.
        below.push(("", None));
    }
    visible_transcript_rows(
        &state.transcript,
        &state.transcript_lines,
        &FrameRows {
            above: &above,
            above_at: state.reasoning_anchor,
            below: &below,
        },
        inner_width,
        height,
        state.scroll_offset,
    )
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
    /// Endpoint the worker composed the session with (C2 step 3, decision 168
    /// R3). Display only: the picker shows it, and `/provider` renders it.
    pub endpoint: Option<String>,
    /// Protocol the worker built the provider with (R3).
    pub protocol: String,
    /// The credential in its ALREADY-REDACTED display form (R2). The raw value
    /// never reaches the frontend, so this is what the provider surfaces show.
    pub credential_display: Option<String>,
    /// Whether the declared credential RESOLVED. The `/models` arm decides on
    /// this today and the frontend cannot recompute it, so the worker answers
    /// it as a fact (never the value).
    pub credential_resolved: bool,
    /// The context-usage suffix the worker's status line carries
    /// (` | ctx N/4096`), empty when the subsystem is off. Cached because a
    /// TRANSIENT status (the add-flow's "fetching models...") must keep the
    /// readout, and the frontend no longer holds the metrics that build it.
    pub context_suffix: String,
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
    /// The stored-transcript index the thinking block renders ABOVE (S3d).
    ///
    /// Thinking is what the model produced FIRST and the answer is what it
    /// produced FROM it, so the block sits above the answer. Appending it
    /// after the transcript instead -- where it used to live -- put it below
    /// every answer line the reveal had already committed, which is the
    /// inversion the owner reported.
    pub reasoning_anchor: usize,
    /// The anchor the CURRENT turn takes when its thinking arrives (S3d).
    ///
    /// [`TuiState::begin_turn`] arms it with the transcript as it stands after
    /// the submitted prompt; the first streamed delta consumes it. It stays
    /// `None` between turns, so a turn that never reasons leaves the block
    /// exactly where the turn that produced it put it.
    pub pending_reasoning_anchor: Option<usize>,
    /// When the current turn started, for the pulsing `working` line.
    pub busy_since: Option<std::time::Instant>,
    /// Answer text received but not yet revealed (S3c).
    pub stream_buffer: String,
    /// The revealed text of the INCOMPLETE line, rendered as a growing row
    /// so a long answer appears left to right instead of popping in whole.
    pub stream_tail: String,
    /// How much of `reasoning` has been revealed.
    pub reasoning_shown: usize,
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
            endpoint: None,
            protocol: String::new(),
            credential_display: None,
            credential_resolved: false,
            context_suffix: String::new(),
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
            reasoning_anchor: 0,
            pending_reasoning_anchor: None,
            busy_since: None,
            stream_buffer: String::new(),
            stream_tail: String::new(),
            reasoning_shown: 0,
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

    /// Open a turn (S1/S3d): start the `working` clock and ARM the thinking
    /// block's anchor.
    ///
    /// The anchor is the transcript as it stands AFTER the submitted prompt,
    /// and the turn's first streamed thinking takes it. Anchoring at the turn's
    /// start -- rather than at that first delta -- is what keeps the block
    /// above the answer's FIRST line: the reveal commits answer lines into the
    /// transcript while it runs, so a later anchor would leave the earliest of
    /// them above the block, which is the inversion the owner reported.
    pub fn begin_turn(&mut self, now: std::time::Instant) {
        self.busy_since = Some(now);
        self.pending_reasoning_anchor = Some(self.effective_transcript_len());
    }

    /// Close a turn (S3d): stop the `working` clock and disarm the anchor.
    ///
    /// Disarming -- not re-anchoring -- is what keeps the block with the turn
    /// that produced it: a later turn that streams no thinking leaves it
    /// exactly where it was, instead of dragging an older trace down the
    /// conversation.
    pub fn end_turn(&mut self) {
        self.busy_since = None;
        self.pending_reasoning_anchor = None;
    }

    /// Release the next character owed to the reader, if any (S3c).
    ///
    /// ONE character per call, by construction: the text is rendered a
    /// character at a time, so the character rate IS the frame rate. The paint
    /// path is the only caller -- one call, one painted frame, one character --
    /// which is why there is no timer, no rate budget and no catch-up here. The
    /// painters own the cadence ([`paint_interval`]); this owns the ORDER.
    ///
    /// S3d: the THINKING is released first, because the block renders above the
    /// answer -- a reader meets the model's output in the order the model
    /// produced it. That costs the answer nothing in practice: the reveal is
    /// limited by the frame cost, several times faster than a reasoning stream,
    /// so it has already drained the thinking by the time answer text arrives.
    pub fn reveal_char(&mut self) {
        if self.reasoning_shown < self.reasoning.len() {
            let ch = self.reasoning[self.reasoning_shown..]
                .chars()
                .next()
                .expect("an index below the length starts a character");
            self.reasoning_shown += ch.len_utf8();
            return;
        }
        if let Some(ch) = self.stream_buffer.chars().next() {
            self.stream_buffer.remove(0);
            if ch == '\n' {
                let line = std::mem::take(&mut self.stream_tail);
                self.push_line(line);
            } else {
                self.stream_tail.push(ch);
            }
        }
    }

    /// Whether the reader is still owed text.
    ///
    /// The character cadence applies only while something is pending; an idle
    /// UI keeps the ordinary frame cadence.
    #[must_use]
    pub fn reveal_pending(&self) -> bool {
        !self.stream_buffer.is_empty()
            || self.reasoning_shown < self.reasoning.len()
    }

    /// Append streamed thinking, bounded to [`REASONING_BYTES`] of ALREADY
    /// REVEALED text.
    ///
    /// The bound keeps a long trace from growing the state without end. What the
    /// reader is still owed is NEVER dropped: with a per-character reveal a fast
    /// trace can be thousands of characters ahead, and trimming that away would
    /// skip text the reader never saw (and jump the visible text forward). The
    /// buffer therefore exceeds the bound while a backlog exists and settles
    /// back to it once the reveal has caught up.
    pub fn push_reasoning(&mut self, text: &str) {
        if let Some(at) = self.pending_reasoning_anchor.take() {
            // The turn's thinking has arrived: take the anchor the turn opened
            // with. Clamped, because the transcript can only have grown since
            // then and a stale index must never point past the end.
            self.reasoning_anchor = at.min(self.effective_transcript_len());
        }
        self.reasoning.push_str(text);
        let excess = self.reasoning.len().saturating_sub(REASONING_BYTES);
        let droppable = excess.min(self.reasoning_shown);
        if droppable == 0 {
            return;
        }
        // The reveal offset is a byte index into THIS buffer, so the cut has to
        // land on a character boundary or the next slice panics.
        let boundary = self
            .reasoning
            .char_indices()
            .map(|(index, _)| index)
            .find(|index| *index >= droppable)
            .unwrap_or(self.reasoning.len());
        self.reasoning.drain(..boundary);
        self.reasoning_shown = self.reasoning_shown.saturating_sub(boundary);
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
    // `visible_transcript_rows` (word boundaries, hard-break long tokens);
    // stored text is never mutated. Scroll windows over rows, so the tail
    // of a long line stays readable instead of clipping off-screen.
    let height = transcript_area.height as usize;
    // S3d: the thinking block renders ABOVE this turn's model output and the
    // answer's current line trails it; both scroll with the conversation and
    // need no layout surgery. The answer's growing line is what makes the
    // answer read left to right instead of appearing whole.
    let wrapped =
        transcript_frame_rows(state, transcript_area.width as usize, height);
    let expanded: Vec<Line<'_>> = wrapped
        .iter()
        .map(|(row, style)| Line::from(row.as_str()).style(*style))
        .collect();

    let transcript = Paragraph::new(Text::from(expanded))
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
    // The same rows the `Frame` path builds, and the same bounded work: see
    // [`visible_transcript_rows`].
    let wrapped =
        transcript_frame_rows(state, transcript_area.width as usize, h);
    let expanded: Vec<Line<'_>> = wrapped
        .iter()
        .map(|(row, style)| Line::from(row.as_str()).style(*style))
        .collect();
    let transcript = Paragraph::new(Text::from(expanded))
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
        // While the reader is owed text the frame cadence IS the character
        // cadence and nothing throttles it (see `paint_interval`); otherwise
        // the ordinary interval.
        let interval = paint_interval(
            self.state.borrow().reveal_pending(),
            REDRAW_INTERVAL,
        );
        let due = match self.last_redraw.get() {
            None => true,
            Some(last) => now.duration_since(last) >= interval,
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
            // S3c: hand the text to the REVEAL rather than the transcript, so a
            // line the provider sends whole still appears a character at a
            // time, and an unfinished line is visible as it grows. The
            // CHARACTER is released by the paint (one per frame), never here:
            // this only buffers and asks for the frame that will release it.
            state.stream_buffer.push_str(&text);
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
/// `EnableMouseCapture` is deliberately NOT sent at entry -- capture is OFF
/// by default so native select, copy and paste work (owner ruling) -- and `drop`
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

/// Validate provider field: [a-z0-9_-]{1,64}, non-empty, no NUL.
/// Human-readable error (D2) — validation rule unchanged.
fn validate_provider_name(value: &str) -> Result<(), String> {
    if !siralos_core::composition::is_provider_id(value) {
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
fn validate_model_name(value: &str) -> Result<(), String> {
    if !siralos_core::composition::is_model_id(value) {
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
mod tests;
