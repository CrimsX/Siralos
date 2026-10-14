//! Approval handling: provider-remove confirmation, pending-approval
//! key routing, and the shared input-queue approval read.

use super::{
    BufRead, InteractiveError, Path, Rc, RefCell, WorkspaceProfileWriteToken,
    Write, io, load_workspace_profile_write_token, remove_profile_config_at,
    render_approval_request, sanitize_for_display,
};
use siralos_core::tool::{ApprovalRequest, ApprovalVerdict};

/// Synchronous approval prompter for the stdio frontend.
///
/// The stdio loop drives the turn on its own thread, so the hook
/// prompts through the loop's own reader and writer, shared behind
/// `Rc<RefCell<..>>` (every use is one brief borrow; the hook never
/// runs while the loop holds one, and the loop never touches either
/// while blocked in the drain). The input queue stays the single
/// interactive-read owner: the verdict is read from the same queue
/// as prompts. A closed (EOF) or unreadable verdict input denies
/// fail-closed, exactly like the shared gate.
pub(crate) struct StdioApprovalPrompter<R, W> {
    reader: Rc<RefCell<R>>,
    writer: Rc<RefCell<W>>,
}

impl<R: BufRead, W: Write> StdioApprovalPrompter<R, W> {
    /// Build the prompter over the loop's shared handles.
    pub fn new(reader: Rc<RefCell<R>>, writer: Rc<RefCell<W>>) -> Self {
        Self { reader, writer }
    }

    /// The executor resolver: present the sanitized request, read
    /// one verdict line, and return it. Write failures are
    /// best-effort (the read still decides); read failures deny.
    /// The handles are `'static` so the resolver closure owns
    /// everything it captures (the application holds it without
    /// borrowing the loop).
    pub fn resolver(
        reader: Rc<RefCell<R>>,
        writer: Rc<RefCell<W>>,
    ) -> siralos_core::tool::SharedApprovalResolver
    where
        R: BufRead + 'static,
        W: Write + 'static,
    {
        let prompter = Self::new(reader, writer);
        siralos_core::tool::SharedApprovalResolver::new(move |request| {
            prompter.resolve(request)
        })
    }

    fn resolve(
        &self,
        request: &siralos_core::tool::ApprovalRequest,
    ) -> siralos_core::tool::ApprovalVerdict {
        use siralos_core::tool::ApprovalVerdict;
        // One borrow per statement: a `RefMut` temporary lives to the
        // end of its full expression, so chaining borrows panics.
        for line in render_approval_request(request) {
            let _ = self.writer.borrow_mut().write_all(line.as_bytes());
            let _ = self.writer.borrow_mut().write_all(b"\n");
        }
        let _ = self.writer.borrow_mut().write_all(b"Approve? (y/N)\n");
        let _ = self.writer.borrow_mut().flush();
        match read_approval_via_input_queue(&mut *self.reader.borrow_mut()) {
            Ok(crate::tui::ApprovalDecision::Approve) => {
                ApprovalVerdict::Approve
            }
            Ok(_) => ApprovalVerdict::Deny,
            Err(_) => ApprovalVerdict::Deny,
        }
    }
}

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

/// How long a blocked approval hook waits between shutdown checks.
/// The worker thread cannot serve anything while blocked in the hook,
/// so shutdown unblocks it through the shared stop flag instead of a
/// second channel; 50 ms bounds the exit latency with no busy spin.
const APPROVAL_POLL_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(50);

/// Worker↔frontend approval exchange for the TUI (Part B).
///
/// The turn runs on the worker thread while the terminal belongs to
/// the UI thread, so the synchronous hook cannot prompt directly.
/// Instead the hook registers a pending request, announces it as a
/// data-only `WorkerEvent::ApprovalRequest`, and blocks for the
/// verdict; the UI thread delivers it straight into the table (no
/// worker cooperation needed, so no deadlock is possible).
///
/// Sharing justification (RUST_STYLE concurrency): the blocked worker
/// and the deciding UI thread are different threads with no
/// pollable common channel (the worker is inside `poll_event`, the
/// UI inside its key pump), so the pending-verdict table is genuinely
/// shared. The lock protects only the table invariant (an entry
/// exists exactly while its hook is blocked); every critical section
/// is insert/remove/send with no blocking and no nesting.
#[derive(Clone)]
pub(crate) struct ApprovalExchange {
    state: std::sync::Arc<std::sync::Mutex<ExchangeState>>,
    events: std::sync::mpsc::SyncSender<crate::session_worker::WorkerEvent>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct ExchangeState {
    next_id: u64,
    pending: std::collections::HashMap<
        u64,
        std::sync::mpsc::Sender<ApprovalVerdict>,
    >,
}

impl std::fmt::Debug for ApprovalExchange {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let pending = self
            .state
            .lock()
            .map(|state| state.pending.len())
            .unwrap_or(usize::MAX);
        formatter
            .debug_struct("ApprovalExchange")
            .field("pending", &pending)
            .finish_non_exhaustive()
    }
}

impl ApprovalExchange {
    /// Build the exchange both threads share. `events` carries the
    /// request announcement; `stop` unblocks hooks on shutdown.
    pub fn new(
        events: std::sync::mpsc::SyncSender<
            crate::session_worker::WorkerEvent,
        >,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(
                ExchangeState::default(),
            )),
            events,
            stop,
        }
    }

    /// A disconnected exchange for handles without a worker (tests,
    /// scripted sources): every request denies, nothing blocks.
    #[cfg(test)]
    pub fn detached() -> Self {
        let (events, _) = std::sync::mpsc::sync_channel(1);
        Self::new(
            events,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    }

    /// Worker side: announce one request and block for its verdict.
    /// A dead frontend, a shutdown, or an unknown failure denies
    /// fail-closed; only an explicit approve approves.
    pub fn request_blocking(
        &self,
        tool_name: &str,
        lines: Vec<String>,
    ) -> ApprovalVerdict {
        let (id, receiver) = {
            let mut state = self.lock_state();
            let id = state.next_id;
            state.next_id = state.next_id.wrapping_add(1);
            let (sender, receiver) = std::sync::mpsc::channel();
            state.pending.insert(id, sender);
            (id, receiver)
        };
        let announced = self.events.send(
            crate::session_worker::WorkerEvent::ApprovalRequest {
                id,
                tool_name: tool_name.to_owned(),
                lines,
            },
        );
        if announced.is_err() {
            self.remove(id);
            return ApprovalVerdict::Deny;
        }
        loop {
            match receiver.recv_timeout(APPROVAL_POLL_INTERVAL) {
                Ok(verdict) => {
                    self.remove(id);
                    return verdict;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.remove(id);
                    return ApprovalVerdict::Deny;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if self.stop.load(std::sync::atomic::Ordering::SeqCst) {
                        self.remove(id);
                        return ApprovalVerdict::Deny;
                    }
                }
            }
        }
    }

    /// UI side: deliver one verdict. Returns `false` for an unknown
    /// or already-resolved id. This layer stays silent (the verdict
    /// path renders loudly); an entry orphaned by a dead worker
    /// thread can only make this return `false`, never hang: the
    /// hook it belonged to is gone with its thread.
    pub fn deliver(&self, id: u64, verdict: ApprovalVerdict) -> bool {
        let sender = self.lock_state().pending.remove(&id);
        match sender {
            Some(sender) => sender.send(verdict).is_ok(),
            None => false,
        }
    }

    /// Pending request count (tests only).
    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.lock_state().pending.len()
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, ExchangeState> {
        self.state.lock().unwrap_or_else(|poison| {
            // Critical sections never panic (insert/remove/send
            // only), so poison means a foreign bug; the table itself
            // stays usable.
            poison.into_inner()
        })
    }

    fn remove(&self, id: u64) {
        self.lock_state().pending.remove(&id);
    }
}

/// Build the executor resolver over one exchange (worker thread).
///
/// Rendering reuses the single shared helper, so the modal shows
/// exactly what stdio would print.
pub(crate) fn exchange_resolver(
    exchange: ApprovalExchange,
) -> siralos_core::tool::SharedApprovalResolver {
    siralos_core::tool::SharedApprovalResolver::new(
        move |request: &ApprovalRequest| {
            exchange.request_blocking(
                &request.tool_name,
                super::render_approval_request(request),
            )
        },
    )
}

/// T2 helper: build a bounded, sanitized approval modal from the lines the
/// session rendered through the sink (exactly as stdio would render them).
/// The stdio render is already bounded; the modal shows at most the last
/// 30 lines with a truncation marker (see `ApprovalModal::new`).
pub fn build_approval_modal(lines: Vec<String>) -> crate::tui::ApprovalModal {
    crate::tui::ApprovalModal::new(lines)
}
