//! Cell-width text measurement: wrapping and truncation shared by
//! the transcript frame and the context pane.

/// Return the terminal cell width used by ratatui. Keeping this calculation
/// in one place prevents byte/character cursor placement from disagreeing
/// with the widget that actually paints the text (especially for CJK and
/// emoji).
pub(super) fn display_width(text: &str) -> usize {
    ratatui::text::Line::from(text).width()
}

/// Wrap one stored transcript line over `width` terminal columns.
///
/// Word-boundary wrapping is retained, but all decisions use display width
/// rather than UTF-8 bytes or scalar count. The stored text is never mutated.
#[must_use]
pub fn wrap_line_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    if display_width(text) <= width {
        return vec![text.to_owned()];
    }
    // Compute scalar display widths once. The previous implementation rebuilt
    // `chars[start..=end]` and recalculated its terminal width for every
    // candidate, which made a bounded 64 KiB line quadratic.
    let cells: Vec<(usize, char, usize)> = text
        .char_indices()
        .map(|(index, ch)| {
            let cell = display_width(&ch.to_string()).max(1);
            (index, ch, cell)
        })
        .collect();
    let mut rows = Vec::new();
    let mut start = 0usize;
    while start < cells.len() {
        let mut end = start;
        let mut used = 0usize;
        let mut last_space = None;
        while end < cells.len() {
            let (_, ch, cell_width) = cells[end];
            if used.saturating_add(cell_width) > width {
                break;
            }
            used = used.saturating_add(cell_width);
            if ch == ' ' {
                last_space = Some(end);
            }
            end += 1;
        }
        if end == start {
            // A wide scalar is wider than the viewport; keep it visible.
            end += 1;
        } else if end < cells.len() && cells[end].1 == ' ' {
            let byte_start = cells[start].0;
            let byte_end = cells[end].0;
            // The break space is a separator, not rendered content.
            rows.push(
                text[byte_start..byte_end].trim_end_matches(' ').to_owned(),
            );
            start = end + 1;
            continue;
        } else if let Some(break_at) =
            last_space.filter(|index| *index > start && end < cells.len())
        {
            let byte_start = cells[start].0;
            let byte_end = cells[break_at].0;
            rows.push(
                text[byte_start..byte_end].trim_end_matches(' ').to_owned(),
            );
            start = break_at + 1;
            continue;
        }
        let byte_start = cells[start].0;
        let byte_end =
            if end < cells.len() { cells[end].0 } else { text.len() };
        rows.push(text[byte_start..byte_end].to_owned());
        start = end;
    }
    rows
}

/// Truncate a line to `max_cells` terminal cells on a scalar boundary.
pub(super) fn truncate_to_width(line: &str, max_cells: usize) -> String {
    if display_width(line) <= max_cells {
        return line.to_owned();
    }
    let mut result = String::new();
    let mut used: usize = 0;
    for character in line.chars() {
        let width = display_width(&character.to_string());
        if used.saturating_add(width) > max_cells {
            break;
        }
        result.push(character);
        used += width;
    }
    result
}
