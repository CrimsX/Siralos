//! Approval handling: provider-remove confirmation, pending-approval
//! key routing, and the shared input-queue approval read.

use super::{
    BufRead, InteractiveError, Path, WorkspaceProfileWriteToken, Write, io,
    load_workspace_profile_write_token, remove_profile_config_at,
    sanitize_for_display,
};

/// Resolve a provider-removal confirmation using a fresh observation when
/// the caller has not retained one. Callers that observed the profile before
/// opening an approval prompt should use [`apply_provider_remove_confirmation_at`].
#[must_use]
pub fn apply_provider_remove_confirmation(
    workspace_root: &Path,
    decision: crate::tui::ApprovalDecision,
) -> String {
    let Some(observed) = load_workspace_profile_write_token(workspace_root)
    else {
        return "provider removal failed (details hidden)\n".to_owned();
    };
    apply_provider_remove_confirmation_at(workspace_root, &observed, decision)
}

/// Resolve a `y/N` provider-removal confirmation into the transcript
/// message — the SINGLE outcome both frontends call (one implementation).
/// `Approve` removes via [`remove_profile_config_at`] and mirrors the save
/// message; `Deny` cancels truthfully without touching the file.
#[must_use]
pub fn apply_provider_remove_confirmation_at(
    workspace_root: &Path,
    observed: &WorkspaceProfileWriteToken,
    decision: crate::tui::ApprovalDecision,
) -> String {
    apply_provider_remove_confirmation_at_with_status(
        workspace_root,
        observed,
        decision,
    )
    .0
}

fn apply_provider_remove_confirmation_at_with_status(
    workspace_root: &Path,
    observed: &WorkspaceProfileWriteToken,
    decision: crate::tui::ApprovalDecision,
) -> (String, bool) {
    match decision {
        crate::tui::ApprovalDecision::Approve => {
            match remove_profile_config_at(workspace_root, observed) {
                Ok(()) => (
                    "provider removed from siralos.toml - restart the session to apply\n"
                        .to_owned(),
                    true,
                ),
                Err(_error) => {
                    ("provider removal failed (details hidden)\n".to_owned(), false)
                }
            }
        }
        crate::tui::ApprovalDecision::Deny => {
            ("provider removal cancelled\n".to_owned(), false)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ApprovalKeyOutcome {
    pub(super) decision: crate::tui::ApprovalDecision,
    pub(super) removal_committed: bool,
}

/// Resolve one keypress while a TUI approval modal is pending — the SINGLE
/// step the live event loop calls (same `&RefCell<TuiState>` shape the loop
/// holds). The boolean wrapper preserves the historical public seam; the
/// richer internal result lets the live loop gate reload on an approved,
/// successful removal rather than on any key that merely closed the modal.
///
/// Provider-removal confirmations (armed by `/provider remove` or the picker
/// row) resolve through [`apply_provider_remove_confirmation`] and report
/// through the sink; ordinary approvals keep the historical `Approved.` /
/// `Denied.` transcript line.
pub(super) fn handle_pending_approval_key_outcome(
    tui_state: &std::cell::RefCell<crate::tui::TuiState>,
    key: crossterm::event::KeyEvent,
    workspace_root: &Path,
    sink: &mut crate::tui::TuiSink,
) -> Option<ApprovalKeyOutcome> {
    // Take the decision first: this block ends the mutable borrow before
    // the body below touches `tui_state` again. (Edition 2024 extends a
    // scrutinee `borrow_mut()` temporary over the whole `if let` body, so
    // borrowing inside that body panics with "already mutably borrowed".)
    let decision = {
        let mut state = tui_state.borrow_mut();
        crate::tui::handle_modal_key(&mut state, key)
    };
    let decision = decision?;
    let confirming_removal = tui_state.borrow().confirming_provider_removal;
    let observed = tui_state.borrow_mut().pending_profile_write_token.take();
    tui_state.borrow_mut().pending_approval = None;
    tui_state.borrow_mut().confirming_provider_removal = false;
    let mut removal_committed = false;
    if confirming_removal {
        // Provider-removal confirmation: resolve through the single
        // outcome both frontends call. The production TUI path supplies the
        // revision observed before the modal; the fallback keeps direct
        // callers/tests truthful while still obtaining a fresh token.
        let removal = if let Some(observed) = observed.as_ref() {
            apply_provider_remove_confirmation_at_with_status(
                workspace_root,
                observed,
                decision,
            )
        } else if let Some(observed) =
            load_workspace_profile_write_token(workspace_root)
        {
            apply_provider_remove_confirmation_at_with_status(
                workspace_root,
                &observed,
                decision,
            )
        } else {
            ("provider removal failed (details hidden)\n".to_owned(), false)
        };
        removal_committed = removal.1;
        let removal = removal.0;
        let rendered = sanitize_for_display(&removal);
        let _ = sink.write_all(rendered.as_bytes());
    } else {
        let verdict = match decision {
            crate::tui::ApprovalDecision::Approve => "Approved.",
            crate::tui::ApprovalDecision::Deny => "Denied.",
        };
        tui_state.borrow_mut().push_line(verdict.to_owned());
    }
    Some(ApprovalKeyOutcome { decision, removal_committed })
}

/// Resolve one approval-modal key while preserving the historical boolean
/// return used by tests and non-removal callers.
pub fn handle_pending_approval_key(
    tui_state: &std::cell::RefCell<crate::tui::TuiState>,
    key: crossterm::event::KeyEvent,
    workspace_root: &Path,
    sink: &mut crate::tui::TuiSink,
) -> bool {
    handle_pending_approval_key_outcome(tui_state, key, workspace_root, sink)
        .is_some()
}

/// Live-loop seam for mouse-wheel transcript scrolling (the modal-fix
/// pattern): the event loop calls this one function, which routes the
/// [`crossterm::event::MouseEvent`] into [`crate::tui::handle_mouse`].
/// Returns `true` when the event moved `scroll_offset` (the loop redraws
/// every drained batch at the loop bottom, so a move is visible on the
/// next draw); `false` for ignored events (non-wheel kinds, modal open,
/// already at the clamp edge).
pub fn handle_tui_mouse(
    tui_state: &std::cell::RefCell<crate::tui::TuiState>,
    event: crossterm::event::MouseEvent,
    viewport_height: u16,
) -> bool {
    let before = tui_state.borrow().scroll_offset;
    crate::tui::handle_mouse(
        &mut tui_state.borrow_mut(),
        event,
        viewport_height,
    );
    tui_state.borrow().scroll_offset != before
}

/// Shared approval gate — same function both frontends call (T2 consolidation).
///
/// The stdio path reads a line via the input-queue (`BufRead::read_line`) and
/// the TUI path feeds the modal's `y`/`n` through the same evaluation via a
/// `Cursor` over the modal answer — no parallel approval logic.
pub fn read_approval_via_input_queue<R: BufRead>(
    reader: &mut R,
) -> Result<crate::tui::ApprovalDecision, InteractiveError> {
    const MAX_APPROVAL_INPUT_BYTES: usize = 64 * 1024;
    let mut bytes = Vec::new();
    let n = std::io::Read::take(
        reader,
        u64::try_from(MAX_APPROVAL_INPUT_BYTES + 1).unwrap_or(u64::MAX),
    )
    .read_until(b'\n', &mut bytes)
    .map_err(InteractiveError::Io)?;
    if bytes.len() > MAX_APPROVAL_INPUT_BYTES {
        return Err(InteractiveError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "approval input exceeds the bounded line length",
        )));
    }
    if n == 0 {
        return Ok(crate::tui::ApprovalDecision::Deny);
    }
    let line = String::from_utf8(bytes).map_err(|_| {
        InteractiveError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "approval input is not valid UTF-8",
        ))
    })?;
    Ok(crate::tui::evaluate_approval_input(&line))
}

/// Convenience shared gate for `&str` inputs (both loops call the same
/// `evaluate_approval_input` defined in `tui.rs` — single definition).
pub fn shared_evaluate_approval(input: &str) -> crate::tui::ApprovalDecision {
    crate::tui::evaluate_approval_input(input)
}

/// T2 helper: build a bounded, sanitized approval modal from the lines the
/// session rendered through the sink (exactly as stdio would render them).
/// The stdio render is already bounded; the modal shows at most the last
/// 30 lines with a truncation marker (see `ApprovalModal::new`).
pub fn build_approval_modal(lines: Vec<String>) -> crate::tui::ApprovalModal {
    crate::tui::ApprovalModal::new(lines)
}
