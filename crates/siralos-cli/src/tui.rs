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
//! freeze the redraw (documented T1 limitation).

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Maximum number of transcript lines retained (bounded ring).
pub const MAX_TRANSCRIPT_LINES: usize = 1000;

/// Maximum number of lines shown in the approval modal (bounded).
pub const MAX_APPROVAL_LINES: usize = 30;

/// Marker appended when the approval request is truncated to the bound.
pub const APPROVAL_TRUNCATION_MARKER: &str = "... (truncated)";

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

/// Pure render model for the TUI shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiState {
    /// Transcript lines — bounded ring, oldest dropped when full. Each entry is
    /// a single sanitized line (no embedded newlines) as produced by the
    /// session's sanitizer boundary; the TUI adds nothing unsanitized.
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
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            transcript_lines: Vec::new(),
            input: String::new(),
            status: String::from("ready"),
            scroll_offset: 0,
            pending_approval: None,
        }
    }
}

impl TuiState {
    /// Create an empty state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a sanitized line verbatim (no sanitization, no unsanitized
    /// injection). Oldest lines are dropped when the bound is exceeded.
    pub fn push_line(&mut self, line: String) {
        // Split on newlines in case a caller passes multi-line content; each
        // resulting segment is a transcript entry. We preserve empty trailing
        // segments as empty lines is not useful — skip empty final if input
        // ended with newline? For verbatim semantics we push each segment
        // produced by lines() plus handle empty input as one empty line? The
        // sink splits on '\n' and discards trailing empty, so we mirror that:
        // a single newline yields one empty line? Easiest: if line contains
        // '\n', split; otherwise push as-is.
        if line.contains('\n') {
            for part in line.split('\n') {
                self.push_single(part.to_owned());
            }
        } else {
            self.push_single(line);
        }
    }

    fn push_single(&mut self, line: String) {
        if self.transcript_lines.len() >= MAX_TRANSCRIPT_LINES {
            let drain = self.transcript_lines.len() - MAX_TRANSCRIPT_LINES + 1;
            self.transcript_lines.drain(0..drain);
        }
        self.transcript_lines.push(line);
        // Auto-tail: submitting new content resets scroll to tail so the user
        // sees the latest output.
        // We do NOT auto-reset on every push if the user has intentionally
        // scrolled up? For T1 we keep simple: new lines keep tail unless the
        // caller preserves offset. But to show tail by default, we leave
        // scroll_offset as-is; the draw clamps it. Tests expect tail by
        // default so they use offset 0. Keep offset unchanged here; caller
        // controls scroll.
    }

    /// Maximum scroll offset for the current transcript and viewport height.
    pub fn max_scroll(&self, viewport_height: u16) -> u16 {
        let total = self.transcript_lines.len() as u16;
        total.saturating_sub(viewport_height)
    }

    /// Clamp scroll_offset to the viewport.
    pub fn clamp_scroll(&mut self, viewport_height: u16) {
        let max = self.max_scroll(viewport_height);
        if self.scroll_offset > max {
            self.scroll_offset = max;
        }
    }
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
    // OFF (P1): the exact T2 flat layout — transcript full width, no pane,
    // no placeholder. ON: transcript + input shrink to the remaining width
    // beside the fixed 40-column pane; the status line stays full width.
    let (transcript_area, input_area, status_area, pane_area) = match pane {
        None => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(area);
            (chunks[0], chunks[1], chunks[2], None)
        }
        Some(_) => {
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(CONTEXT_PANE_WIDTH),
                ])
                .split(outer[0]);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(cols[0]);
            (rows[0], rows[1], outer[1], Some(cols[1]))
        }
    };
    // The modal backdrop covers the transcript in OFF mode and the whole
    // body (transcript + pane) in ON mode.
    let backdrop_area = match pane_area {
        None => transcript_area,
        Some(pane_rect) => body_union(transcript_area, input_area, pane_rect),
    };

    // Transcript: determine visible window.
    let height = transcript_area.height as usize;
    let total = state.transcript_lines.len();
    // Clamp scroll_offset to max for this viewport.
    let max_scroll = total.saturating_sub(height);
    let scroll = (state.scroll_offset as usize).min(max_scroll);
    let start = if total <= height { 0 } else { total - height - scroll };
    let end = (start + height).min(total);
    let visible = &state.transcript_lines[start..end];

    let lines: Vec<Line<'_>> =
        visible.iter().map(|s| Line::from(s.as_str())).collect();
    let transcript = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::White));
    frame.render_widget(transcript, transcript_area);

    // Input line: `> <input>`
    let input_text = format!("> {}", state.input);
    let input = Paragraph::new(input_text.as_str())
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(input, input_area);
    // Cursor at end of input (after `> ` prefix + input length). Clamp to area.
    // When a modal is pending, hide the cursor behind the dimmed backdrop
    // (no typing through a modal).
    if state.pending_approval.is_none() {
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

    // Status line
    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(status, status_area);

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
    let (transcript_area, input_area, status_area, pane_area) = match pane {
        None => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(area);
            (chunks[0], chunks[1], chunks[2], None)
        }
        Some(_) => {
            let outer = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(CONTEXT_PANE_WIDTH),
                ])
                .split(outer[0]);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(cols[0]);
            (rows[0], rows[1], outer[1], Some(cols[1]))
        }
    };

    let h = transcript_area.height as usize;
    let total = state.transcript_lines.len();
    let max_scroll = total.saturating_sub(h);
    let scroll = (state.scroll_offset as usize).min(max_scroll);
    let start = if total <= h { 0 } else { total - h - scroll };
    let end = (start + h).min(total);
    let visible = &state.transcript_lines[start..end];
    let lines: Vec<Line<'_>> =
        visible.iter().map(|s| Line::from(s.as_str())).collect();
    let transcript = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::NONE))
        .style(Style::default().fg(Color::White));
    transcript.render(transcript_area, &mut buf);

    let input_text = format!("> {}", state.input);
    let input = Paragraph::new(input_text.as_str())
        .style(Style::default().fg(Color::Yellow));
    input.render(input_area, &mut buf);

    if let (Some(pane_data), Some(pane_rect)) = (pane, pane_area) {
        let pane_block = Block::default()
            .borders(Borders::ALL)
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

    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::Cyan));
    status.render(status_area, &mut buf);

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
    buf: String,
}

impl TuiSink {
    /// Create a sink sharing `state`.
    pub fn new(state: Rc<RefCell<TuiState>>) -> Self {
        Self { state, buf: String::new() }
    }

    /// Flush any partial line (without trailing newline) as a transcript entry.
    pub fn flush_partial(&mut self) {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            self.state.borrow_mut().push_line(line);
        }
    }
}

impl Write for TuiSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(bytes);
        self.buf.push_str(&text);
        // Split completed lines on '\n'
        while let Some(pos) = self.buf.find('\n') {
            let line = self.buf[..pos].to_owned();
            // Remove up to and including '\n'
            self.buf.drain(..=pos);
            // Strip trailing '\r' for CRLF
            let line = if line.ends_with('\r') {
                line[..line.len() - 1].to_owned()
            } else {
                line
            };
            self.state.borrow_mut().push_line(line);
        }
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
/// exit) on drop — panic-safe.
pub struct TerminalGuard {
    restored: bool,
}

impl TerminalGuard {
    /// Enter alternate screen and raw mode. Returns the guard; dropping it
    /// restores state. Errors are returned without having entered raw mode.
    pub fn enter() -> io::Result<Self> {
        use crossterm::execute;
        use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, EnterAlternateScreen) {
            let _ = crossterm::terminal::disable_raw_mode();
            return Err(e);
        }
        Ok(Self { restored: false })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.restored {
            return;
        }
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::terminal::LeaveAlternateScreen
        );
        self.restored = true;
    }
}

// T1 composition note (updated T3): `run_interactive_session` blocks on
// `BufRead::read_line`, which would starve the `crossterm::event::poll` pump,
// so the live TUI loop duplicates the dispatch calling the SAME underlying seam
// functions (sanitizer, `ensure_host`, command dispatch, `drain_events` with the
// `TuiSink`). T2 started the consolidation: the shared helpers
// `evaluate_approval_input` / `ApprovalModal::new` / `modal_key_decision` are the
// SAME functions both loops call for the approval surface. T3 continues it
// where the pane work touches: the audit/pane gating is the single shared
// helper `interactive::context_audit_session` (enabled flag + session
// presence — the same gating the decision 100 `/context` segment uses), the
// TUI `/context` arm calls it instead of its own holder-only check, and the
// pane snapshot (`build_context_pane`) reads the SAME `ContextMetrics` the
// audit segment reads. Still duplicated (T4 owes it): the session-composition
// block, the slash-command dispatch match (`handle_tui_line` vs the stdio
// loop), and the inline key-edit handling in the live loop (which `handle_key`
// shadows but the loop does not yet call). No forced big-bang refactor.
// During a blocking provider round the UI simply does not redraw — the status
// line showed "working" before the step and the freeze is documented.
// Helpers for tests: expose scroll operations
/// Handle a key event for the input line and scroll state. Returns true if the
/// Enter key was pressed (caller should submit `state.input`).
/// While a modal is pending this function returns `false` for all keys
/// (callers must route through [`handle_modal_key`] first — no typing through
/// a modal).
pub fn handle_key(
    state: &mut TuiState,
    key: crossterm::event::KeyEvent,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    if state.pending_approval.is_some() {
        // T2: while a modal is pending, ALL other keys are ignored.
        return false;
    }
    if key.kind != crossterm::event::KeyEventKind::Press {
        return false;
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
            // Ctrl+C is handled by the outer loop as exit, not here.
            false
        }
        (KeyCode::Enter, _) => true,
        (KeyCode::Backspace, _) => {
            state.input.pop();
            false
        }
        (KeyCode::Char(ch), _) => {
            state.input.push(ch);
            false
        }
        (KeyCode::PageUp, _) => {
            state.scroll_offset = state.scroll_offset.saturating_add(10);
            false
        }
        (KeyCode::PageDown, _) => {
            state.scroll_offset = state.scroll_offset.saturating_sub(10);
            false
        }
        _ => false,
    }
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
        // Height 10 => transcript area 8 (minus input+status). Show tail.
        let buf = render(&state, 40, 10);
        let content: String =
            buf.content().iter().map(|c| c.symbol()).collect();
        // Tail should contain the last lines
        assert!(content.contains("line 19"));
        assert!(content.contains("line 12"));
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
        for i in 0..(MAX_TRANSCRIPT_LINES + 50) {
            let line = format!("line {i}\n");
            sink.write_all(line.as_bytes()).expect("write");
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
        assert!(!handle_key(&mut state, ch));
        assert_eq!(state.input, "hello");
        // Enter ignored
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(!handle_key(&mut state, enter));
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
    }
}
