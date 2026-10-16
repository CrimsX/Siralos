//! Approval decisions and modal handling for TUI prompts.

use super::{APPROVAL_TRUNCATION_MARKER, MAX_APPROVAL_LINES, TuiState};

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
    state.pending_approval.as_ref()?.modal()?;
    modal_key_decision(key)
}
