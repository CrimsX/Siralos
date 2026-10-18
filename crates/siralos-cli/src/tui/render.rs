//! Frame drawing: the transcript frame rows, modal layout, the
//! provider-add form rows, and headless buffer rendering.

use super::{
    Block, BorderType, Borders, Buffer, CONTEXT_PANE_WIDTH, Color, Constraint,
    ContextPaneData, Direction, Frame, Layout, Line, Paragraph,
    ProviderAddField, ProviderAddForm, Rect, Span, Style, Text,
    TranscriptEntry, TuiState, Widget, compose_bottom_row, context_pane_lines,
    display_width, model_picker_lines, redacted_form_credential,
    safe_display_label, safe_form_endpoint_edit,
    safe_form_endpoint_for_credential, safe_form_message,
    safe_form_message_for_credential, safe_model_id, working_line,
    wrap_line_to_width,
};

/// The rows a frame paints that are NOT stored transcript, and where they
/// belong (S3d).
///
/// Named fields because the placement IS the meaning: `above` renders above
/// the stored entry at `above_at` (the thinking block, which belongs above the
/// answer it explains), and `below` trails the last stored entry (the answer's
/// growing line, the gap that separates the indicator). Empty slices
/// reproduce the plain stored transcript exactly, which is what keeps every
/// reasoning-free frame byte-identical.
pub(super) struct FrameRows<'a> {
    pub(super) above: &'a [(&'a str, Option<&'a str>)],
    pub(super) above_at: usize,
    pub(super) below: &'a [(&'a str, Option<&'a str>)],
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
pub(super) fn visible_transcript_rows(
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
        let safe_text = crate::sanitize::sanitize_for_display(text);
        let text_style = style_for_transcript_line(&safe_text);
        let mut rows: Vec<(String, Style)> =
            wrap_line_to_width(&safe_text, inner_width)
                .into_iter()
                .map(|row| (row, text_style))
                .collect();
        if let Some(ts) = timestamp {
            let dim = Style::default().fg(Color::DarkGray);
            let safe_timestamp = crate::sanitize::sanitize_for_display(ts);
            rows.extend(
                wrap_line_to_width(&safe_timestamp, inner_width)
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
    // Transcript/input/status layout (owner 2026-10-17: no top header bar;
    // provider/model moved into the bottom row).
    // Owner QoL: the `working` indicator owns a row ONLY while the model
    // works, so idle frames stay byte-identical to the pinned ones.
    // The turn timer IS the busy signal now that the bottom bar no longer
    // carries a `working` word.
    let busy_rows = u16::from(state.busy_since.is_some());
    let (transcript_area, busy_area, input_area, status_area, pane_area) =
        match pane {
            None => {
                // I5 OFF: transcript spans full width, no gap (Min(0) fill)
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(busy_rows),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ])
                    .split(area);
                (chunks[0], chunks[1], chunks[2], chunks[3], None)
            }
            Some(_) => {
                // I5 ON: transcript Min(0) + pane Length(40) fills width, no gap
                let outer = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(1)])
                    .split(area);
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(CONTEXT_PANE_WIDTH),
                    ])
                    .split(outer[0]);
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(busy_rows),
                        Constraint::Length(1),
                    ])
                    .split(cols[0]);
                (rows[0], rows[1], rows[2], outer[1], Some(cols[1]))
            }
        };
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
            // Available height for the popup (terminal height minus input/status rows).
            // Bounded popup: grows to fit, but never exceeds the rows above
            // the input bar (height -2: input + status).
            let available_height = (area.height.saturating_sub(2)) as usize;
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
                .min(area.height.saturating_sub(2))
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

    // Input line: `> <input>`. Sanitize at the final render boundary as
    // well as upstream; pasted control bytes must never reach a widget.
    let safe_input = crate::sanitize::sanitize_for_display(&state.input);
    let input_text = format!("> {safe_input}");
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
    // When a modal or any add-form is open, hide the cursor behind
    // the dimmed backdrop (no typing through a modal).
    if state
        .pending_approval
        .as_ref()
        .and_then(|approval| approval.modal())
        .is_none()
        && state.provider_add_form.is_none()
        && state.ui_spec_form.is_none()
    {
        let input_width =
            u16::try_from(display_width(&safe_input)).unwrap_or(u16::MAX);
        let cursor_x = input_area
            .x
            .saturating_add(2)
            .saturating_add(input_width)
            .min(input_area.x + input_area.width.saturating_sub(1));
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

    // Bottom row (owner 2026-10-17): status left, usage metrics middle,
    // provider/model flush right. Working status keeps its distinct yellow
    // bold word style (H5).
    {
        let status = safe_display_label(&state.status);
        let bottom = compose_bottom_row(
            status_area.width,
            &status,
            state.provider.as_deref(),
            state.model.as_deref(),
            &state.metrics,
        );
        frame.render_widget(Paragraph::new(bottom), status_area);
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

    // UI spec add/edit form (ADR 0039 P4): same modal discipline as
    // the provider form — rounded modal, current field highlighted,
    // per-field description, error line when the last submit was
    // invalid.
    if let Some(form) = &state.ui_spec_form {
        let backdrop = Block::default()
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));
        frame.render_widget(backdrop, backdrop_area);
        let modal_area = centered_rect(75, 75, backdrop_area);
        frame.render_widget(ratatui::widgets::Clear, modal_area);
        let title = match &form.editing {
            Some(id) => format!(
                " edit UI component {} ",
                crate::sanitize::sanitize_for_display(id)
            ),
            None => " add UI component ".to_owned(),
        };
        let modal_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black).fg(Color::Cyan));
        let inner = modal_block.inner(modal_area);
        frame.render_widget(modal_block, modal_area);
        let lines = ui_spec_form_lines(form);
        let paragraph = Paragraph::new(Text::from(lines))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    // Modal overlay (T2): dimmed backdrop + centered modal with the sanitized
    // approval request (bounded to MAX_APPROVAL_LINES). This reuses the same
    // sanitizer-bound lines the stdio path would render — no unsanitized content.
    if let Some(modal) =
        state.pending_approval.as_ref().and_then(|a| a.modal())
    {
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
        let text: Vec<Line<'_>> = modal
            .lines
            .iter()
            .map(|line| Line::from(safe_form_message(line)))
            .collect();
        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }
}

pub(super) fn provider_add_form_lines(
    form: &ProviderAddForm,
) -> Vec<Line<'static>> {
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
            } else if field == ProviderAddField::ApiKey {
                "> api key: key:***█".to_owned()
            } else if field == ProviderAddField::Url {
                format!(
                    "> {}: {}█",
                    label,
                    safe_form_endpoint_edit(
                        &form.input,
                        form.credential_env.as_deref(),
                    )
                )
            } else {
                format!(
                    "> {}: {}█",
                    label,
                    safe_form_message_for_credential(
                        &form.input,
                        form.credential_env.as_deref(),
                    )
                )
            }
        } else if let Some(val) = stored {
            if val.is_empty() {
                format!("  {label}: —")
            } else {
                let rendered = if field == ProviderAddField::ApiKey {
                    redacted_form_credential(Some(val)).unwrap_or_default()
                } else if field == ProviderAddField::Url {
                    safe_form_endpoint_for_credential(
                        val,
                        form.credential_env.as_deref(),
                    )
                } else {
                    safe_form_message_for_credential(
                        val,
                        form.credential_env.as_deref(),
                    )
                };
                format!("  {label}: {rendered}")
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
                        Line::from(format!(
                            "    {prefix}{}",
                            safe_model_id(item)
                        ))
                        .style(style),
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
                    Line::from(format!("    {}", safe_form_message(note)))
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
            Line::from(format!("    error: {}", safe_form_message(err)))
                .style(Style::default().fg(Color::Red)),
        );
    }
    lines
}

/// UI spec form lines: one label + value row per field in form
/// order (id row only while adding), the current field's
/// description, accumulated params, and the error line when the
/// last submit was invalid. Same modal discipline as the provider
/// form; values render sanitized.
pub(super) fn ui_spec_form_lines(
    form: &crate::tui::forms::UiSpecForm,
) -> Vec<Line<'static>> {
    use crate::tui::forms::UiSpecField;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut rows: Vec<(UiSpecField, Option<String>)> = Vec::new();
    if form.editing.is_none() {
        rows.push((UiSpecField::Id, form.id.clone()));
    }
    rows.push((UiSpecField::Title, form.title.clone()));
    rows.push((UiSpecField::Feed, form.feed.clone()));
    rows.push((UiSpecField::Placement, form.placement.clone()));
    rows.push((
        UiSpecField::MaxRows,
        form.max_rows.map(|size| size.to_string()),
    ));
    rows.push((
        UiSpecField::MaxCols,
        form.max_cols.map(|size| size.to_string()),
    ));
    rows.push((
        UiSpecField::Enabled,
        form.enabled.map(
            |flag| {
                if flag { "y".to_owned() } else { "n".to_owned() }
            },
        ),
    ));
    for (field, stored) in rows {
        let label = field.label();
        let description = field.description();
        let is_current = field == form.field && form.completed.is_none();
        let has_stored = stored.is_some();
        // User-derived values render through the neutral
        // sanitizer, not the provider redactor: titles and params
        // may legitimately contain `key:` or `@` shapes.
        let display = if is_current {
            format!(
                "> {label}: {}",
                crate::sanitize::sanitize_for_display(&form.input)
            )
        } else if let Some(value) = stored {
            if value.is_empty() {
                format!("  {label}: —")
            } else {
                format!(
                    "  {label}: {}",
                    crate::sanitize::sanitize_for_display(&value)
                )
            }
        } else {
            format!("  {label}:")
        };
        let style = if is_current {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else if has_stored {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(display).style(style));
        lines.push(
            Line::from(format!("    {description}"))
                .style(Style::default().fg(Color::DarkGray)),
        );
    }
    if !form.params.is_empty() || form.field == UiSpecField::Params {
        lines.push(
            Line::from("    params so far:")
                .style(Style::default().fg(Color::DarkGray)),
        );
        for (key, value) in &form.params {
            lines.push(Line::from(format!(
                "      {}",
                crate::sanitize::sanitize_for_display(&format!(
                    "{key}={value}"
                ))
            )));
        }
    }
    lines.push(
        Line::from("    Enter to continue, Esc to cancel")
            .style(Style::default().fg(Color::DarkGray)),
    );
    if let Some(err) = &form.error {
        lines.push(
            Line::from(format!(
                "    error: {}",
                crate::sanitize::sanitize_for_display(err)
            ))
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
    let (transcript_area, busy_area, input_area, status_area, pane_area) =
        match pane {
            None => {
                // I5 OFF: transcript spans full width, no gap (Min(0) fill)
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(busy_rows),
                        Constraint::Length(1),
                        Constraint::Length(1),
                    ])
                    .split(area);
                (chunks[0], chunks[1], chunks[2], chunks[3], None)
            }
            Some(_) => {
                // I5 ON: transcript Min(0) + pane Length(40) fills width, no gap
                let outer = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(0), Constraint::Length(1)])
                    .split(area);
                let cols = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(CONTEXT_PANE_WIDTH),
                    ])
                    .split(outer[0]);
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(0),
                        Constraint::Length(busy_rows),
                        Constraint::Length(1),
                    ])
                    .split(cols[0]);
                (rows[0], rows[1], rows[2], outer[1], Some(cols[1]))
            }
        };

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
            let available_height = (area.height.saturating_sub(2)) as usize;
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
                .min(area.height.saturating_sub(2))
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

    let safe_input = crate::sanitize::sanitize_for_display(&state.input);
    let input_text = format!("> {safe_input}");
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

    // Bottom row — same single row the Frame path composes.
    {
        let status = safe_display_label(&state.status);
        let bottom = compose_bottom_row(
            status_area.width,
            &status,
            state.provider.as_deref(),
            state.model.as_deref(),
            &state.metrics,
        );
        Paragraph::new(bottom).render(status_area, &mut buf);
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

    if let Some(modal) =
        state.pending_approval.as_ref().and_then(|a| a.modal())
    {
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
        let text: Vec<Line<'_>> = modal
            .lines
            .iter()
            .map(|line| Line::from(safe_form_message(line)))
            .collect();
        let paragraph = Paragraph::new(Text::from(text))
            .style(Style::default().fg(Color::White).bg(Color::Black))
            .wrap(ratatui::widgets::Wrap { trim: false });
        paragraph.render(inner, &mut buf);
    }

    buf
}
