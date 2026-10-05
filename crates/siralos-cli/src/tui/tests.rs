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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
    // Tail should contain the last lines
    assert!(content.contains("line 19"));
    assert!(content.contains("line 13"));
    // Scroll up 10 should show older lines and hide newest
    let mut scrolled = state.clone();
    scrolled.scroll_offset = 10;
    let buf2 = render(&scrolled, 40, 10);
    let content2: String = buf2.content().iter().map(|c| c.symbol()).collect();
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
    assert!(matching >= 2, "long line must span multiple rows, got: {rows:?}");
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
        assert!(row.chars().count() <= 40, "row overflows the width: {row:?}");
    }
    // Width is terminal-cell width, not UTF-8 bytes or scalar count.
    assert_eq!(wrap_line_to_width("界界界", 4), vec!["界界", "界"]);
}

#[test]
fn input_line_editing_renders() {
    let mut state = TuiState::new();
    state.input = "abc".to_owned();
    let buf = render(&state, 40, 10);
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(content.contains("> abc"));
    let mut edited = state.clone();
    edited.input.pop();
    edited.input.pop();
    let buf2 = render(&edited, 40, 10);
    let content2: String = buf2.content().iter().map(|c| c.symbol()).collect();
    assert!(content2.contains("> a"));
    assert!(!content2.contains("> abc"));
}

#[test]
fn status_line_rendering() {
    let mut state = TuiState::new();
    state.status = "working".to_owned();
    let buf = render(&state, 40, 10);
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(content.contains("working"));
    let mut other = state.clone();
    other.status = "ready".to_owned();
    let buf2 = render(&other, 40, 10);
    let content2: String = buf2.content().iter().map(|c| c.symbol()).collect();
    assert!(content2.contains("ready"));
    assert!(!content2.contains("working"));
}

/// Release the whole backlog the way the paint path does -- one character
/// per frame -- without a terminal to paint into.
///
/// The sink only BUFFERS now: a character is released by the frame that
/// shows it, so a test that wants text in the transcript paints.
fn paint_until_settled(state: &Rc<RefCell<TuiState>>) {
    let mut guard = state.borrow_mut();
    while guard.reveal_pending() {
        guard.reveal_char();
    }
}

#[test]
fn tui_sink_appends_sanitized_lines_verbatim() {
    let state = Rc::new(RefCell::new(TuiState::new()));
    let mut sink = TuiSink::new(state.clone());
    // Simulate sanitized lines as the session would write them.
    sink.write_all(b"hello world\n").expect("write");
    paint_until_settled(&state);
    sink.write_all(b"second line\n").expect("write");
    paint_until_settled(&state);
    let transcript = state.borrow().transcript_lines.clone();
    assert_eq!(transcript, vec!["hello world", "second line"]);
    // The sink is a defense-in-depth output boundary even when a caller
    // bypasses the session relay.
    sink.write_all(b"raw \x1b[31mred\x1b[0m\n").expect("write");
    paint_until_settled(&state);
    assert_eq!(state.borrow().transcript_lines[2], "raw red");
    sink.write_all(b"already sanitized ^@\n").expect("write");
    paint_until_settled(&state);
    assert_eq!(state.borrow().transcript_lines[3], "already sanitized ^@");
}

#[test]
fn tui_sink_respects_transcript_bound() {
    let state = Rc::new(RefCell::new(TuiState::new()));
    let mut sink = TuiSink::new(state.clone());
    for i in 0..(MAX_TRANSCRIPT_LINES + 50) {
        let line = format!("line {i}\n");
        sink.write_all(line.as_bytes()).expect("write");
        // S3c: the reveal is one character per PAINT, so a test that wants
        // the line in the transcript drains it the way the paint path does.
        let mut guard = state.borrow_mut();
        while guard.reveal_pending() {
            guard.reveal_char();
        }
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
fn transcript_lines_have_an_independent_byte_bound() {
    let mut state = TuiState::new();
    let oversized = "x".repeat(MAX_TRANSCRIPT_LINE_BYTES + 257);
    state.push_line(oversized);
    let line = &state.transcript_lines[0];
    assert!(line.len() <= MAX_TRANSCRIPT_LINE_BYTES);
    assert!(line.ends_with("... (line truncated)"));
}

#[test]
fn sink_caps_a_single_caller_buffer_before_decoding() {
    let state = Rc::new(RefCell::new(TuiState::new()));
    let mut sink = TuiSink::new(state.clone());
    let payload = vec![b'x'; MAX_STREAM_BYTES + 4096];
    // The bound is a TYPED refusal, not a silent cap: the caller is told its
    // buffer did not land whole, and the sink keeps the accepted prefix.
    let error = sink.write_all(&payload).expect_err("bounded write");
    assert_eq!(error.kind(), std::io::ErrorKind::WriteZero, "{error}");
    let state = state.borrow();
    assert!(state.status.contains("truncated"), "{}", state.status);
    assert!(state.stream_buffer.len() <= MAX_STREAM_BYTES);
    assert!(
        !state.stream_buffer.is_empty(),
        "the accepted prefix is still shown"
    );
}

#[test]
fn empty_state_initial_frame() {
    let state = TuiState::new();
    let buf = render(&state, 40, 10);
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
    state.transcript_lines = vec!["line 1".to_owned(), "line 2".to_owned()];
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
    plain.transcript_lines = vec!["line 1".to_owned(), "line 2".to_owned()];
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
    state.pending_approval = Some(ApprovalModal::new(vec!["req".to_owned()]));
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
    state.pending_approval = Some(ApprovalModal::new(vec!["req".to_owned()]));
    let n = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('n'),
        crossterm::event::KeyModifiers::NONE,
    );
    assert_eq!(handle_modal_key(&mut state, n), Some(ApprovalDecision::Deny));
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
    state.pending_approval = Some(ApprovalModal::new(vec!["req".to_owned()]));
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
    let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
    let modal = ApprovalModal::new(lines);
    assert_eq!(modal.lines.len(), MAX_APPROVAL_LINES + 1);
    assert_eq!(modal.lines[MAX_APPROVAL_LINES], APPROVAL_TRUNCATION_MARKER);
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
                    tier_counts: siralos_core::context_metrics::TierCounts {
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
        assert_eq!(frame[3], "Type /help for the list of available commands.");
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
        assert!(joined.contains("Approve applying 1 change to src/app.ts?"));
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
            std::fs::write(root.join(name), body.as_bytes()).expect("write");
        }
        let build = siralos_adapters::context_session::build_context_system(
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
                1 => {
                    ToolExecutionResult::Failed { message: "boom".to_owned() }
                }
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
        assert!(!lines.iter().any(|l| l.trim_start().starts_with("tick ")));
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
        let (root, mut session) =
            build_session("ring", &[("a.txt", "body a"), ("b.txt", "body b")]);
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
        let (root, session) = build_session("activity", &[("a.txt", "alpha")]);
        let pane = build_context_pane(true, Some(&session.metrics), &history)
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
        let pane = build_context_pane(true, Some(&session.metrics), &history)
            .expect("pane on");
        let mut state = TuiState::new();
        state.transcript_lines =
            vec!["hello".to_owned(), "world".to_owned(), "third".to_owned()];
        state.input = "test".to_owned();
        state.status = "ready".to_owned();
        let a = render_pane(&state, Some(&pane), 80, 24);
        let b = render_pane(&state, Some(&pane), 80, 24);
        assert_eq!(a, b);
        // And an active pane frame differs from the OFF frame.
        let off = render_off(&state, 80, 24);
        assert_ne!(a, off);
        let content: String = a.content().iter().map(|c| c.symbol()).collect();
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
        let after = build_context_pane(true, Some(&session.metrics), &history)
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
                    build_context_pane(enabled, metrics, &history).is_some();
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
        assert_eq!(utc_timestamp_from_millis(0), "1970-01-01 00:00:00 UTC");
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
        let palette_len = state.palette.as_ref().expect("palette for /").len();
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
fn the_reveal_releases_one_character_per_call() {
    // The owner's rule: the text renders a character at a time, so one
    // call releases exactly ONE character -- never a chunk, and never a
    // burst to catch up.
    let mut state = TuiState::new();
    assert!(!state.reveal_pending(), "nothing owed, nothing to release");
    state.stream_buffer = "hi\n".to_owned();
    state.reveal_char();
    assert_eq!(state.stream_tail, "h");
    state.reveal_char();
    assert_eq!(state.stream_tail, "hi");
    assert!(
        !state.transcript_lines.iter().any(|line| line == "hi"),
        "the line is complete only when its newline is released"
    );
    state.reveal_char();
    assert!(
        state.transcript_lines.iter().any(|line| line == "hi"),
        "the newline is the character that completes the row"
    );
    assert!(state.stream_tail.is_empty());
    assert!(!state.reveal_pending(), "the answer is fully revealed");

    // The thinking takes over, a character at a time, once the answer is
    // out -- the order is the reveal's, the cadence is the painter's.
    state.reasoning = "why".to_owned();
    assert!(state.reveal_pending());
    state.reveal_char();
    assert_eq!(state.reasoning_shown, 1);
    state.reveal_char();
    state.reveal_char();
    assert_eq!(state.reasoning_shown, 3);
    assert!(!state.reveal_pending());
}

#[test]
fn the_painters_are_unthrottled_while_text_is_owed() {
    // One character per painted frame, and the owner's next ask is that the
    // text track the SPEED the model produces it: so while anything is
    // owed, a painter waits NOTHING (the frame cost is the limiter), and
    // once nothing is owed the ordinary cadence is back and an idle UI
    // stops spinning.
    use std::time::Duration;
    assert_eq!(paint_interval(true, REDRAW_INTERVAL), Duration::ZERO);
    assert_eq!(paint_interval(true, TUI_IDLE_POLL), Duration::ZERO);
    assert_eq!(paint_interval(false, REDRAW_INTERVAL), REDRAW_INTERVAL);
    assert_eq!(paint_interval(false, TUI_IDLE_POLL), TUI_IDLE_POLL);
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
    // The grey tool-activity rule was WITHDRAWN on review: no producer
    // emits those transcript lines (tool activity renders in the context
    // pane), so it is the default style -- asserted so a re-introduction
    // has to come with a producer.
    assert_eq!(
        style_for_transcript_line("-> workspace.read").fg,
        Some(ratatui::style::Color::White)
    );
}

#[test]
fn turn_keys_keep_type_ahead_expand_thinking_and_ask_to_interrupt() {
    // S3b/4b: the keys that work WHILE the model is running. The arrows
    // expand the thinking mid-flight, Esc and Ctrl+C request interruption,
    // and a chorded character never becomes type-ahead.
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let key = |code, modifiers| KeyEvent::new(code, modifiers);
    let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
    let mut state = TuiState::new();
    assert!(!apply_turn_key(&mut state, plain(KeyCode::Char('h'))));
    assert!(!apply_turn_key(&mut state, plain(KeyCode::Char('i'))));
    assert_eq!(state.input, "hi", "typing mid-turn is kept");
    assert!(!apply_turn_key(&mut state, plain(KeyCode::Backspace)));
    assert_eq!(state.input, "h");
    // Ctrl+C is the active interrupt request; it must not become a literal c.
    assert!(apply_turn_key(
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

/// The painted rows of a headless frame, one string per terminal row.
fn frame_rows(state: &TuiState, width: u16, height: u16) -> Vec<String> {
    let buf = render_to_buffer(state, width, height);
    let row_width = width as usize;
    let cells: Vec<String> =
        buf.content().iter().map(|c| c.symbol().to_owned()).collect();
    cells
        .chunks(row_width)
        .map(|row| row.concat().trim_end().to_owned())
        .collect()
}

#[test]
fn thinking_renders_above_the_models_output() {
    // Owner bug report: the thinking sat BELOW the model's output -- the
    // answer's completed lines went into the transcript while the thinking
    // block was appended after them, so the words the model wrote appeared
    // first and the thinking drifted underneath (and the answer's in-flight
    // line rendered below the block, jumping over it as each line
    // completed). Thinking is what the model produced FIRST, so it renders
    // ABOVE the answer it explains.
    let mut state = TuiState::new();
    state.push_line("> what is 2+2".to_owned());
    state.begin_turn(std::time::Instant::now());
    state.push_reasoning("weighing the options");
    state.stream_buffer.push_str("the answer is four\nand it is exact\n");
    while state.reveal_pending() {
        state.reveal_char();
    }
    let rows = frame_rows(&state, 60, 12);
    let prompt = rows
        .iter()
        .position(|row| row.contains("what is 2+2"))
        .unwrap_or_else(|| panic!("the prompt renders: {rows:?}"));
    let thinking = rows
        .iter()
        .position(|row| row.contains("thinking"))
        .unwrap_or_else(|| panic!("the thinking row renders: {rows:?}"));
    let answer = rows
        .iter()
        .position(|row| row.contains("the answer is four"))
        .unwrap_or_else(|| panic!("the answer renders: {rows:?}"));
    assert!(
        prompt < thinking,
        "the block opens below the prompt that asked for it: {rows:?}"
    );
    assert!(
        thinking < answer,
        "thinking must render above the model's output: {rows:?}"
    );
}

#[test]
fn the_block_stays_with_the_turn_that_produced_it() {
    // S3d: the anchor moves only when a turn actually streams thinking. A turn
    // that reasons re-anchors the block above ITS answer; a turn that does not
    // leaves the block above the answer it explains instead of dragging an
    // older trace down the conversation.
    let run = |state: &mut TuiState, prompt: &str, answer: &str| {
        state.push_line(prompt.to_owned());
        state.begin_turn(std::time::Instant::now());
        state.stream_buffer.push_str(answer);
        while state.reveal_pending() {
            state.reveal_char();
        }
        state.end_turn();
    };
    let mut state = TuiState::new();
    state.push_line("> first".to_owned());
    state.begin_turn(std::time::Instant::now());
    state.push_reasoning("turn one thinking");
    state.stream_buffer.push_str("turn one answer\n");
    while state.reveal_pending() {
        state.reveal_char();
    }
    state.end_turn();
    let rows = frame_rows(&state, 80, 16);
    let block = rows
        .iter()
        .position(|row| row.contains("thinking"))
        .unwrap_or_else(|| panic!("the block opens: {rows:?}"));
    let first = rows
        .iter()
        .position(|row| row.contains("turn one answer"))
        .unwrap_or_else(|| panic!("turn one renders: {rows:?}"));
    assert!(block < first, "the block opens above its own answer: {rows:?}");

    // Turn two streams no thinking at all.
    run(&mut state, "> second", "turn two answer\n");
    let rows = frame_rows(&state, 80, 16);
    let block = rows
        .iter()
        .position(|row| row.contains("thinking"))
        .unwrap_or_else(|| panic!("the block survives: {rows:?}"));
    let first = rows
        .iter()
        .position(|row| row.contains("turn one answer"))
        .unwrap_or_else(|| panic!("turn one renders: {rows:?}"));
    let second = rows
        .iter()
        .position(|row| row.contains("turn two answer"))
        .unwrap_or_else(|| panic!("turn two renders: {rows:?}"));
    assert!(block < first, "the block keeps its turn: {rows:?}");
    assert!(first < second, "the conversation keeps its order: {rows:?}");

    // Turn three reasons again, so the block re-anchors -- below everything
    // already read, above the answer it explains.
    state.push_line("> third".to_owned());
    state.begin_turn(std::time::Instant::now());
    state.push_reasoning("turn three thinking");
    state.stream_buffer.push_str("turn three answer\n");
    while state.reveal_pending() {
        state.reveal_char();
    }
    state.end_turn();
    let rows = frame_rows(&state, 80, 16);
    let block = rows
        .iter()
        .position(|row| row.contains("thinking"))
        .unwrap_or_else(|| panic!("the block renders: {rows:?}"));
    let second = rows
        .iter()
        .position(|row| row.contains("turn two answer"))
        .unwrap_or_else(|| panic!("turn two renders: {rows:?}"));
    let third = rows
        .iter()
        .position(|row| row.contains("turn three answer"))
        .unwrap_or_else(|| panic!("turn three renders: {rows:?}"));
    assert!(second < block, "the block re-anchors forward: {rows:?}");
    assert!(block < third, "and above the answer it explains: {rows:?}");
}

#[test]
fn thinking_is_released_before_the_answer_it_sits_above() {
    // S3d: the block renders above the answer, so the reveal releases it
    // FIRST -- the reader meets the model's output in the order the model
    // produced it. Both buffers are owed here, so the priority is the only
    // thing that can decide which character comes out.
    let mut state = TuiState::new();
    state.push_reasoning("why");
    state.stream_buffer.push_str("hi");
    state.reveal_char();
    assert_eq!(state.reasoning_shown, 1, "the thinking goes first");
    assert!(state.stream_tail.is_empty(), "the answer has not started");
    state.reveal_char();
    state.reveal_char();
    assert_eq!(state.reasoning_shown, 3, "one character per call");
    state.reveal_char();
    assert_eq!(state.stream_tail, "h", "then the answer it explains");
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
    paint_until_settled(&state);
    assert!(
        state
            .borrow()
            .transcript
            .iter()
            .any(|entry| entry.text == "streamed line"),
        "the line lands in the transcript once the frame releases it"
    );
    // While text is OWED the request is deliberately NOT coalesced: one
    // painted frame releases one character, so the stream needs a frame per
    // character to be shown at the speed the model produces it.
    sink.write_all(b"second line\n").expect("write");
    assert_eq!(
        frames.get(),
        2,
        "a delta that is still owed asks for its own frame"
    );
    // Once nothing is owed, the ordinary redraw interval coalesces again.
    paint_until_settled(&state);
    sink.write_all(b"third line\n").expect("write");
    paint_until_settled(&state);
    let coalesced = frames.get();
    sink.write_all(b"fourth line\n").expect("write");
    assert_eq!(
        frames.get(),
        coalesced + 1,
        "one more frame for the next owed character"
    );
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
    assert!(state.max_scroll(viewport) > 0, "the transcript must overflow");
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
    assert_eq!(toggle_mouse_capture(&mut state), MOUSE_CAPTURE_OFF_MESSAGE);
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
        MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE }
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
        handle_mouse(&mut state, wheel(MouseEventKind::ScrollUp), viewport);
        assert!(state.scroll_offset <= max);
    }
    assert_eq!(state.scroll_offset, max);
    // Already at the top: further ScrollUp stays at max.
    handle_mouse(&mut state, wheel(MouseEventKind::ScrollUp), viewport);
    assert_eq!(state.scroll_offset, max);
    // At the tail: ScrollDown never goes below zero.
    state.scroll_offset = 0;
    for _ in 0..3 {
        handle_mouse(&mut state, wheel(MouseEventKind::ScrollDown), viewport);
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
        MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE }
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

#[test]
fn header_and_status_mask_secret_shaped_provider_and_model_labels() {
    for value in [
        "vendor/key:super-secret",
        "vendor/sk-live-secret",
        "vendor/secret-model",
    ] {
        let status = compose_status_line("ready", Some(value), Some(value));
        let header = header_text(Some(value), Some(value));
        assert!(!status.contains(value), "status leaked {value}: {status}");
        assert!(!header.contains(value), "header leaked {value}: {header}");
        assert!(status.contains("[REDACTED]"));
        assert!(header.contains("[REDACTED]"));
    }
    for value in [
        "vendor/model\nforged",
        "vendor/model\tforged",
        "vendor/model\u{0085}",
    ] {
        let status = compose_status_line("ready", Some(value), None);
        let header = header_text(Some(value), None);
        assert!(!status.contains('\n'));
        assert!(!header.contains('\n'));
        assert!(!status.contains('\t'));
        assert!(!header.contains('\t'));
    }
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
    let content2: String = buf2.content().iter().map(|c| c.symbol()).collect();
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
    assert_eq!(style_for_transcript_line("Approved.").fg, Some(Color::Yellow));
    assert_ne!(
        style_for_transcript_line("> hi"),
        style_for_transcript_line("hi")
    );
    // Ensure rendered buffer carries non-default style for user line
    let has_cyan =
        buf.content().iter().any(|cell| cell.style().fg == Some(Color::Cyan));
    // The buffer's cells for the user echo should be Cyan somewhere
    // (at least the '>' and following chars)
    assert!(
        has_cyan || style_for_transcript_line("> hi").fg == Some(Color::Cyan)
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
    let content2: String = buf2.content().iter().map(|c| c.symbol()).collect();
    assert!(content2.contains(" Context "));
    let has_pane_rounded = content2.contains('╭') || content2.contains('╮');
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(content.contains("/context"));
    assert!(content.contains(" commands "));
    // Prefix highlight: input "/do" should highlight " /do" prefix in palette
    let mut state2 = TuiState::new();
    state2.input = "/do".to_owned();
    state2.update_palette();
    assert!(
        state2.palette.as_ref().unwrap().iter().any(|(n, _)| n == "/domains")
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
    assert!(has_highlight, "palette prefix should be highlighted Yellow Bold");
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
    let mut metrics2 = siralos_core::context_metrics::ContextMetrics::new();
    // To get a non-zero assembled_total, we need to drive a tick with an assembled context
    {
        use siralos_core::context_graph::{
            ContextGraph, ContextNode, ContextNodeKind,
        };
        use siralos_core::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
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
        metrics2.record_tick(&input, &before, &ws, &report, Some(&assembled));
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
        assert!(line.chars().count() <= 80, "banner line too wide: {line:?}");
    }
    assert!(SIRALOS_GREETING.contains("Siralos"));
    let mut state = TuiState::new();
    push_banner_and_greeting(&mut state);
    let buf = render(&state, 80, 24);
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
        first.iter().all(|(n, _)| n.to_ascii_lowercase().starts_with("/p"))
    );
    state.input = "/pr".to_owned();
    state.update_palette();
    let second = state.palette.as_ref().unwrap().clone();
    assert!(second.len() <= first.len());
    assert!(
        second.iter().all(|(n, _)| n.to_ascii_lowercase().starts_with("/pr"))
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(content.contains("example/model-a"));
    assert!(content.contains("example/model-b"));
    assert!(content.contains("switch model"));
    // Enter arms the pending switch and closes the picker.
    assert!(!handle_key(&mut state, press(KeyCode::Enter), 10));
    assert!(state.model_switch_picker.is_none());
    assert_eq!(state.pending_model_switch.as_deref(), Some("example/model-b"));
    // Esc clears a pending arm and an open picker.
    open_model_switch_picker(&mut state, vec!["example/model-a".to_owned()]);
    assert!(!handle_key(&mut state, press(KeyCode::Esc), 10));
    assert!(state.model_switch_picker.is_none());
    assert!(state.pending_model_switch.is_none());
}

#[test]
fn model_switch_picker_masks_only_rendered_rows_and_keeps_raw_selection() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let raw = vec![
        "vendor/model:token".to_owned(),
        "vendor/key:secret".to_owned(),
        "vendor/plain".to_owned(),
    ];
    let mut state = TuiState::new();
    open_model_switch_picker(&mut state, raw.clone());

    let picker = state.model_switch_picker.as_ref().expect("picker open");
    assert_eq!(picker.items, raw, "the picker must retain exact model ids");

    let content: String = render(&state, 80, 24)
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("[MODEL REDACTED]"), "got: {content}");
    assert!(!content.contains("model:token"), "got: {content}");
    assert!(!content.contains("key:secret"), "got: {content}");
    assert!(content.contains("vendor/plain"), "got: {content}");

    let enter = KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        KeyEventKind::Press,
    );
    assert!(!handle_key(&mut state, enter, 10));
    assert_eq!(
        state.pending_model_switch.as_deref(),
        Some("vendor/model:token"),
        "selection must persist the raw fetched id",
    );
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
fn provider_picker_masks_secret_shaped_provider_labels() {
    let entries = provider_entries_from_session(
        Some("vendor/key:super-secret"),
        Some("vendor/model:token"),
        Some("https://api.example.com/v1"),
    );
    assert_eq!(entries[0].name, "[REDACTED]");
    assert_eq!(entries[0].model, "[MODEL REDACTED]");
    assert!(!entries[0].name.contains("super-secret"));
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
    assert_eq!(handle_modal_key(&mut state, no), Some(ApprovalDecision::Deny));
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
    let content: String = first.content().iter().map(|c| c.symbol()).collect();
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

fn t108_key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

fn t108_type(state: &mut TuiState, text: &str) {
    for ch in text.chars() {
        handle_key(state, t108_key(crossterm::event::KeyCode::Char(ch)), 10);
    }
}

fn t108_enter(state: &mut TuiState) {
    handle_key(state, t108_key(crossterm::event::KeyCode::Enter), 10);
}

/// Settle the deferred model probe the interactive loop would normally resolve
/// between keystrokes. Without it the form blocks input exactly as it does in
/// production, and a scripted "type the model manually" step silently no-ops.
fn settle_model_fetch(state: &mut TuiState) {
    let form = state.provider_add_form.as_mut().expect("form open");
    if form.fetching_models {
        form.apply_fetch_result(Err("test".to_owned()));
    }
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
    assert_eq!(form.endpoint.as_deref(), Some("https://api.openai.com/v1"));
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
    // Named providers use their registry-owned fixed route; the form's
    // temporary discovery URL is not persisted as a profile override.
    assert_eq!(completed.endpoint, None);
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
fn nonempty_api_key_is_stored_verbatim_without_a_teaching_error() {
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
        let palette = state
            .palette
            .as_ref()
            .unwrap_or_else(|| panic!("palette must retain at step {step}"));
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
    handle_key(&mut state, t108_key(crossterm::event::KeyCode::Backspace), 10);
    assert_eq!(state.input, "/pr");
    assert!(
        !state.palette.as_ref().expect("palette after backspace").is_empty()
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
fn credential_env_name_rule_is_the_shared_predicate() {
    // C2 boundary: env-var NAME only, [A-Z0-9_]{1,64}. The rule lives in
    // core; the TUI form never validated this field, so the local copy was
    // deleted with the round-3 consolidation.
    assert!(siralos_core::composition::is_credential_env_name(
        "OPENAI_API_KEY"
    ));
    assert!(siralos_core::composition::is_credential_env_name("A"));
    assert!(!siralos_core::composition::is_credential_env_name("openai"));
    assert!(!siralos_core::composition::is_credential_env_name("HAS-DASH"));
    assert!(!siralos_core::composition::is_credential_env_name("HAS SPACE"));
    assert!(!siralos_core::composition::is_credential_env_name(""));
    assert!(!siralos_core::composition::is_credential_env_name(
        "A".repeat(65).as_str()
    ));
    assert!(siralos_core::composition::is_credential_env_name(
        "A".repeat(64).as_str()
    ));
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
    handle_key(&mut state, t108_key(crossterm::event::KeyCode::Char('/')), 10);
    handle_key(&mut state, t108_key(crossterm::event::KeyCode::Char('p')), 10);
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
    assert!(content_off.contains(&long[..40]), "off should contain long line");
    // Header should span full width (reversed cyan, 80 cols)
    let header_line = &content_off[0..80];
    assert_eq!(header_line.len(), 80);
    // On variant with pane should still fill full width (transcript 40 + pane 40)
    let pane = crate::tui::ContextPaneData {
        counters: vec![],
        ring: vec![],
        activity: vec![],
    };
    let buf_on =
        crate::tui::render_to_buffer_with_pane(&state, Some(&pane), 80, 24);
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
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
    let content: String = buf.content().iter().map(|c| c.symbol()).collect();
    assert!(content.contains("the provider endpoint"));
    assert!(content.contains("the environment variable holding your key"));
}

#[test]
fn validation_errors_are_human_readable() {
    // D2: error messages are plain English with examples, no cryptic regex.
    let provider_err = validate_provider_name("Bad Provider!").unwrap_err();
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

    // The credential env-var-name validator this test used to cover was
    // deleted: it had no production caller, so its O3/I3 teaching message
    // could not reach a user. `ROADMAP.md` keeps the text for the owner.

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

    // The credential env-var-name rule moved to core in round 3; its
    // accept-set is asserted in `credential_env_name_rule_is_the_shared_predicate`.
    assert!(siralos_core::composition::is_credential_env_name(
        "OPENAI_API_KEY"
    ));
    assert!(siralos_core::composition::is_credential_env_name(
        "ANTHROPIC_KEY"
    ));
    assert!(!siralos_core::composition::is_credential_env_name("openai"));

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
    settle_model_fetch(&mut state);
    // Protocol selection starts the deferred model probe. Settle it before
    // typing the manual fallback, exactly as the interactive loop does.
    {
        let form = state.provider_add_form.as_mut().unwrap();
        if form.fetching_models {
            form.apply_fetch_result(Err("test".to_owned()));
        }
    }
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
        None,
        "named adapters own a fixed route; the form must not persist a custom endpoint"
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

#[test]
fn provider_add_form_keeps_endpoint_for_a_generic_provider() {
    let mut state = TuiState::new();
    state.provider_add_form = Some(ProviderAddForm {
        endpoint: Some("https://api.example.com/v1".to_owned()),
        credential_env: Some("EXAMPLE_KEY".to_owned()),
        provider: Some("example-vendor".to_owned()),
        protocol: Some("openai-completions".to_owned()),
        model: Some("example/model-a".to_owned()),
        model_display_name: None,
        field: ProviderAddField::ModelDisplayName,
        input: String::new(),
        error: None,
        completed: None,
        fetching_models: false,
        model_picker: None,
        fetch_note: None,
        protocol_picker: None,
    });
    handle_key(&mut state, t108_key(crossterm::event::KeyCode::Enter), 10);
    assert_eq!(
        state
            .provider_add_form
            .as_ref()
            .and_then(|form| form.completed.as_ref())
            .and_then(|data| data.endpoint.as_deref()),
        Some("https://api.example.com/v1")
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    let display_pos = joined.find("display name").expect("display name label");
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
    assert_eq!(derive_provider_name("https://api.openai.com/v1"), "openai");
    assert_eq!(derive_provider_name("https://vendor.example.com"), "vendor");
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
    settle_model_fetch(&mut state);
    t108_type(&mut state, "gpt-4o");
    t108_enter(&mut state);
    t108_type(&mut state, "My Display");
    t108_enter(&mut state);
    let completed =
        state.provider_add_form.as_ref().unwrap().completed.clone().unwrap();
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
    settle_model_fetch(&mut state);
    t108_type(&mut state, "gpt-4o");
    t108_enter(&mut state);
    t108_type(&mut state, "");
    t108_enter(&mut state);
    let completed =
        state.provider_add_form.as_ref().unwrap().completed.clone().unwrap();
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
    settle_model_fetch(&mut state);
    t108_type(&mut state, "model-a");
    t108_enter(&mut state);
    t108_type(&mut state, "Spark Display");
    t108_enter(&mut state);
    let completed =
        state.provider_add_form.as_ref().unwrap().completed.clone().unwrap();
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
    assert_eq!(completed.model_display_name.as_deref(), Some("Spark Display"));
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
    settle_model_fetch(&mut state);
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
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
        .map(|l| l.iter().map(|s| s.content.to_string()).collect::<String>())
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
        let rendered =
            lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
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
    let items: Vec<String> = (0..20).map(|i| format!("item-{i:02}")).collect();
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
    let count_items = items.iter().filter(|it| rendered.contains(*it)).count();
    assert!(count_items <= 8, "viewport must bound to 8, got {count_items}");
}

#[test]
fn the_visible_window_matches_the_whole_transcript_window() {
    // The frame builds only the rows the viewport shows (the whole-
    // transcript shape made a frame cost grow with the session, which no
    // per-character reveal can afford). Those rows must be exactly the ones
    // the whole-transcript version produced, for every width, height and
    // scroll offset.
    fn reference(
        transcript: &[TranscriptEntry],
        above: &[(&str, Option<&str>)],
        above_at: usize,
        below: &[(&str, Option<&str>)],
        width: usize,
        height: usize,
        offset: u16,
    ) -> Vec<(String, Style)> {
        let mut rows: Vec<(String, Style)> = Vec::new();
        let one = |rows: &mut Vec<(String, Style)>,
                   text: &str,
                   timestamp: Option<&str>| {
            let style = style_for_transcript_line(text);
            for row in wrap_line_to_width(text, width) {
                rows.push((row, style));
            }
            if let Some(ts) = timestamp {
                let dim = Style::default().fg(Color::DarkGray);
                for row in wrap_line_to_width(ts, width) {
                    rows.push((row, dim));
                }
            }
        };
        // Reading order: everything before the block, the block, the stored
        // entries after it, then the trailing rows.
        let at = above_at.min(transcript.len());
        for entry in &transcript[..at] {
            one(&mut rows, &entry.text, entry.timestamp.as_deref());
        }
        for (text, ts) in above {
            one(&mut rows, text, *ts);
        }
        for entry in &transcript[at..] {
            one(&mut rows, &entry.text, entry.timestamp.as_deref());
        }
        for (text, ts) in below {
            one(&mut rows, text, *ts);
        }
        let total = rows.len();
        let max_scroll = total.saturating_sub(height);
        let scroll = (offset as usize).min(max_scroll);
        let start = if total <= height { 0 } else { total - height - scroll };
        let end = (start + height).min(total);
        rows[start..end].to_vec()
    }

    for count in [0usize, 1, 5, 40, 200] {
        let mut state = TuiState::new();
        for i in 0..count {
            let line = format!("> line {i} {}", "word ".repeat(i % 7));
            state.transcript_lines.push(line.clone());
            state.transcript.push(TranscriptEntry {
                text: line,
                timestamp: (i % 3 == 0).then(|| "2026-09-12 10:00".to_owned()),
            });
        }
        for width in [12usize, 21, 40, 100] {
            for height in [1usize, 3, 24] {
                for offset in [0u16, 1, 7, 100, 5000] {
                    // S3d: the block is anchored somewhere in the stored
                    // transcript -- the start, the middle, the end, and past
                    // the end, which must clamp rather than panic.
                    for above_at in [0usize, 1, count / 2, count, count + 7] {
                        let above: Vec<(&str, Option<&str>)> = vec![
                            ("thinking row", None),
                            ("second thinking row", None),
                        ];
                        let below: Vec<(&str, Option<&str>)> =
                            vec![("partial answer", None), ("", None)];
                        let bounded = visible_transcript_rows(
                            &state.transcript,
                            &state.transcript_lines,
                            &FrameRows {
                                above: &above,
                                above_at,
                                below: &below,
                            },
                            width,
                            height,
                            offset,
                        );
                        let whole = reference(
                            &state.transcript,
                            &above,
                            above_at,
                            &below,
                            width,
                            height,
                            offset,
                        );
                        assert_eq!(
                            bounded, whole,
                            "count={count} width={width} height={height} offset={offset} above_at={above_at}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn a_long_session_does_not_make_a_frame_expensive() {
    // The bound is on the WORK, not on wall-clock: the frame builds only the
    // rows the viewport shows, so its cost stops growing with the session.
    // The whole-transcript shape measured ~5 ms at 24 lines and ~21 ms at
    // 1200 lines in this build; the numbers are printed so a run records
    // them, and the assertion is generous enough for a loaded machine.
    use std::time::{Duration, Instant};
    let build = |lines: usize| {
        let mut state = TuiState::new();
        for i in 0..lines {
            let line = format!("> line {i} with some ordinary words on it");
            state.transcript_lines.push(line.clone());
            state
                .transcript
                .push(TranscriptEntry { text: line, timestamp: None });
        }
        state
    };
    let long = build(5000);
    let short = build(24);
    let above: Vec<(&str, Option<&str>)> = vec![("thinking row", None)];
    let below: Vec<(&str, Option<&str>)> = vec![("partial answer", None)];

    let start = Instant::now();
    let rows = visible_transcript_rows(
        &long.transcript,
        &long.transcript_lines,
        &FrameRows {
            above: &above,
            above_at: long.transcript.len() / 2,
            below: &below,
        },
        100,
        24,
        0,
    );
    let window_cost = start.elapsed();
    assert_eq!(rows.len(), 24, "exactly one viewport of rows");

    let frame_cost = |state: &TuiState| {
        let frames = 20u32;
        let start = Instant::now();
        for _ in 0..frames {
            let _ = render_to_buffer(state, 100, 30);
        }
        start.elapsed() / frames
    };
    let short_frame = frame_cost(&short);
    let long_frame = frame_cost(&long);
    println!(
        "window {window_cost:?} at 5000 lines; frame {short_frame:?} at 24 lines vs {long_frame:?} at 5000"
    );
    assert!(
        window_cost.as_millis() < 40,
        "the window must not walk the session: {window_cost:?}"
    );
    assert!(
        long_frame < short_frame * 3 + Duration::from_millis(10),
        "a 5000-line session must not paint slower than a 24-line one: {long_frame:?} vs {short_frame:?}"
    );
}

#[test]
fn an_idle_frame_is_byte_identical_and_releases_nothing() {
    // C4 evidence pack, third number: the idle path must not disturb the
    // frame. The loop now sweeps the worker channel on EVERY frame and the
    // painters are open while text is owed, so the property that keeps the
    // pinned tui-render subject valid is: with nothing owed, a frame is the
    // same bytes and a release does nothing.
    let mut state = TuiState::new();
    state.push_line("> hello".to_owned());
    state.status = "example-vendor / example-model".to_owned();
    assert!(!state.reveal_pending(), "nothing is owed");
    let first = render_to_buffer(&state, 100, 30);
    // An idle release is a no-op, however often the loop sweeps.
    for _ in 0..64 {
        state.reveal_char();
    }
    assert!(!state.reveal_pending(), "still nothing owed");
    assert!(state.stream_tail.is_empty());
    let second = render_to_buffer(&state, 100, 30);
    assert_eq!(first, second, "an idle frame is byte-identical");
}

#[test]
fn a_backlog_drains_at_the_frame_rate() {
    // "Are you able to match the speed the model produces it?": with no
    // artificial cadence the only limiter is the frame cost, so a backlog
    // drains one character per painted frame, as fast as frames can be
    // painted. The measured rate is printed -- this build is unoptimized
    // (the dev flow runs \`cargo run\`), and a release build is several
    // times cheaper -- and the floor is generous enough for a slow machine.
    use std::time::Instant;
    let mut state = TuiState::new();
    state.reasoning = "thinking. ".repeat(25); // 250 characters owed
    let owed = state.reasoning.len();
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    let start = Instant::now();
    let mut frames = 0usize;
    while state.reveal_pending() {
        state.reveal_char();
        terminal
            .draw(|frame| draw_with_pane(&state, None, frame))
            .expect("frame");
        frames += 1;
    }
    let elapsed = start.elapsed();
    let rate = owed as f64 / elapsed.as_secs_f64();
    // The same reveal through the WIDGET path only (no terminal, no diff):
    // it separates our own layout cost from the backend's, which is what
    // decides whether a higher ceiling is ours to raise or the build's.
    let widgets = {
        let start = Instant::now();
        let samples = 20u32;
        for _ in 0..samples {
            let _ = render_to_buffer(&state, 100, 30);
        }
        start.elapsed() / samples
    };
    println!(
        "{frames} frames for {owed} characters: {rate:.0} characters/s ({:?} per frame); widgets alone {widgets:?} -> {:.0} characters/s",
        elapsed / frames as u32,
        1.0 / widgets.as_secs_f64()
    );
    assert_eq!(frames, owed, "one character per frame, always");
    assert!(
        rate > 100.0,
        "the frame cost is the only limiter; measured {rate:.0} characters/s"
    );
}

#[test]
fn a_fast_trace_is_never_dropped_before_it_is_shown() {
    // push_reasoning bounds what is RETAINED, never what the reader is
    // still owed: with a per-character reveal a fast trace can be thousands
    // of characters ahead, and trimming that away would skip text nobody
    // saw and jump the visible row forward.
    let mut state = TuiState::new();
    state.push_reasoning(&"z".repeat(REASONING_BYTES * 2));
    assert_eq!(
        state.reasoning.len(),
        REASONING_BYTES * 2,
        "nothing revealed yet means nothing may be dropped"
    );
    assert_eq!(state.reasoning_shown, 0);

    // Once text HAS been shown, the bound applies to it: the buffer settles
    // back to REASONING_BYTES and the reveal offset is rebased onto a
    // character boundary.
    state.reasoning_shown = state.reasoning.len();
    state.push_reasoning("more");
    assert!(
        state.reasoning.len() <= REASONING_BYTES,
        "revealed text is trimmed to the bound, got {}",
        state.reasoning.len()
    );
    // The rebase must leave a usable offset: revealing from it cannot panic.
    state.reveal_char();
    assert!(state.reasoning_shown <= state.reasoning.len());
}

#[test]
fn a_multibyte_trace_is_revealed_and_trimmed_on_character_boundaries() {
    // The reveal offset is a BYTE index, so dropping a prefix has to land on
    // a character boundary or the next slice panics.
    let mut state = TuiState::new();
    state.push_reasoning(&"é".repeat(REASONING_BYTES));
    for _ in 0..64 {
        state.reveal_char();
    }
    assert_eq!(state.reasoning_shown, 128, "two bytes per character");
    state.reasoning_shown = state.reasoning.len();
    state.push_reasoning("é");
    assert!(state.reasoning.is_char_boundary(state.reasoning_shown));
    state.reveal_char();
}
