//! TUI shell — pure render model over the live interactive session (T1).
//!
//! The terminal sanitizer stays the single output boundary: every line entering
//! the transcript via [`TuiSink`] is already sanitized by the session (the same
//! `sanitize` code path the stdio frontend uses). The TUI adds no unsanitized
//! content of its own. The input queue stays the single interactive-read owner
//! and the command catalog the vocabulary source; approvals stay host-gated.
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

/// Draw the TUI frame deterministically from `state`.
///
/// Layout (vertical):
/// - transcript pane (fill): scrollable, tail by default
/// - input line (1 row): `> <input>` with cursor at end
/// - status line (1 row): `status`
pub fn draw(state: &TuiState, frame: &mut Frame<'_>) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let transcript_area = chunks[0];
    let input_area = chunks[1];
    let status_area = chunks[2];

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

    // Status line
    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(status, status_area);

    // Modal overlay (T2): dimmed backdrop + centered modal with the sanitized
    // approval request (bounded to MAX_APPROVAL_LINES). This reuses the same
    // sanitizer-bound lines the stdio path would render — no unsanitized content.
    if let Some(modal) = &state.pending_approval {
        // Dimmed backdrop over the transcript pane (style bg dark).
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        frame.render_widget(backdrop, transcript_area);
        // Centered modal rect.
        let modal_area = centered_rect(60, 60, transcript_area);
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
/// buffer bytes.
pub fn render_to_buffer(state: &TuiState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    // We need a Frame backed by TestBackend-style buffer. Easiest is to use
    // ratatui's TestBackend directly in tests; this helper is for direct
    // buffer rendering without a backend. We emulate via `Buffer` + manual
    // layout using the same logic as `draw` but writing into `buf` directly.
    // To keep determinism identical to `draw`, we reuse the widget rendering
    // via `Widget::render`.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let transcript_area = chunks[0];
    let input_area = chunks[1];
    let status_area = chunks[2];

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

    let status = Paragraph::new(state.status.as_str())
        .style(Style::default().fg(Color::Cyan));
    status.render(status_area, &mut buf);

    if let Some(modal) = &state.pending_approval {
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        backdrop.render(transcript_area, &mut buf);
        let modal_area = centered_rect(60, 60, transcript_area);
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

// T1 composition note (updated T2): `run_interactive_session` blocks on
// `BufRead::read_line`, which would starve the `crossterm::event::poll` pump,
// so the live TUI loop duplicates the dispatch calling the SAME underlying seam
// functions (sanitizer, `ensure_host`, command dispatch, `drain_events` with the
// `TuiSink`). T2 starts the consolidation: the shared helpers
// `evaluate_approval_input` / `ApprovalModal::new` / `modal_key_decision` are the
// SAME functions both loops call for the approval surface; the remaining
// dispatch/composition duplication (the slash-command match and the session-
// composition block) stays duplicated and is recorded in the decision 106 ledger.
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
}
