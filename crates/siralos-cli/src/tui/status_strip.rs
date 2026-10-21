//! The always-visible status strip (ADR 0039 §2 amendment
//! 2026-10-20).
//!
//! `status`-placed components render as one composed summary line in
//! the TUI bottom region, directly above the metrics row. The strip
//! feeds from the staged panels snapshot — the same worker report
//! the F2 panel host renders — restaged by the idle loop once per
//! [`super::STATUS_STRIP_REFRESH`] period. Staged content never
//! ticks on its own; content older than
//! [`super::STATUS_STRIP_STALE`] earns an honest `staged Nm ago`
//! marker instead of pretending liveness. When nothing projects,
//! the strip claims no row and the frame stays byte-identical to
//! the no-component one.

use super::{
    Color, Duration, Line, STATUS_STRIP_STALE, Span, Style, UiPanels,
    display_width, truncate_to_width,
};

/// The honest staleness marker for staged strip content: `None`
/// within the grace window, one settled minute count beyond it.
#[must_use]
pub fn status_stale_marker(elapsed: Duration) -> Option<String> {
    if elapsed < STATUS_STRIP_STALE {
        return None;
    }
    let minutes = std::cmp::max(1, elapsed.as_secs() / 60);
    Some(format!("staged {minutes}m ago"))
}

/// The strip content for one staged snapshot: `Title: line · line`
/// fragments joined with a middle dot with empty-content slots
/// absent, truncated to `width` cells while reserving room for the
/// marker. The marker itself is NOT part of the returned string;
/// [`compose_status_strip`] renders it dimmed. `None` when nothing
/// renders — no snapshot, no `status` projection, or every slot
/// empty.
#[must_use]
pub fn compose_status_strip_text(
    panels: Option<&UiPanels>,
    width: u16,
    stale_marker: Option<&str>,
) -> Option<String> {
    let projection = &panels.as_ref()?.projection;
    let mut fragments: Vec<String> = Vec::new();
    for slot in &projection.status_strip {
        if slot.lines.is_empty() {
            continue;
        }
        let joined = slot.lines.join(" · ");
        fragments.push(format!("{}: {}", slot.title, joined));
    }
    if fragments.is_empty() {
        return None;
    }
    let content = fragments.join(" · ");
    let budget = usize::from(width).saturating_sub(
        stale_marker.map_or(0, |marker| 3 + display_width(marker)),
    );
    Some(truncate_to_width(&content, budget))
}

/// The styled strip line for the live draw: plain content plus the
/// dimmed staleness marker, when one is claimed. Return value
/// matches [`compose_status_strip_text`] verbatim in its first
/// span.
#[must_use]
pub fn compose_status_strip(
    panels: Option<&UiPanels>,
    width: u16,
    stale_marker: Option<&str>,
) -> Option<Line<'static>> {
    let content = compose_status_strip_text(panels, width, stale_marker)?;
    let mut spans: Vec<Span<'static>> = vec![Span::raw(content)];
    if let Some(marker) = stale_marker {
        spans.push(Span::styled(
            format!(" — {marker}"),
            Style::default().fg(Color::DarkGray),
        ));
    }
    Some(Line::from(spans))
}
