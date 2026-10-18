//! Worker pump plumbing: prompt round-trips over the worker seam,
//! pending-reply draining, and the shared event drain.

use super::{
    EventSource, InteractiveError, ModelListingGate, Path, Rc, RefCell,
    TerminalSanitizer, ToolLoopEvent, TuiState, WorkerCommand, WorkerEvent,
    WorkerSource, WorkerWait, Write, model_listing_diagnostic,
    model_listing_gate, persist_switched_model, safe_profile_diagnostic,
    sanitize_for_display,
};

/// How long a relay waits for the worker before it runs the frontend's tick.
///
/// This IS the point of the worker boundary: with the session on the other
/// thread the frontend owns its clock, so the reveal, the pulse, the thinking
/// expansion and the interrupt key keep their cadence while the model is
/// silent. It is the same interval the sink's redraw throttle uses.
const WORKER_WAIT: std::time::Duration = crate::tui::REDRAW_INTERVAL;

/// Ceiling on how long a COMMAND relay waits for its answer.
///
/// A `/context`, `/models`, `/model` or `/reload` asks the worker for one
/// reply. A worker that never produces it must become a typed error, not an
/// interactive session the user cannot leave with Ctrl+C. A turn relay is
/// deliberately excluded: the turn is the user's own request in flight.
const WORKER_ANSWER_DEADLINE: std::time::Duration =
    std::time::Duration::from_secs(300);

/// Consecutive empty zero-timeout polls before the relay stops hot-spinning.
const ZERO_TIMEOUT_IDLE_LIMIT: usize = 64;

/// Shared worker-vanished notice: the exact user-facing sentence in
/// one definition. Sink-write sites append the newline; error
/// values carry the bare sentence. (The TUI-drain variant names its
/// own waiter and stays distinct.)
const WORKER_STOPPED_MESSAGE: &str = "worker stopped before it finished";

/// When a worker relay stops (C2 step 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Until {
    /// A turn: the worker's `TurnFinished`, or a worker that stopped first.
    TurnFinished,
    /// One answer to a request command: the first `Report`, `Failed` or
    /// `Models`. The CALLER renders it, because each arm owns its wording.
    Answer,
    /// An applied composition change: the `Ready` that follows, or a `Failed`.
    Applied,
    /// A status-only command (`SetModel`): the worker answers with exactly one
    /// `Ready` on success or one `Failed` on refusal. Unlike `Applied`, there is
    /// no report to wait for.
    Status,
    /// Nothing is expected (C3): apply whatever the worker has ALREADY
    /// produced and return. This is the frame-level sweep, so an event that
    /// arrives between commands cannot sit in the channel unread.
    Drain,
}

/// What one relay run collected, beyond what it applied itself.
#[derive(Debug, Default)]
pub(super) struct ReplyEffects {
    /// The answer event, when the caller asked for one.
    pub(super) answer: Option<WorkerEvent>,
    /// The worker stopped before the wait was satisfied.
    stopped: bool,
    /// The stop was observed as the worker's terminal `Stopped` event, not
    /// merely as a disconnected channel. A drain keeps this distinction so a
    /// real stop can become a frontend error while a scripted/closed channel
    /// remains quiet.
    pub(super) stopped_event: bool,
    /// A tool approval request suspended the turn: the exchange id the
    /// dispatcher's modal verdict must be delivered to. Set with the
    /// modal; the dispatcher pumps the modal, delivers, and re-enters
    /// for the remainder of the turn.
    pub(super) approval_pending: Option<u64>,
}

/// Apply the worker's header snapshot to the frontend's own state (C2 step 3).
///
/// The header, the picker's values and the context suffix are all derived from
/// the composition, which lives in the worker now (decision 168 R3: display may
/// cross, authority may not). This is the frontend half of `Ready`.
fn apply_status(
    state: &Rc<RefCell<TuiState>>,
    status: &crate::session_worker::SessionStatus,
) {
    let mut state = state.borrow_mut();
    state.status = status.status.clone();
    state.provider = status.provider.clone();
    state.model = status.model.clone();
    state.endpoint = status.endpoint.clone();
    state.protocol = status.protocol.clone();
    state.credential_display = status.credential_display.clone();
    state.credential_resolved = status.credential_resolved;
    state.live_model_switchable = status.live_model_switchable;
    state.metrics = status.metrics.clone();
}

/// Relay worker events into the frontend (C2 step 3).
///
/// This is the TUI's drain. The channel is read in order; the events that are
/// frontend STATE (the header snapshot, the pane, the model list) are applied
/// here; the events that are transcript cross the terminal sanitizer through
/// the one shared bridge, so the sanitizer stays the single output boundary and
/// a failure keeps the wording the stdio frontend shows.
///
/// `progress` runs on every event AND on every idle tick, exactly as the
/// session's keep-alive ticks did, so liveness no longer depends on the
/// provider's cadence.
#[allow(clippy::too_many_arguments)]
pub(super) fn pump_worker(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
    until: Until,
) -> Result<ReplyEffects, InteractiveError> {
    let mut effects = ReplyEffects::default();
    // `/reload` is an applied-composition command: the worker sends the
    // human-readable report (or failure) and then the refreshed `Ready`
    // snapshot. Keep both sides of that protocol before returning, so a
    // successful reload cannot finish with a stale header.
    let mut applied_answer_seen = false;
    let mut applied_ready_seen = false;
    // A command relay is BOUNDED. A turn relay and a drain are not: a turn is
    // the user's request in flight, and a drain is the frame's own sweep.
    let answer_deadline: Option<std::time::Instant> = match until {
        Until::Answer | Until::Applied | Until::Status => {
            std::time::Instant::now().checked_add(WORKER_ANSWER_DEADLINE)
        }
        Until::TurnFinished | Until::Drain => None,
    };
    // Consecutive zero-timeout idles (owed text, no paint loop advancing it)
    // before the relay stops hot-spinning on an empty channel.
    let mut zero_timeout_idles = 0usize;
    let mut stop;
    loop {
        if let Some(limit) = answer_deadline {
            if std::time::Instant::now() >= limit {
                return Err(InteractiveError::Worker(
                    "worker did not answer in time".to_owned(),
                ));
            }
        }
        // A drain never waits: an empty channel is the END of its work, not a
        // tick to sit through. Neither does a turn while the reader is still
        // owed text: the tick IS the frame, one character per frame, and
        // matching the model's speed means painting that frame as soon as the
        // previous one is done rather than on a timer. A caller that never
        // advances the reveal (a test, a stalled paint loop) falls back to a
        // real wait instead of spinning on the channel.
        let timeout = match until {
            Until::Drain => std::time::Duration::ZERO,
            _ if state.borrow().reveal_pending()
                && zero_timeout_idles < ZERO_TIMEOUT_IDLE_LIMIT =>
            {
                std::time::Duration::ZERO
            }
            _ => WORKER_WAIT,
        };
        let event = match worker.wait(timeout) {
            WorkerWait::Event(event) => {
                zero_timeout_idles = 0;
                event
            }
            WorkerWait::Idle => {
                if until == Until::Drain {
                    break;
                }
                if timeout.is_zero() {
                    zero_timeout_idles = zero_timeout_idles.saturating_add(1);
                } else {
                    zero_timeout_idles = 0;
                }
                // Nothing arrived: this is the tick, not a stall.
                if progress() {
                    worker.cancel();
                }
                continue;
            }
            WorkerWait::Gone => {
                effects.stopped = true;
                // Truthful, not silent: a relay that was WAITING would
                // otherwise wait for output that can never arrive. A DRAIN was
                // waiting for nothing, and it runs on every frame -- announcing
                // a vanished worker there would print one line twenty times a
                // second.
                announce_worker_stopped(sink, until)?;
                break;
            }
        };
        stop = false;
        match event {
            // Frontend state, never transcript. `Ready` is ignored by the
            // shared bridge on purpose, so it is applied here.
            WorkerEvent::Ready(status) => {
                apply_status(state, &status);
                if until == Until::Applied {
                    applied_ready_seen = true;
                    // The report/failure is the caller-facing answer. If a
                    // producer ever sends `Ready` first, retain it as a safe
                    // fallback and keep waiting for that answer event.
                    if !applied_answer_seen {
                        effects.answer = Some(WorkerEvent::Ready(status));
                    }
                    stop = applied_answer_seen;
                } else if until == Until::Status {
                    // SetModel has no report of its own: its `Ready` IS the
                    // answer that proves the live switch moved.
                    effects.answer = Some(WorkerEvent::Ready(status));
                    stop = true;
                } else {
                    stop = false;
                }
            }
            WorkerEvent::Pane(data) => {
                // The shared slot the draw path reads, so the very next frame
                // shows the pane the worker just pushed (decision 167 D1).
                *pane.borrow_mut() = Some(data);
            }
            WorkerEvent::TurnFinished => stop = true,
            WorkerEvent::Stopped => {
                effects.stopped = true;
                effects.stopped_event = true;
                stop = true;
                announce_worker_stopped(sink, until)?;
            }
            // A tool approval suspends the turn mid-round: arm the
            // modal and yield to the dispatcher, which pumps the
            // modal keys, delivers the verdict straight into the
            // exchange, and re-enters for the remainder. Never an
            // answer (it must not satisfy a request command) and
            // never transcript (only the verdict line is recorded).
            // This diverts under every `Until` mode on purpose: a
            // frame Drain sweep can only meet a request the turn
            // relay already suspended on (the worker blocks until
            // its verdict), and arming the modal there too keeps the
            // main-loop key branch as the single verdict path.
            // Lines are re-sanitized at this boundary: the producer
            // sanitizes, and this second pass keeps that true for any
            // future producer that forgets.
            WorkerEvent::ApprovalRequest { id, lines, .. } => {
                let lines = lines
                    .iter()
                    .map(|line| sanitize_for_display(line))
                    .collect();
                {
                    let mut state = state.borrow_mut();
                    state.pending_approval =
                        Some(crate::tui::PendingApproval::ToolApproval {
                            modal: crate::tui::ApprovalModal::new(lines),
                            exchange_id: id,
                        });
                }
                effects.approval_pending = Some(id);
                stop = true;
            }
            // One answer, handed back UNRENDERED: the caller owns the wording.
            // A TURN and a DRAIN have no caller to hand one to, so their
            // answers fall through to the shared bridge below and are rendered:
            // an answer is never swallowed, whichever relay saw it.
            WorkerEvent::Report(text)
                if matches!(until, Until::Answer | Until::Status) =>
            {
                effects.answer = Some(WorkerEvent::Report(text));
                stop = true;
            }
            WorkerEvent::Report(text) if until == Until::Applied => {
                effects.answer = Some(WorkerEvent::Report(text));
                applied_answer_seen = true;
                stop = applied_ready_seen;
            }
            WorkerEvent::Failed(message)
                if matches!(until, Until::Answer | Until::Status) =>
            {
                effects.answer = Some(WorkerEvent::Failed(message));
                stop = true;
            }
            WorkerEvent::Failed(message) if until == Until::Applied => {
                effects.answer = Some(WorkerEvent::Failed(message));
                applied_answer_seen = true;
                stop = applied_ready_seen;
            }
            WorkerEvent::Models(models)
                if matches!(
                    until,
                    Until::Answer | Until::Applied | Until::Status
                ) =>
            {
                effects.answer = Some(WorkerEvent::Models(models));
                stop = true;
            }
            // Panels snapshots are frontend STATE like Ready/Models:
            // store them and answer the relay. Content lines arrive
            // sanitized from the worker; the draw path renders them
            // verbatim.
            WorkerEvent::Panels(boxed)
                if matches!(until, Until::Answer | Until::Status) =>
            {
                let panels = *boxed;
                state.borrow_mut().panels = Some(panels.clone());
                effects.answer = Some(WorkerEvent::Panels(Box::new(panels)));
                stop = true;
            }
            // Everything else is transcript. `Pane` never reaches this arm (it
            // is frontend state and is handled above); the bridge keeps its own
            // arm for its direct callers and its tests.
            other => {
                let mut scratch_pane = None;
                let mut sanitizer = worker.output_sanitizer.borrow_mut();
                let applied = crate::session_worker::apply_worker_event(
                    other,
                    &mut sanitizer,
                    sink,
                    reasoning,
                    &mut scratch_pane,
                );
                if let Err(error) = applied {
                    if error.kind() != std::io::ErrorKind::WriteZero {
                        return Err(InteractiveError::Io(error));
                    }
                    // `WriteZero` here is the TUI's own STREAM BOUND, not a
                    // dead terminal: the sink kept the accepted prefix and set
                    // a truncation status. A long answer must not end the
                    // session, so the turn continues with what fit.
                }
            }
        }
        // The per-event tick: what the keep-alive events used to drive.
        if progress() {
            worker.cancel();
        }
        if stop {
            break;
        }
    }
    Ok(effects)
}

/// Apply whatever the worker has ALREADY produced, without waiting (C3).
///
/// The relay waits for an answer; this is the loop's per-frame sweep, so the
/// loop is a genuine channel drain at every iteration rather than only while a
/// command is outstanding.
pub(super) fn drain_pending_worker(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
) -> Result<(), InteractiveError> {
    let mut progress = || false;
    let mut reasoning = |_text: &str| {};
    let effects = pump_worker(
        worker,
        sink,
        state,
        pane,
        &mut progress,
        &mut reasoning,
        Until::Drain,
    )?;
    if effects.stopped {
        return Err(InteractiveError::Worker(
            "worker stopped before the TUI finished".to_owned(),
        ));
    }
    Ok(())
}

/// Ask the worker one request command and return its answer (C2 step 3).
///
/// Six dispatcher arms are exactly this shape: send, relay, render whatever
/// comes back. The relay is where the tick, the pane cache and the sanitizer
/// boundary live, so the arms stay one line of intent each. Implemented over
/// [`ask_worker_effects`]; only the answer crosses this seam.
#[allow(clippy::too_many_arguments)]
pub(super) fn ask_worker(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
    command: WorkerCommand,
    until: Until,
) -> Result<Option<WorkerEvent>, InteractiveError> {
    Ok(ask_worker_effects(
        worker, sink, state, pane, progress, reasoning, command, until,
    )?
    .answer)
}

/// Announce a vanished or stopped worker on the sink. A DRAIN
/// relay runs on every frame, so announcing there would print one
/// line twenty times a second. Single wording via
/// [`WORKER_STOPPED_MESSAGE`].
fn announce_worker_stopped(
    sink: &mut crate::tui::TuiSink,
    until: Until,
) -> Result<(), InteractiveError> {
    if until != Until::Drain {
        let message =
            sanitize_for_display(&format!("{WORKER_STOPPED_MESSAGE}\n"));
        sink.write_all(message.as_bytes()).map_err(InteractiveError::Io)?;
    }
    Ok(())
}

/// Shared relay tail: a worker that stopped before it finished
/// is a loud error, never a silent partial relay.
fn finish_relay(
    effects: ReplyEffects,
) -> Result<ReplyEffects, InteractiveError> {
    if effects.stopped {
        return Err(InteractiveError::Worker(
            WORKER_STOPPED_MESSAGE.to_owned(),
        ));
    }
    Ok(effects)
}

/// Ask the worker one request command and return the full relay
/// effects (C2 step 3, Part B).
///
/// Same behavior as [`ask_worker`], but the caller also sees
/// `approval_pending`: a suspended tool-approval turn the caller
/// resumes after the modal verdict is delivered.
#[allow(clippy::too_many_arguments)]
pub(super) fn ask_worker_effects(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
    command: WorkerCommand,
    until: Until,
) -> Result<ReplyEffects, InteractiveError> {
    if !worker.send(command) {
        return Err(InteractiveError::Worker(
            "worker is unavailable; command was not applied".to_owned(),
        ));
    }
    finish_relay(pump_worker(
        worker, sink, state, pane, progress, reasoning, until,
    )?)
}

/// Resume a tool-approval-suspended turn after its verdict was
/// delivered (Part B).
///
/// No command is sent: the turn is still running on the worker,
/// unblocked by the verdict. Returns the relay effects; a fresh
/// `approval_pending` means the turn suspended again on a later
/// action, and the caller routes its modal the same way.
pub(super) fn relay_suspended_turn(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
) -> Result<ReplyEffects, InteractiveError> {
    finish_relay(pump_worker(
        worker,
        sink,
        state,
        pane,
        progress,
        reasoning,
        Until::TurnFinished,
    )?)
}

/// Bare `/model`: ask the worker for the provider's models and open the
/// switch picker over them (C2 step 3).
///
/// The fetch reads the endpoint AND the credential, so it belongs where they
/// live (decision 168 R2) and only the ids come back. The picker itself is the
/// same `ModelPicker` + sliding viewport the add-flow uses; a missing
/// provider/endpoint, or a failed or empty fetch, is reported truthfully with
/// the explicit-id hint instead of opening an empty picker.
#[allow(clippy::too_many_arguments)]
pub(super) fn open_model_picker_via_worker(
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    worker: &mut WorkerSource,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
) -> Result<(), InteractiveError> {
    let (provider, endpoint, credential_display, credential_resolved) = {
        let state = state.borrow();
        (
            state.provider.clone(),
            state.endpoint.clone(),
            state.credential_display.clone(),
            state.credential_resolved,
        )
    };
    let gate = model_listing_gate(
        provider.as_deref(),
        endpoint.as_deref(),
        credential_display.is_some(),
        credential_resolved,
    );
    if gate != ModelListingGate::Ready {
        let mut message = model_listing_diagnostic(gate).to_owned();
        if gate == ModelListingGate::NoProvider {
            message.push_str(
                "pass /model <id> to switch once a provider is set, or add one with /provider\n",
            );
        }
        let message = sanitize_for_display(&message);
        sink.write_all(message.as_bytes()).map_err(InteractiveError::Io)?;
        return Ok(());
    }
    let answer = ask_worker(
        worker,
        sink,
        state,
        pane,
        progress,
        reasoning,
        WorkerCommand::ModelsFetch,
        Until::Answer,
    )?;
    match answer {
        Some(WorkerEvent::Models(models)) if !models.is_empty() => {
            let opened = {
                let mut state = state.borrow_mut();
                crate::tui::open_model_switch_picker(&mut state, models);
                state.model_switch_picker.is_some()
            };
            if !opened {
                let msg = sanitize_for_display(
                    "model list unavailable: returned model ids were invalid — pass /model <id> to switch\n",
                );
                sink.write_all(msg.as_bytes())
                    .map_err(InteractiveError::Io)?;
            }
        }
        _ => {
            let msg = sanitize_for_display(
                "model list unavailable — pass /model <id> to switch\n",
            );
            sink.write_all(msg.as_bytes()).map_err(InteractiveError::Io)?;
        }
    }
    Ok(())
}

/// `/model <id>` and the model picker's Enter: persist on the frontend, apply
/// in the worker (decision 167 D3).
///
/// The frontend owns the profile write, so a refused switch changes neither
/// disk nor session, and the message the user reads is the persist's own --
/// one definition, both frontends. Only once it succeeded does the worker learn
/// about it, and the header it re-announces is the proof the live cells moved.
#[allow(clippy::too_many_arguments)]
pub(super) fn switch_model_via_worker(
    workspace_root: &Path,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    worker: &mut WorkerSource,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
    new_model: &str,
) -> Result<(), InteractiveError> {
    let provider = state.borrow().provider.clone();
    // Gate BEFORE the persist, exactly as the stdio frontend does. The worker
    // answers `SetModel` with one `Ready` or one `Failed`; persisting first
    // would leave the file claiming a model this composition cannot adopt, and
    // the next `/reload` would then read that as drift.
    if !state.borrow().live_model_switchable {
        let rendered = sanitize_for_display(
            "model switch unavailable for this provider mode; restart required\n",
        );
        sink.write_all(rendered.as_bytes()).map_err(InteractiveError::Io)?;
        return Ok(());
    }
    match persist_switched_model(
        workspace_root,
        provider.as_deref(),
        new_model,
    ) {
        Ok(message) => {
            // Apply it where the session lives. Do not claim success until the
            // worker acknowledges the live switch; a dead worker is a partial
            // state, not a successful model switch.
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::SetModel(new_model.to_owned()),
                Until::Status,
            )?;
            match answer {
                Some(WorkerEvent::Ready(_)) => {
                    let rendered = sanitize_for_display(&message);
                    sink.write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                }
                Some(WorkerEvent::Failed(error)) => {
                    let rendered = sanitize_for_display(&format!(
                        "model persisted but live switch failed: {error}\n"
                    ));
                    sink.write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                }
                _ => {
                    sink.write_all(
                        sanitize_for_display(
                            "model persisted but worker did not acknowledge the live switch\n",
                        )
                        .as_bytes(),
                    )
                    .map_err(InteractiveError::Io)?;
                }
            }
        }
        Err(reason) => {
            let rendered = sanitize_for_display(&reason);
            sink.write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
    }
    Ok(())
}

pub(super) fn drain_events<S, W>(
    application: &mut S,
    writer: &RefCell<W>,
    // S2 chunk 4b: called for EVERY drained event. A frontend that can
    // repaint uses the keep-alive ticks to draw, to collect what the user
    // typed while the model works, and to report an interrupt request;
    // returning `true` cancels the response.
    progress: &mut dyn FnMut() -> bool,
    // S3: thinking goes to its own sink. Stdio ignores it (byte-identical),
    // the TUI buffers it for the collapsed row.
    reasoning: &mut dyn FnMut(&str),
) -> Result<(), InteractiveError>
where
    S: crate::session_worker::EventSource,
    W: Write,
{
    // The writer is shared with the approval hook (which prompts while
    // the drain is blocked inside a tool call), so every use is one
    // brief borrow; nothing here holds it across `poll_event`.
    let mut sanitizer = TerminalSanitizer::new();
    while let Some(event) = application.poll_event() {
        if progress() {
            application.cancel();
        }
        match event {
            ToolLoopEvent::TextDelta { text } => {
                writer
                    .borrow_mut()
                    .write_all(sanitizer.push(&text).as_bytes())
                    .map_err(InteractiveError::Io)?;
            }
            ToolLoopEvent::ResponseCompleted => {
                // Drain any dangling escape that never terminated.
                writer
                    .borrow_mut()
                    .write_all(sanitizer.flush().as_bytes())
                    .map_err(InteractiveError::Io)?;
                writer
                    .borrow_mut()
                    .write_all(b"\n")
                    .map_err(InteractiveError::Io)?;
            }
            ToolLoopEvent::ResponseCancelled => {
                writer
                    .borrow_mut()
                    .write_all(sanitizer.flush().as_bytes())
                    .map_err(InteractiveError::Io)?;
                writer
                    .borrow_mut()
                    .write_all(b"Response cancelled.\n")
                    .map_err(InteractiveError::Io)?;
            }
            ToolLoopEvent::ResponseFailed { message } => {
                writer
                    .borrow_mut()
                    .write_all(sanitizer.flush().as_bytes())
                    .map_err(InteractiveError::Io)?;
                let safe = crate::sanitize::sanitize_for_display(&message);
                writer
                    .borrow_mut()
                    .write_all(format!("Response failed: {safe}\n").as_bytes())
                    .map_err(InteractiveError::Io)?;
            }
            ToolLoopEvent::ToolFailed { message, .. } => {
                writer
                    .borrow_mut()
                    .write_all(sanitizer.flush().as_bytes())
                    .map_err(InteractiveError::Io)?;
                let safe = crate::sanitize::sanitize_for_display(&message);
                writer
                    .borrow_mut()
                    .write_all(format!("Tool failed: {safe}\n").as_bytes())
                    .map_err(InteractiveError::Io)?;
            }
            ToolLoopEvent::ReasoningDelta { text } => reasoning(&text),
            // The keep-alive tick carries no output: the `progress`
            // callback above already gave the frontend its chance.
            ToolLoopEvent::ProviderPending
            | ToolLoopEvent::ToolCancelled { .. }
            | ToolLoopEvent::ResponseStarted
            | ToolLoopEvent::ToolStarted { .. }
            | ToolLoopEvent::ToolCompleted { .. }
            | ToolLoopEvent::ContextPressure { .. } => {}
        }
    }
    Ok(())
}

/// Spawn the worker the TUI drives (C2 step 3).
///
/// R1: the frontend keeps the workspace root for profile writes, so it hands
/// the worker OWNED paths -- the session is composed on the far thread
/// (decision 167) and cannot borrow this stack frame.
pub(super) fn spawn_tui_worker(
    workspace_root: &Path,
    config_path: Option<&Path>,
) -> crate::session_worker::WorkerHandle {
    crate::session_worker::spawn_worker(
        Some(workspace_root.to_path_buf()),
        config_path.map(Path::to_path_buf),
    )
}

/// Wait for the worker's first events, BEFORE the terminal is taken over
/// (C2 step 3).
///
/// The worker composes the session on its own thread, so its startup
/// diagnostics (`eprintln`: lock drift, a profile that was not applied, a
/// credential that did not resolve) must land on the normal screen, and a
/// composition failure must be reported exactly where the synchronous
/// composition used to report it. Both are what this wait buys.
///
/// The pane snapshot arrives BEFORE the header when the context subsystem is
/// on, so the first frame already has both.
/// It takes the source BY VALUE because every failure path here still has to
/// JOIN the worker: a frontend that returned an error while the worker was
/// still flushing would lose the recordings on exactly the startup that failed.
pub(super) fn await_worker_ready(
    mut worker: WorkerSource,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
) -> Result<WorkerSource, InteractiveError> {
    loop {
        match worker.recv() {
            Some(WorkerEvent::Pane(data)) => *pane.borrow_mut() = Some(data),
            Some(WorkerEvent::Ready(status)) => {
                apply_status(state, &status);
                // Host-owned startup notices land in the transcript once,
                // sanitized like every other terminal line. They ride the
                // header (never a standalone event: nothing may precede
                // the header, and a stray report could be misconsumed as
                // a command answer).
                {
                    let mut state = state.borrow_mut();
                    for notice in &status.startup_notices {
                        state.push_line(sanitize_for_display(notice));
                    }
                }
                return Ok(worker);
            }
            Some(WorkerEvent::Failed(message)) => {
                // The composition failed: the worker is done, so stop it (the
                // guard would too) and report a bounded, redacted diagnostic.
                let message = safe_profile_diagnostic(&message);
                let detail = match worker
                    .shutdown_bounded(std::time::Duration::from_secs(2))
                {
                    Ok(()) => message,
                    Err(reason) => {
                        format!("{message}; worker shutdown failed: {reason}")
                    }
                };
                return Err(InteractiveError::Worker(detail));
            }
            Some(WorkerEvent::Stopped) | None => {
                let detail = match worker
                    .shutdown_bounded(std::time::Duration::from_secs(2))
                {
                    Ok(()) => {
                        "the worker stopped before it composed a session"
                            .to_owned()
                    }
                    Err(reason) => format!(
                        "the worker stopped before it composed a session; worker shutdown failed: {reason}"
                    ),
                };
                return Err(InteractiveError::Worker(detail));
            }
            // Nothing else can precede the first command (the worker sends the
            // pane, then the header, then blocks); if it ever does, say so
            // instead of dropping it silently.
            Some(other) => {
                let kind = match other {
                    WorkerEvent::Session(_) => "a session event",
                    WorkerEvent::Pane(_) => "a pane event",
                    WorkerEvent::TurnFinished => "turn completion",
                    WorkerEvent::Stopped => "a stop event",
                    WorkerEvent::Report(_) => "a report",
                    WorkerEvent::Panels(_) => "a panels snapshot",
                    WorkerEvent::Failed(_) => "a failure",
                    WorkerEvent::Models(_) => "a model list",
                    WorkerEvent::Ready(_) => "a header",
                    WorkerEvent::ApprovalRequest { .. } => {
                        "an approval request"
                    }
                };
                let detail = match worker
                    .shutdown_bounded(std::time::Duration::from_secs(2))
                {
                    Ok(()) => format!(
                        "the worker announced {kind} before its header"
                    ),
                    Err(reason) => format!(
                        "the worker announced {kind} before its header; worker shutdown failed: {reason}"
                    ),
                };
                return Err(InteractiveError::Worker(detail));
            }
        }
    }
}
