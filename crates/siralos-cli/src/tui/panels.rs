//! Declarative UI panel host (ADR 0039 §6).
//!
//! Renders a [`UiPanels`] snapshot: a tab bar over a content area
//! showing either the selected tab full-bleed or the 2×2 split.
//! Rendering is entirely host-owned and deterministic —
//! byte-identical buffers for identical snapshots — over already
//! sanitized feed content. The transcript `draw` path is untouched;
//! this view renders only while the frontend's panels mode is on.

use ratatui::{
    Frame,
    backend::TestBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Paragraph},
};

use siralos_core::ui::{
    UiComponentSpec, UiFeed, UiProjection, UnvalidatedSpec,
};

use super::state::TuiState;
use super::text::{display_width, truncate_to_width};
use crate::sanitize::sanitize_for_display;

/// Fixed grid tab label, always last in the tab bar.
pub const GRID_TAB_LABEL: &str = "grid";
/// Placeholder title for empty grid cells and the empty host.
pub const EMPTY_CELL_TITLE: &str = "(empty)";
/// Status hint rendered in the panels view bottom row.
pub const PANELS_STATUS_HINT: &str = "F2/Esc transcript · [ ] switch tab";

/// One panels-view snapshot: a projection plus the selected tab
/// index (`tabs.len()` addresses the fixed grid tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiPanels {
    /// Projected tabs and grid cells.
    pub projection: UiProjection,
    /// Selected tab (`0..=tabs.len()`; `== tabs.len()` is grid).
    pub selected: usize,
}

impl UiPanels {
    /// Clamp the selection into range after the projection moves.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        let tabs = self.projection.tabs.len();
        if self.selected > tabs {
            self.selected = tabs;
        }
        self
    }

    /// Tab titles in bar order, grid tab last.
    #[must_use]
    pub fn tab_titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = self
            .projection
            .tabs
            .iter()
            .map(|slot| slot.title.clone())
            .collect();
        titles.push(GRID_TAB_LABEL.to_owned());
        titles
    }

    /// Move selection one tab forward (`backward` goes back),
    /// wrapping across the grid tab.
    pub fn cycle_tab(&mut self, backward: bool) {
        let count = self.projection.tabs.len() + 1;
        if backward {
            self.selected = self.selected.checked_sub(1).unwrap_or(count - 1);
        } else {
            self.selected = (self.selected + 1) % count;
        }
    }
}

/// The four starter component specs proving the model (ADR 0039
/// D6): module status, workspace listing, context summary, tools
/// catalog. Pure spec data; feed resolution arrives with the
/// authoring flow.
#[must_use]
pub fn starter_specs() -> Vec<UiComponentSpec> {
    [
        ("mod-status", "Modules", "modules.status", "tab"),
        ("ws-list", "Workspace", "workspace.list", "cell:0,0"),
        ("ctx-summary", "Context", "context.summary", "cell:0,1"),
        ("tools-catalog", "Tools", "tools.catalog", "cell:1,0"),
    ]
    .iter()
    .map(|(id, title, feed, placement)| {
        UiComponentSpec::parse(&UnvalidatedSpec {
            id: (*id).to_owned(),
            title: (*title).to_owned(),
            feed: (*feed).to_owned(),
            enabled: true,
            placement: (*placement).to_owned(),
            max_rows: 20,
            max_cols: 60,
            params: Vec::new(),
        })
        .expect("starter specs are valid by construction")
    })
    .collect()
}

/// Feed behind one starter spec, for merge and test plumbing.
#[must_use]
pub fn starter_feed(id: &str) -> Option<UiFeed> {
    starter_specs()
        .into_iter()
        .find(|spec| spec.id() == id)
        .map(|spec| spec.feed())
}

/// Sanitize one feed line for panel display. Single choke point:
/// every caller passes raw feed content through here before it
/// reaches the projector or a buffer.
#[must_use]
pub fn sanitize_panel_line(line: &str) -> String {
    sanitize_for_display(line)
}

/// Draw the panels view from live frontend state: header,
/// tab bar, content, status hint. A missing snapshot renders the
/// sane empty host.
pub fn draw_panels(state: &TuiState, frame: &mut Frame<'_>) {
    let provider = match (&state.provider, &state.model) {
        (Some(provider), Some(model)) => {
            Some((provider.as_str(), model.as_str()))
        }
        (Some(provider), _) => Some((provider.as_str(), "")),
        _ => None,
    };
    draw_panels_snapshot(
        state.panels.as_ref(),
        provider,
        &state.status,
        frame,
    );
}

/// Draw one panels snapshot: header, tab bar, content, status
/// hint. `None` renders the sane empty host.
pub fn draw_panels_snapshot(
    panels: Option<&UiPanels>,
    provider: Option<(&str, &str)>,
    status: &str,
    frame: &mut Frame<'_>,
) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    render_header(provider, frame, rows[0]);
    render_tab_bar(panels, frame, rows[1]);
    render_content(panels, frame, rows[2]);
    let hint = truncate_to_width(
        &format!(
            "{} | {PANELS_STATUS_HINT}",
            truncate_to_width(status, area.width as usize / 2)
        ),
        area.width as usize,
    );
    frame.render_widget(Paragraph::new(hint), rows[3]);
}

fn render_header(
    provider: Option<(&str, &str)>,
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
) {
    use super::forms::safe_display_label;
    let width = area.width as usize;
    let left = " Siralos panels ";
    let right_opt: Option<String> = match provider {
        Some((provider, model))
            if !provider.is_empty() && !model.is_empty() =>
        {
            Some(format!(
                "{} / {}",
                safe_display_label(provider),
                safe_display_label(model)
            ))
        }
        Some((provider, _)) if !provider.is_empty() => {
            Some(safe_display_label(provider))
        }
        _ => None,
    };
    let left_len = display_width(left);
    let text = if let Some(right) = right_opt {
        let middle = width.saturating_sub(left_len + display_width(&right));
        format!("{left}{}{right}", " ".repeat(middle))
    } else {
        format!("{left}{}", " ".repeat(width.saturating_sub(left_len)))
    };
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .fg(Color::Cyan)
                .bg(Color::Black)
                .add_modifier(Modifier::REVERSED),
        ),
        area,
    );
}

fn render_tab_bar(
    panels: Option<&UiPanels>,
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
) {
    let width = area.width as usize;
    let (titles, selected) = match panels {
        Some(panels) => (panels.tab_titles(), panels.selected),
        None => (vec![GRID_TAB_LABEL.to_owned()], 0),
    };
    let mut spans = vec![Span::raw(" ")];
    for (index, title) in titles.iter().enumerate() {
        let label = format!("[{title}] ");
        if index == selected {
            spans.push(Span::styled(
                label,
                Style::default().add_modifier(Modifier::REVERSED),
            ));
        } else {
            spans.push(Span::raw(label));
        }
    }
    let text: String =
        spans.iter().map(|span| span.content.as_ref()).collect();
    frame.render_widget(
        Paragraph::new(truncate_to_width(&text, width))
            .style(Style::default().fg(Color::Yellow)),
        area,
    );
}

fn render_content(
    panels: Option<&UiPanels>,
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
) {
    let show_grid = match panels {
        Some(panels) => panels.selected >= panels.projection.tabs.len(),
        None => true,
    };
    if show_grid {
        render_grid(panels, frame, area);
    } else if let Some(panels) = panels {
        let slot = &panels.projection.tabs[panels.selected];
        render_slot(&slot.title, &slot.lines, frame, area);
    }
}

fn render_grid(
    panels: Option<&UiPanels>,
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(area);
    for (row, row_area) in rows.iter().enumerate() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(*row_area);
        for (col, cell_area) in cols.iter().enumerate() {
            let slot = panels
                .and_then(|panels| panels.projection.grid[row][col].as_ref());
            match slot {
                Some(slot) => {
                    render_slot(&slot.title, &slot.lines, frame, *cell_area)
                }
                None => render_slot(EMPTY_CELL_TITLE, &[], frame, *cell_area),
            }
        }
    }
}

fn render_slot(
    title: &str,
    lines: &[String],
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let mut rows: Vec<String> = lines
        .iter()
        .take(inner_height)
        .map(|line| truncate_to_width(line, inner_width))
        .collect();
    while rows.len() < inner_height {
        rows.push(String::new());
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .title(truncate_to_width(title, inner_width));
    frame.render_widget(Paragraph::new(rows.join("\n")).block(block), area);
}

/// Render one panels view to an off-screen buffer (tests +
/// differential harness pinning).
#[must_use]
pub fn render_panels_to_buffer(
    panels: Option<&UiPanels>,
    provider: Option<(&str, &str)>,
    status: &str,
    width: u16,
    height: u16,
) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| draw_panels_snapshot(panels, provider, status, frame))
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// Cycle the tab selection of staged panels state. Missing or
/// out-of-range state clamps instead of panicking.
pub fn cycle_panels_tab(panels: &mut Option<UiPanels>, backward: bool) {
    if let Some(state) = panels {
        let tabs = state.projection.tabs.len();
        if state.selected > tabs {
            state.selected = tabs;
        }
        state.cycle_tab(backward);
    }
}
