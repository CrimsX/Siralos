//! C2 (ticket 130, decision 167): the message contract between the UI thread
//! and the worker thread that owns the session.
//!
//! The session **cannot cross threads**: the providers carry their live cells
//! as \`Rc<RefCell<..>>\`, so \`SiralosApplication\` is \`!Send\`. The worker
//! therefore CONSTRUCTS the session and the two sides speak only in plain data.
//! Every type here must be \`Send\`; the assertions at the bottom of this file
//! are the compile-time proof, so the wiring cannot accidentally depend on a
//! non-transferable value.
//!
//! This module owns the bridge contract and its bounded worker construction.
//! C2's wiring (the worker loop, the cancel flag, the single replay flush) is
//! written against it.

use crate::sanitize::{TerminalSanitizer, sanitize_for_display};
use siralos_core::tool::ToolLoopEvent;

/// Bounded cooperative drain budget used before a stopping worker flushes.
const SHUTDOWN_QUIESCE_TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(250);
/// Upper bound on events discarded while quiescing a stopped response.
const MAX_SHUTDOWN_DRAIN_EVENTS: usize = 256;

/// Project a worker shutdown detail onto the ONE reporting boundary the
/// frontends use: sanitized, single-line, bounded, and with credential-shaped
/// or workspace-path content replaced instead of echoed.
///
/// Shutdown text is built from provider and transport errors, so it is
/// untrusted data exactly like any other worker report. A lifecycle failure
/// must still be visible; a secret inside it must not become visible with it.
pub(crate) fn safe_shutdown_detail(value: &str) -> String {
    let sanitized = sanitize_for_display(value);
    let lower = sanitized.to_ascii_lowercase();
    let sensitive =
        ["://", "key:", "sk-", "akia", "secret", "token", "bearer "]
            .iter()
            .any(|marker| lower.contains(marker))
            || sanitized.contains('\\');
    if sensitive {
        return "worker shutdown detail hidden".to_owned();
    }
    let single_line: String = sanitized
        .chars()
        .map(|character| {
            if character == '\n' || character == '\r' || character.is_control()
            {
                ' '
            } else {
                character
            }
        })
        .collect();
    siralos_core::language::truncate_utf8_bytes(&single_line, 512)
}

/// A command from the UI thread to the worker.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerCommand {
    /// Run one prompt turn.
    Prompt(String),
    /// Cancel the active turn. The worker ALSO watches an external flag, so a
    /// cancellation request can be recorded while a provider read is blocked;
    /// the transport seam must make that read abortable to stop the turn.
    Cancel,
    /// Display-only: the projection behind \`/context\`.
    ContextReport,
    /// Display-only: the tool projection behind \`/tools\`.
    ToolsReport,
    /// \`/model <id>\`: the UI has already persisted the profile, so the worker
    /// only applies it live and reports the outcome (decision 167 D3 keeps
    /// persist-before-live by ordering, not by sharing state).
    SetModel(String),
    /// Apply a reloaded composition the same way.
    Reload,
    /// List the provider's models. The fetch needs the endpoint and the
    /// credential, so it happens where they live (decision 168 R2): a secret
    /// never crosses to the frontend just so the frontend can fetch.
    ModelsFetch,
    /// Install a plugin folder into the session's domain registry. These three
    /// mutate the registry and activate hosts, so they belong with the session
    /// (decision 168 R4): the frontend cannot hold a registry it does not own.
    DomainsAdd(String),
    /// Enable an installed plugin.
    DomainsEnable(String),
    /// Activate an installed plugin through the profile's narrowing gate.
    DomainsActivate(String),
    /// Stop: flush the recordings exactly once and exit the loop.
    Shutdown,
}

/// An event from the worker to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerEvent {
    /// One session event, in order.
    Session(ToolLoopEvent),
    /// A detached pane snapshot (decision 167 D1: advisory, may lag a frame,
    /// never blocks the turn).
    Pane(crate::tui::ContextPaneData),
    /// A display report answering one of the request commands.
    Report(String),
    /// A command failed. The text is for a frontend to render truthfully;
    /// failing a command must never look like success.
    Failed(String),
    /// The turn is over: nothing more arrives until the next command.
    TurnFinished,
    /// The provider's model ids, answering `ModelsFetch`.
    Models(Vec<String>),
    /// What the frontend header shows (C2 step 3). Sent once when the loop
    /// starts and again whenever the composition moves under it (`SetModel`,
    /// `Reload`), because only the session can derive it.
    Ready(SessionStatus),
    /// The worker is exiting. A replay-persistence failure is also sent as
    /// `Failed`; the typed join result is authoritative for shutdown callers.
    Stopped,
}

/// The header the frontend shows: the composed status segment plus the two
/// names it is built from.
#[derive(Clone, PartialEq)]
pub struct SessionStatus {
    /// Publicly constructed snapshots are debug-redacted; the normal producer
    /// also applies credential-aware projection before sending them.
    /// Endpoint fields are authority-only.
    /// The composed status segment (provider, model, context usage).
    pub status: String,
    /// Applied provider name, if any.
    pub provider: Option<String>,
    /// Model to display -- the display name when the profile declares one.
    pub model: Option<String>,
    /// Endpoint the session was composed with (decision 168 R3: display only).
    pub endpoint: Option<String>,
    /// Protocol string the provider was built with.
    pub protocol: String,
    /// The credential in its display form, ALREADY REDACTED. The raw value
    /// never leaves the worker (decision 168 R2): a frontend that received it
    /// could put a secret in `TuiState`, the render path and any log.
    pub credential_display: Option<String>,
    /// Whether the declared credential RESOLVED. The `/models` arm decides on
    /// this today, so the frontend needs the answer and not the secret;
    /// `false` also covers "no credential declared".
    pub credential_resolved: bool,
    /// Whether `/model <id>` can move the live model for THIS composition.
    ///
    /// The frontend owns the profile write, so it has to know whether the
    /// switch is possible BEFORE it persists: persisting a switch the worker
    /// cannot apply would leave the file claiming a model the session never
    /// adopted, and the next `/reload` would then look like drift.
    pub live_model_switchable: bool,
    /// The context-usage suffix the status line carries (` | ctx N/4096`), empty
    /// when the context subsystem is off. It crosses so a frontend can
    /// re-render a TRANSIENT status -- the add-flow's "fetching models..." --
    /// with the same suffix instead of dropping a readout it cannot compute.
    pub context_suffix: String,
}

impl std::fmt::Debug for SessionStatus {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("SessionStatus")
            .field("status", &"[PROJECTED]")
            .field(
                "provider",
                &self.provider.as_deref().map(|_| "[CONFIGURED]"),
            )
            .field("model", &self.model.as_deref().map(|_| "[CONFIGURED]"))
            .field(
                "endpoint",
                &self.endpoint.as_deref().map(|_| "[PROJECTED]"),
            )
            .field("protocol", &"[PROJECTED]")
            .field("credential_display", &"[REDACTED]")
            .field("credential_resolved", &self.credential_resolved)
            .field("live_model_switchable", &self.live_model_switchable)
            .field("context_suffix", &"[PROJECTED]")
            .finish()
    }
}

/// The external cancel flag (decision 167): the UI sets it, and the worker
/// polls it between events. A provider call that is already blocked in an
/// external read is cooperative and must be made abortable by its transport
/// seam.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelFlag {
    /// A fresh, unset flag.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the worker to cancel the active turn.
    pub fn request(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether a cancel has been requested.
    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Clear the request (the worker resets it when a new turn starts).
    pub fn clear(&self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// The shared atomic used by a bounded provider probe.
    #[must_use]
    pub fn flag(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::clone(&self.0)
    }
}

// The compile-time proof that the contract can cross threads. If a future edit
// puts an \`Rc\` (or any other \`!Send\` value) in a message, this stops compiling.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<WorkerCommand>();
    assert_send::<WorkerEvent>();
    assert_send::<WorkerStopOutcome>();
    assert_send::<CancelFlag>();
};

/// The result of the worker's single replay-flush operation.
///
/// `Legacy` is the compatibility result for an implementation that still only
/// provides the old void [`WorkerSession::flush`] method. A real composition
/// overrides the typed seam and reports either no recorder or the exact
/// persisted recording count. It is deliberately not represented as a
/// boolean: a successful flush and a session with nothing to flush are
/// different evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushOutcome {
    /// The legacy void flush ran; no count is available.
    Legacy,
    /// This session has no retaining recorder, so no store was expected.
    NoRecorder,
    /// The bounded replay store was written with this many recordings.
    Persisted {
        /// Number of validated recordings written to the store.
        recordings: usize,
    },
}

/// A typed replay persistence failure.
///
/// The detail is required to be report-safe by implementors: it may name a
/// bounded error class, but must not contain a credential, raw provider body,
/// or an absolute path. The worker sanitizes the rendered event again at the
/// frontend boundary, while the typed variant keeps the failure class visible
/// to the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlushError {
    /// The replay store could not be persisted.
    Persistence(String),
}

impl std::fmt::Display for FlushError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Persistence(message) => {
                write!(formatter, "replay persistence failed: {message}")
            }
        }
    }
}

impl std::error::Error for FlushError {}

/// What a worker actually did when it stopped.
///
/// This is returned by the typed shutdown seam instead of an empty success.
/// A flush failure still means the thread has stopped, but it is not a
/// successful stop and must never be folded into `Flushed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerStopOutcome {
    /// There was no live worker to join (for example, a scripted handle).
    AlreadyStopped,
    /// The worker stopped after a successful flush.
    Flushed(FlushOutcome),
    /// The worker stopped, but its replay flush failed.
    FlushFailed(FlushError),
    /// The legacy void flush ran without typed persistence evidence.
    FlushEvidenceMissing,
    /// The worker stopped before quiescing; the single flush attempt is included
    /// so recorded evidence is never silently discarded.
    QuiesceFailed {
        /// Bounded diagnostic explaining why the response remained active.
        detail: String,
        /// Result of the one persistence attempt made after the failed drain.
        flush: Result<FlushOutcome, FlushError>,
    },
}

/// Why a bounded worker join did not produce a stop outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerShutdownError {
    /// The worker did not finish before the caller's deadline.
    TimedOut,
    /// The worker thread panicked while stopping.
    ThreadPanicked,
}

impl std::fmt::Display for WorkerShutdownError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("worker shutdown timed out"),
            Self::ThreadPanicked => {
                formatter.write_str("worker thread panicked during shutdown")
            }
        }
    }
}

impl std::error::Error for WorkerShutdownError {}

/// What the worker needs from a session (C2).
///
/// Implemented for the composed session in the wiring step; a test double
/// implements it here, so the loop is proven before any thread touches a real
/// session. A method is fallible ONLY where the real session is: a prompt can
/// be refused (already responding), a model switch can be refused, the rest
/// cannot fail at all -- with ONE deliberate exception: `reload` refuses until the reload path moves behind this boundary, and the command stays in the trait so the wiring has the shape it needs.
pub trait WorkerSession {
    /// Start one prompt turn.
    fn send_prompt(&mut self, prompt: &str) -> Result<(), String>;
    /// The next session event, or `None` when the turn is over.
    fn poll_event(&mut self) -> Option<ToolLoopEvent>;
    /// Whether a turn is currently running.
    fn is_responding(&self) -> bool;
    /// A detached pane snapshot for the frontend (decision 167 D1).
    fn pane(&self) -> Option<crate::tui::ContextPaneData>;
    /// The projection report behind `/context`.
    fn context_report(&self) -> String;
    /// The tool projection report behind `/tools`.
    fn tools_report(&self) -> String;
    /// Apply a model switch the UI has already persisted (decision 167 D3).
    fn set_model(&mut self, model: &str) -> Result<(), String>;
    /// Re-apply the reloaded composition (decision 167 D3) and return the
    /// report the frontend shows. The report belongs to the SESSION: a loop
    /// that invented its own could announce a reload that never happened.
    fn reload(&mut self) -> Result<String, String>;
    /// The turn's events are done (C2 step 3). What follows a finished turn
    /// runs HERE, with the session, because it reads state only the owner can
    /// see -- the context demand loop reads the session's own history.
    fn turn_settled(&mut self);
    /// The provider's model ids (decision 168 R2). Fallible exactly where a
    /// fetch is: unconfigured, unreachable, or a non-success status.
    fn fetch_models(
        &mut self,
        cancellation: &CancelFlag,
    ) -> Result<Vec<String>, String>;
    /// Install a plugin folder, returning the report the frontend shows.
    fn domains_add(&mut self, folder: &str) -> Result<String, String>;
    /// Enable an installed plugin.
    fn domains_enable(&mut self, id: &str) -> Result<String, String>;
    /// Activate an installed plugin through the profile's narrowing gate.
    fn domains_activate(&mut self, id: &str) -> Result<String, String>;
    /// The header the frontend shows (C2 step 3). The status segment is derived
    /// from the composition and its context metrics, so the frontend cannot
    /// build it once the session lives here.
    fn status(&self) -> SessionStatus;
    /// Host cancellation authority.
    fn cancel(&mut self);
    /// Legacy void flush, called exactly once on shutdown. New implementations
    /// should override [`WorkerSession::flush_result`] so persistence evidence
    /// is typed rather than inferred from a log line.
    fn flush(&mut self);
    /// Typed replay-flush seam. The default keeps existing synchronous and test
    /// implementations source-compatible while making the compatibility case
    /// explicit. A composed session that owns a replay recorder must override
    /// this method and return [`FlushOutcome::NoRecorder`] or
    /// [`FlushOutcome::Persisted`], or return [`FlushError::Persistence`].
    fn flush_result(&mut self) -> Result<FlushOutcome, FlushError> {
        self.flush();
        Ok(FlushOutcome::Legacy)
    }
    /// Turn on the keep-alive ticks (C2 step 3). Only a frontend that repaints
    /// while it waits wants them: the worker's session belongs to the TUI, so
    /// the WORKER turns them on, and stdio's session must not get them.
    fn enable_progress_ticks(&mut self);
}

/// The narrow source the shared drain reads (C2 step 3b).
///
/// \`drain_events\` needs exactly these two calls, so the frontend can point it
/// at a worker-owned session instead of a locally composed one. The seam is
/// what lets the switch stay atomic (decision 167): the real application
/// implements it by pure delegation today, so stdio and TUI drain byte-for-byte
/// what they drained before, and the wiring step adds the worker-backed
/// implementation without touching the drain.
pub trait EventSource {
    /// The next session event, or \`None\` when nothing is pending.
    fn poll_event(&mut self) -> Option<ToolLoopEvent>;
    /// Ask the session to stop the current turn.
    fn cancel(&mut self);
}

/// Run the worker loop until `Shutdown` (C2).
///
/// Synchronous and single-threaded on its own thread: receive a command, act,
/// drain the session into worker events, repeat. The cancel flag is checked
/// between commands AND between drained events. A provider already blocked in
/// an external read remains a transport-seam responsibility; this module does
/// not claim to interrupt that call.
///
/// `flush_result` is called exactly once, on shutdown, which is decision 78's
/// single-owner rule made mechanical. The legacy wrapper discards the typed
/// result for source compatibility; lifecycle owners should use
/// [`run_worker_loop_typed`] or [`WorkerHandle::shutdown_typed`].
pub fn run_worker_loop<S: WorkerSession>(
    commands: &std::sync::mpsc::Receiver<WorkerCommand>,
    events: &std::sync::mpsc::SyncSender<WorkerEvent>,
    cancel: &CancelFlag,
    session: &mut S,
) {
    let _ = run_worker_loop_typed(commands, events, cancel, session);
}

/// Run the worker loop and expose its typed stop evidence to synchronous
/// callers. The legacy `run_worker_loop` wrapper remains for source
/// compatibility, while lifecycle owners can no longer accidentally discard a
/// flush or quiesce failure.
pub fn run_worker_loop_typed<S: WorkerSession>(
    commands: &std::sync::mpsc::Receiver<WorkerCommand>,
    events: &std::sync::mpsc::SyncSender<WorkerEvent>,
    cancel: &CancelFlag,
    session: &mut S,
) -> WorkerStopOutcome {
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    run_worker_loop_with_stop(commands, events, cancel, &stop, session)
}

/// Ceiling on how long one non-terminal event delivery may retry while the
/// frontend's bounded queue stays full. Past it the worker stops instead of
/// waiting for a consumer that is not draining.
const EVENT_DELIVERY_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);

struct StopAwareEvents<'a> {
    inner: &'a std::sync::mpsc::SyncSender<WorkerEvent>,
    stop: &'a std::sync::atomic::AtomicBool,
}

impl StopAwareEvents<'_> {
    fn request_stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn send(&self, event: WorkerEvent) -> Result<(), ()> {
        let mut event = Some(event);
        // A frontend that stops draining but keeps the channel open must not
        // park a credential-holding worker in a retry loop forever: delivery
        // is bounded, and an expired deadline is a stop, not silence.
        let deadline =
            std::time::Instant::now().checked_add(EVENT_DELIVERY_TIMEOUT);
        loop {
            if self.stop.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(());
            }
            match self.inner.try_send(event.take().expect("event sent once")) {
                Ok(()) => return Ok(()),
                Err(std::sync::mpsc::TrySendError::Full(value)) => {
                    event = Some(value);
                    let expired = match deadline {
                        Some(limit) => std::time::Instant::now() >= limit,
                        None => true,
                    };
                    if expired {
                        self.request_stop();
                        return Err(());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    // A frontend that disappeared must not leave a
                    // credential-holding worker asleep on a command queue.
                    self.request_stop();
                    return Err(());
                }
            }
        }
    }

    /// Terminal evidence gets one short, bounded delivery attempt even after
    /// the stop bit is set. The typed join result remains authoritative if a
    /// full event queue cannot accept it; this method must never wait forever
    /// while the owner is trying to restore the terminal.
    fn send_terminal(&self, event: WorkerEvent) {
        let mut event = Some(event);
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(100));
        loop {
            match self.inner.try_send(event.take().expect("event sent once")) {
                Ok(()) => return,
                Err(std::sync::mpsc::TrySendError::Full(value)) => {
                    event = Some(value);
                    let terminal_deadline_reached = match deadline {
                        Some(limit) => std::time::Instant::now() >= limit,
                        None => true,
                    };
                    if terminal_deadline_reached {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    return;
                }
            }
        }
    }
}

fn drain_until_settled<S: WorkerSession>(
    session: &mut S,
    timeout: std::time::Duration,
) -> bool {
    let Some(deadline) = std::time::Instant::now().checked_add(timeout) else {
        return false;
    };
    let mut drained = 0usize;
    while session.is_responding() {
        if drained >= MAX_SHUTDOWN_DRAIN_EVENTS
            || std::time::Instant::now() >= deadline
        {
            return false;
        }
        match session.poll_event() {
            Some(_) => drained += 1,
            None => {
                if !session.is_responding() {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }
    true
}

fn flush_and_stop<S: WorkerSession>(
    session: &mut S,
    events: &StopAwareEvents<'_>,
) -> WorkerStopOutcome {
    // Every exit path reaches the same cancellation point, including a
    // command-channel or event-channel disconnect. Do not let a provider keep
    // running while its owning session is being flushed.
    let was_responding = session.is_responding();
    if was_responding {
        session.cancel();
    }
    if was_responding
        && !drain_until_settled(session, SHUTDOWN_QUIESCE_TIMEOUT)
    {
        // Repeat the cancellation request at the cleanup boundary before
        // taking the persistence snapshot; some transports observe it there.
        session.cancel();
        let detail = format!(
            "cancelled response did not quiesce within {} ms",
            SHUTDOWN_QUIESCE_TIMEOUT.as_millis()
        );
        // Persistence remains a single-owner responsibility even when cleanup
        // could not prove quiescence. Make one bounded attempt and retain both
        // failure classes in the typed outcome instead of dropping recordings.
        let flush = session.flush_result();
        let message = match &flush {
            Ok(FlushOutcome::Legacy) => format!(
                "{detail}; replay flush outcome is unavailable; typed persistence evidence is missing"
            ),
            Ok(_) => detail.clone(),
            Err(error) => format!("{detail}; replay flush failed: {error}"),
        };
        let safe = crate::sanitize::sanitize_for_display(&message);
        events.send_terminal(WorkerEvent::Failed(safe.clone()));
        events.send_terminal(WorkerEvent::Stopped);
        return WorkerStopOutcome::QuiesceFailed { detail: safe, flush };
    }
    if was_responding {
        // The demand loop runs only after the terminal sentinel has restored
        // the application state. Calling it earlier snapshots active response
        // state and can make the next prompt fail as AlreadyResponding.
        session.turn_settled();
    }
    let outcome = match session.flush_result() {
        Ok(FlushOutcome::Legacy) => WorkerStopOutcome::FlushEvidenceMissing,
        Ok(outcome) => WorkerStopOutcome::Flushed(outcome),
        Err(error) => WorkerStopOutcome::FlushFailed(error),
    };
    match &outcome {
        WorkerStopOutcome::FlushFailed(error) => {
            let message = format!("replay flush failed: {error}");
            let safe = crate::sanitize::sanitize_for_display(&message);
            events.send_terminal(WorkerEvent::Failed(safe));
        }
        WorkerStopOutcome::FlushEvidenceMissing => {
            events.send_terminal(WorkerEvent::Failed(
                "replay flush outcome is unavailable; typed persistence evidence is missing"
                    .to_owned(),
            ));
        }
        _ => {}
    }
    events.send_terminal(WorkerEvent::Stopped);
    outcome
}

fn run_worker_loop_with_stop<S: WorkerSession>(
    commands: &std::sync::mpsc::Receiver<WorkerCommand>,
    events: &std::sync::mpsc::SyncSender<WorkerEvent>,
    cancel: &CancelFlag,
    stop: &std::sync::atomic::AtomicBool,
    session: &mut S,
) -> WorkerStopOutcome {
    let events = StopAwareEvents { inner: events, stop };
    // The startup pane snapshot goes FIRST. The frontend used to BUILD it
    // before its loop and cannot any more -- the metrics and the history live
    // here (decision 167 D1) -- and a frontend that waits for the header must
    // already hold the pane it will draw with that header. Sending it after the
    // header would make the first frame a race.
    if let Some(pane) = session.pane() {
        if events.send(WorkerEvent::Pane(pane)).is_err() {
            return flush_and_stop(session, &events);
        }
    }
    // The frontend cannot derive its header any more: it arrives before any
    // command, and again whenever a command moves the composition under it.
    if events.send(WorkerEvent::Ready(session.status())).is_err() {
        return flush_and_stop(session, &events);
    }
    loop {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let command = match commands
            .recv_timeout(std::time::Duration::from_millis(10))
        {
            Ok(command) => command,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if stop.load(std::sync::atomic::Ordering::SeqCst)
            && !matches!(command, WorkerCommand::Shutdown)
        {
            break;
        }
        match command {
            WorkerCommand::Prompt(prompt) => {
                if cancel.is_requested() {
                    session.cancel();
                    let _ = events.send(WorkerEvent::Failed(
                        "prompt cancelled before it started".to_owned(),
                    ));
                    let _ = events.send(WorkerEvent::TurnFinished);
                    cancel.clear();
                    continue;
                }
                match session.send_prompt(&prompt) {
                    Ok(()) => {
                        while let Some(event) = session.poll_event() {
                            if events
                                .send(WorkerEvent::Session(event))
                                .is_err()
                            {
                                break;
                            }
                            if cancel.is_requested() {
                                session.cancel();
                            }
                            if let Some(pane) = session.pane() {
                                let _ = events.send(WorkerEvent::Pane(pane));
                            }
                        }
                        // If the event relay was stopped, the response may
                        // still be active. Leave settlement to the common
                        // shutdown path so it can cancel, drain, and settle
                        // before the replay recorder is flushed.
                        if !session.is_responding() {
                            session.turn_settled();
                        }
                        // The settled pane: the demand loop has just run, so
                        // this is the first snapshot that can show its effect.
                        if let Some(pane) = session.pane() {
                            let _ = events.send(WorkerEvent::Pane(pane));
                        }
                        let _ = events.send(WorkerEvent::TurnFinished);
                        cancel.clear();
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                        let _ = events.send(WorkerEvent::TurnFinished);
                        cancel.clear();
                    }
                }
            }
            WorkerCommand::Cancel => {
                session.cancel();
                // The command is observed only between turns, so the shared
                // request is fully consumed here. Do not let an idle Ctrl+C
                // make the next prompt look pre-cancelled.
                cancel.clear();
            }
            WorkerCommand::ContextReport => {
                let report = session.context_report();
                let _ = events.send(WorkerEvent::Report(report));
            }
            WorkerCommand::ToolsReport => {
                let report = session.tools_report();
                let _ = events.send(WorkerEvent::Report(report));
            }
            WorkerCommand::SetModel(model) => {
                match session.set_model(&model) {
                    // No report of its own: the frontend owns the profile
                    // write and already reported its outcome (decision 167
                    // D3), so a line here would say it twice. The header IS
                    // re-announced, because the composition moved under it.
                    Ok(()) => {
                        let _ =
                            events.send(WorkerEvent::Ready(session.status()));
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                    }
                }
            }
            WorkerCommand::ModelsFetch => {
                if cancel.is_requested() {
                    let _ = events.send(WorkerEvent::Failed(
                        "model listing cancelled before it started".to_owned(),
                    ));
                    cancel.clear();
                    continue;
                }
                match session.fetch_models(cancel) {
                    Ok(models) => {
                        let _ = events.send(WorkerEvent::Models(models));
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                    }
                }
                // A probe may have observed cancellation or completed
                // normally; do not carry that bit into the next prompt.
                cancel.clear();
            }
            WorkerCommand::DomainsAdd(value) => {
                match session.domains_add(&value) {
                    Ok(report) => {
                        let _ = events.send(WorkerEvent::Report(report));
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                    }
                }
            }
            WorkerCommand::DomainsEnable(value) => {
                match session.domains_enable(&value) {
                    Ok(report) => {
                        let _ = events.send(WorkerEvent::Report(report));
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                    }
                }
            }
            WorkerCommand::DomainsActivate(value) => {
                match session.domains_activate(&value) {
                    Ok(report) => {
                        let _ = events.send(WorkerEvent::Report(report));
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                    }
                }
            }
            WorkerCommand::Reload => match session.reload() {
                Ok(report) => {
                    let _ = events.send(WorkerEvent::Report(report));
                    let _ = events.send(WorkerEvent::Ready(session.status()));
                }
                Err(message) => {
                    let _ = events.send(WorkerEvent::Failed(message));
                    // Reload may revoke authority before returning an error;
                    // refresh the header so stale route/credential state is
                    // never left visible in the frontend.
                    let _ = events.send(WorkerEvent::Ready(session.status()));
                }
            },
            WorkerCommand::Shutdown => {
                return flush_and_stop(session, &events);
            }
        }
    }
    // The command channel closed without a Shutdown: flush once anyway, so a
    // vanished UI cannot lose the recordings silently.
    flush_and_stop(session, &events)
}

/// One bounded wait on the worker (C2 step 3).
///
/// A frontend cannot use a blocking receive for a turn: it must keep its own
/// clock while the worker is silent, because that clock is what keeps the
/// reveal, the pulse and the key handling moving. The three outcomes are the
/// whole protocol of a wait -- an event, a tick, or a worker that is gone.
#[derive(Debug)]
pub enum WorkerWait {
    /// One event arrived.
    Event(WorkerEvent),
    /// Nothing arrived within the timeout: this is the frontend's tick, not an
    /// error.
    Idle,
    /// The worker is gone: the channel closed, so nothing more will arrive.
    Gone,
}

/// The UI's handle on a running worker (C2 step 2).
///
/// The compatibility `shutdown` method remains available, but the typed
/// [`WorkerHandle::shutdown_typed`] method is the authoritative lifecycle
/// result. A timeout is never converted into an empty success, and dropping a
/// live handle emits a bounded diagnostic before the join handle is released.
pub struct WorkerHandle {
    commands: std::sync::mpsc::SyncSender<WorkerCommand>,
    events: std::sync::mpsc::Receiver<WorkerEvent>,
    cancel: CancelFlag,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    join: Option<std::thread::JoinHandle<WorkerStopOutcome>>,
}

impl WorkerHandle {
    /// Send one command (`false` when the worker is already gone).
    pub fn send(&self, command: WorkerCommand) -> bool {
        const MAX_COMMAND_STRING_BYTES: usize = 64 * 1024;
        let string_len = match &command {
            WorkerCommand::Prompt(value)
            | WorkerCommand::SetModel(value)
            | WorkerCommand::DomainsAdd(value)
            | WorkerCommand::DomainsEnable(value)
            | WorkerCommand::DomainsActivate(value) => Some(value.len()),
            WorkerCommand::Cancel
            | WorkerCommand::ContextReport
            | WorkerCommand::ToolsReport
            | WorkerCommand::Reload
            | WorkerCommand::ModelsFetch
            | WorkerCommand::Shutdown => None,
        };
        if string_len.is_some_and(|length| length > MAX_COMMAND_STRING_BYTES) {
            return false;
        }
        self.commands.try_send(command).is_ok()
    }

    /// The next worker event, or `None` when the worker has stopped.
    pub fn recv(&self) -> Option<WorkerEvent> {
        self.events.recv().ok()
    }

    /// Wait up to `timeout` for the next event (C2 step 3).
    pub fn wait(&self, timeout: std::time::Duration) -> WorkerWait {
        match self.events.recv_timeout(timeout) {
            Ok(event) => WorkerWait::Event(event),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                WorkerWait::Idle
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                WorkerWait::Gone
            }
        }
    }

    /// Receive one already-produced event without blocking.
    pub fn try_recv(&self) -> Option<WorkerEvent> {
        self.events.try_recv().ok()
    }

    /// Everything the worker has already produced, without blocking.
    pub fn try_recv_all(&self) -> Vec<WorkerEvent> {
        const MAX_BATCH: usize = 1024;
        let mut events = Vec::new();
        while events.len() < MAX_BATCH {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }
        events
    }

    /// The shared cancel flag the worker polls between events.
    #[must_use]
    pub fn cancel_flag(&self) -> &CancelFlag {
        &self.cancel
    }

    /// Compatibility wrapper for call sites that historically returned `()`.
    /// It is deliberately noisy on failure: the typed method below is the path
    /// that should be used for lifecycle decisions.
    pub fn shutdown(&mut self) {
        if let Err(error) =
            self.shutdown_bounded(std::time::Duration::from_secs(2))
        {
            eprintln!("siralos: worker shutdown: {error}");
        }
    }

    /// Cancel first, request `Shutdown`, and return the worker's typed stop
    /// result after a bounded join. A timeout leaves the join handle installed
    /// so the owner can retry; it is never represented as success and the
    /// handle's `Drop` reports a final diagnostic if it is later released.
    pub fn shutdown_typed(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<WorkerStopOutcome, WorkerShutdownError> {
        let deadline = std::time::Instant::now().checked_add(timeout);
        self.cancel.request();
        // Signal the command before the stop bit. An idle worker can then
        // consume the explicit command and emit its terminal evidence; a
        // worker in a turn still observes the cancellation flag first.
        let _ = self.commands.try_send(WorkerCommand::Shutdown);
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let Some(join) = self.join.take() else {
            return Ok(WorkerStopOutcome::AlreadyStopped);
        };
        while !join.is_finished() {
            let deadline_reached = match deadline {
                Some(limit) => std::time::Instant::now() >= limit,
                // A duration that cannot be represented by the platform clock
                // cannot provide a bounded join; fail closed instead of
                // detaching the worker after an unbounded wait.
                None => true,
            };
            if deadline_reached {
                self.join = Some(join);
                return Err(WorkerShutdownError::TimedOut);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        join.join().map_err(|_| WorkerShutdownError::ThreadPanicked)
    }

    /// Compatibility error-shaped shutdown used by the existing frontends. It
    /// now also maps a typed replay flush failure to `Err`, instead of joining
    /// the thread and calling that empty success.
    pub fn shutdown_bounded(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let outcome = match self.shutdown_typed(timeout) {
            Ok(WorkerStopOutcome::AlreadyStopped) => Ok(()),
            Ok(WorkerStopOutcome::FlushEvidenceMissing) => Err(
                "replay flush outcome is unavailable; typed persistence evidence is missing"
                    .to_owned(),
            ),
            Ok(WorkerStopOutcome::Flushed(_)) => Ok(()),
            Ok(WorkerStopOutcome::FlushFailed(error)) => {
                Err(format!("replay flush failed: {error}"))
            }
            Ok(WorkerStopOutcome::QuiesceFailed { detail, flush }) => {
                let flush_detail = match flush {
                    Ok(FlushOutcome::Legacy) => "; replay flush outcome is unavailable"
                        .to_owned(),
                    Ok(_) => String::new(),
                    Err(error) => format!("; replay flush failed: {error}"),
                };
                Err(format!(
                    "worker response did not quiesce: {detail}{flush_detail}"
                ))
            }
            Err(WorkerShutdownError::TimedOut) => {
                Err("worker shutdown timed out; recordings may be incomplete"
                    .to_owned())
            }
            Err(WorkerShutdownError::ThreadPanicked) => {
                Err("worker thread panicked during shutdown".to_owned())
            }
        };
        outcome.map_err(|error| safe_shutdown_detail(&error))
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        if self.join.is_none() {
            return;
        }
        // A direct handle owner is not allowed to detach a credential-holding
        // worker silently. Give cancellation the same bounded opportunity as
        // the explicit API, then leave an unmistakable diagnostic if the
        // thread still cannot be joined.
        if let Err(error) =
            self.shutdown_bounded(std::time::Duration::from_secs(2))
        {
            eprintln!(
                "siralos: worker handle dropped before shutdown completed: {error}"
            );
        }
    }
}

/// The frontend half of the bridge: the worker seen as the drain's source.
///
/// \`drain_events\` reads session events through \`EventSource\`; everything the
/// worker sends that is NOT a session event -- a pane snapshot, a display
/// report, a failure, the end of a turn, the final stop -- is handed back by
/// \`take_pending\` for \`apply_worker_event\`. The split is deliberate: the drain
/// owns the sanitizer boundary, the frontend owns what it does with the rest.
///
/// Ordering: session events keep their order among themselves and pending
/// events keep theirs. A pane snapshot arriving between two deltas is applied
/// after both, which decision 167 D1 already allows (advisory, may lag a frame).
pub struct WorkerSource {
    handle: WorkerHandle,
    ready: std::collections::VecDeque<ToolLoopEvent>,
    pending: Vec<WorkerEvent>,
    deferred: std::collections::VecDeque<WorkerEvent>,
    pub(crate) output_sanitizer: std::cell::RefCell<TerminalSanitizer>,
}

impl std::fmt::Debug for WorkerSource {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        // Only queue cardinalities: the queued events carry provider and tool
        // text, and a startup error type can be formatted by a caller.
        formatter
            .debug_struct("WorkerSource")
            .field("ready", &self.ready.len())
            .field("pending", &self.pending.len())
            .field("deferred", &self.deferred.len())
            .finish_non_exhaustive()
    }
}

impl WorkerSource {
    /// Wrap a handle (from \`spawn_worker\`) as a drainable source.
    #[must_use]
    pub fn new(handle: WorkerHandle) -> Self {
        Self {
            handle,
            ready: std::collections::VecDeque::new(),
            pending: Vec::new(),
            deferred: std::collections::VecDeque::new(),
            output_sanitizer: std::cell::RefCell::new(TerminalSanitizer::new()),
        }
    }

    /// The non-session events seen since the last call, in order.
    pub fn take_pending(&mut self) -> Vec<WorkerEvent> {
        std::mem::take(&mut self.pending)
    }

    /// Send one command (\`false\` when the worker is already gone).
    pub fn send(&self, command: WorkerCommand) -> bool {
        self.handle.send(command)
    }

    /// The shared cancel flag, for a loop that also polls it directly.
    #[must_use]
    pub fn cancel_flag(&self) -> &CancelFlag {
        self.handle.cancel_flag()
    }

    /// Wait up to `timeout` for the next worker event (C2 step 3).
    ///
    /// `drain_events` keeps its non-blocking `poll_event`; a frontend that owns
    /// a clock uses THIS instead, so an idle worker costs one tick rather than
    /// a blocked UI thread.
    pub fn wait(&self, timeout: std::time::Duration) -> WorkerWait {
        self.handle.wait(timeout)
    }

    fn admit_event(&mut self, event: WorkerEvent, max_pending: usize) {
        match event {
            WorkerEvent::Session(inner) => {
                if self.ready.len() < max_pending {
                    self.ready.push_back(inner);
                } else {
                    self.deferred.push_back(WorkerEvent::Session(inner));
                }
            }
            other if self.pending.len() < max_pending => {
                self.pending.push(other)
            }
            other => self.deferred.push_back(other),
        }
    }

    ///
    /// Only for the one moment a frontend can afford to wait with nothing to
    /// draw: the startup handshake, before its terminal exists. Inside a turn
    /// the frontend uses `wait`, because its clock must keep running.
    pub fn recv(&self) -> Option<WorkerEvent> {
        self.handle.recv()
    }

    /// Compatibility wrapper; use [`WorkerSource::shutdown_typed`] when the
    /// lifecycle result matters.
    pub fn shutdown(&mut self) {
        self.handle.shutdown();
    }

    /// Stop the worker with the same bounded deadline while preserving the
    /// timeout/panic and replay-flush diagnostics for callers that can report
    /// them.
    pub fn shutdown_bounded(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        self.handle.shutdown_bounded(timeout)
    }

    /// Return the worker's typed stop outcome or a typed bounded-join error.
    pub fn shutdown_typed(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<WorkerStopOutcome, WorkerShutdownError> {
        self.handle.shutdown_typed(timeout)
    }
}

/// Step 4: the worker is stopped on EVERY exit path (decision 168 R6).
///
/// The normal exit calls `shutdown` explicitly, which is what a reader expects.
/// This guard is what makes the guarantee true for the OTHER paths as well: an
/// early `?` return, a panic inside the loop, or an exit branch added later
/// that nobody remembers to shut down.
///
/// It must be declared AFTER the terminal guard: locals drop in reverse
/// declaration order, so the join (and therefore the recordings' single flush)
/// happens BEFORE the alternate screen is restored -- known, not hoped for.
pub struct WorkerGuard {
    source: WorkerSource,
    stopped: bool,
}

impl WorkerGuard {
    /// Take ownership of a running worker.
    #[must_use]
    pub fn new(source: WorkerSource) -> Self {
        Self { source, stopped: false }
    }

    /// The worker, for as long as the guard has not stopped it.
    pub fn source(&mut self) -> &mut WorkerSource {
        &mut self.source
    }

    /// Compatibility wrapper for cleanup call sites that cannot return an
    /// error. A bounded failure is printed, never silently treated as success.
    pub fn shutdown(&mut self) {
        if let Err(error) = self.shutdown_result() {
            eprintln!("siralos: worker shutdown: {error}");
        }
    }

    /// Stop the worker once and return the typed stop outcome.
    pub fn shutdown_typed(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<WorkerStopOutcome, WorkerShutdownError> {
        if self.stopped {
            return Ok(WorkerStopOutcome::AlreadyStopped);
        }
        let outcome = self.source.shutdown_typed(timeout);
        match &outcome {
            // A flush failure is still a completed join, so do not retry a
            // stopped worker on guard drop; the returned outcome carries the
            // failure to the caller. A panic also consumed the join handle
            // and cannot be retried.
            Ok(_) | Err(WorkerShutdownError::ThreadPanicked) => {
                self.stopped = true;
            }
            Err(WorkerShutdownError::TimedOut) => {}
        }
        outcome
    }

    /// Stop the worker once and return any bounded-shutdown or flush failure.
    pub fn shutdown_result(&mut self) -> Result<(), String> {
        let outcome = match self.shutdown_typed(std::time::Duration::from_secs(2)) {
            Ok(WorkerStopOutcome::AlreadyStopped) => Ok(()),
            Ok(WorkerStopOutcome::FlushEvidenceMissing) => Err(
                "replay flush outcome is unavailable; typed persistence evidence is missing"
                    .to_owned(),
            ),
            Ok(WorkerStopOutcome::Flushed(_)) => Ok(()),
            Ok(WorkerStopOutcome::FlushFailed(error)) => {
                Err(format!("replay flush failed: {error}"))
            }
            Ok(WorkerStopOutcome::QuiesceFailed { detail, flush }) => {
                let flush_detail = match flush {
                    Ok(FlushOutcome::Legacy) => "; replay flush outcome is unavailable"
                        .to_owned(),
                    Ok(_) => String::new(),
                    Err(error) => format!("; replay flush failed: {error}"),
                };
                Err(format!(
                    "worker response did not quiesce: {detail}{flush_detail}"
                ))
            }
            Err(WorkerShutdownError::TimedOut) => {
                Err("worker shutdown timed out; recordings may be incomplete"
                    .to_owned())
            }
            Err(WorkerShutdownError::ThreadPanicked) => {
                Err("worker thread panicked during shutdown".to_owned())
            }
        };
        outcome.map_err(|error| safe_shutdown_detail(&error))
    }
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown_result() {
            eprintln!("siralos: worker shutdown: {error}");
        }
    }
}

impl EventSource for WorkerSource {
    fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        const MAX_PENDING: usize = 2048;
        // Preserve ordering when either local bounded queue is full. The
        // worker now applies backpressure on its channel; the frontend keeps
        // overflow here rather than silently losing a terminal/control event.
        if let Some(event) = self.deferred.pop_front() {
            self.admit_event(event, MAX_PENDING);
        }
        // Do not remove another event from the bounded channel unless there
        // is room in every local queue. `try_recv_all` would otherwise pull a
        // batch into an unbounded-looking deferred queue and lose it when the
        // local cap was reached.
        while self.ready.len() < MAX_PENDING
            && self.pending.len() < MAX_PENDING
            && self.deferred.len() < MAX_PENDING
        {
            let Some(event) = self.handle.try_recv() else {
                break;
            };
            self.admit_event(event, MAX_PENDING);
        }
        if let Some(event) = self.deferred.pop_front() {
            self.admit_event(event, MAX_PENDING);
        }
        self.ready.pop_front()
    }

    fn cancel(&mut self) {
        // Both halves: the flag reaches the worker between events, and the
        // command reaches it when it is not in a turn. An already-blocked
        // provider read remains bounded by that provider's transport seam.
        self.handle.cancel_flag().request();
        self.handle.send(WorkerCommand::Cancel);
    }
}

/// Spawn the worker that OWNS the session (C2 step 2).
///
/// The session cannot cross threads (decision 167), so composition happens
/// inside the thread: this takes owned paths and builds `InteractiveOptions`
/// from them there. A composition failure is reported as a `Failed` event
/// rather than a panic, so a frontend can render it truthfully.
pub fn spawn_worker(
    workspace_root: Option<std::path::PathBuf>,
    config_path: Option<std::path::PathBuf>,
) -> WorkerHandle {
    const COMMAND_CAPACITY: usize = 256;
    const EVENT_CAPACITY: usize = 4096;
    let (command_tx, command_rx) =
        std::sync::mpsc::sync_channel::<WorkerCommand>(COMMAND_CAPACITY);
    let (event_tx, event_rx) =
        std::sync::mpsc::sync_channel::<WorkerEvent>(EVENT_CAPACITY);
    let cancel = CancelFlag::new();
    let thread_cancel = cancel.clone();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_stop = std::sync::Arc::clone(&stop);
    let join = std::thread::spawn(move || {
        let options = crate::interactive::InteractiveOptions {
            config_path: config_path.as_deref(),
            workspace_root: workspace_root.as_deref(),
        };
        match crate::interactive::compose_session(options) {
            Ok(mut session) => {
                session.enable_progress_ticks();
                run_worker_loop_with_stop(
                    &command_rx,
                    &event_tx,
                    &thread_cancel,
                    &thread_stop,
                    &mut session,
                )
            }
            Err(error) => {
                let _ = event_tx.send(WorkerEvent::Failed(error.to_string()));
                let _ = event_tx.send(WorkerEvent::Stopped);
                WorkerStopOutcome::AlreadyStopped
            }
        }
    });
    WorkerHandle {
        commands: command_tx,
        events: event_rx,
        cancel,
        stop,
        join: Some(join),
    }
}

/// What a frontend does with one worker event (C2 step 3).
///
/// The semantics that matter -- the sanitizer boundary, the exact failure
/// wording, and where the thinking goes -- live HERE rather than inside the
/// loop, so they are tested before the loop is rewired to the worker.
///
/// # Errors
///
/// Propagates the writer's IO error; nothing else here can fail.
pub fn apply_worker_event<W: std::io::Write>(
    event: WorkerEvent,
    sanitizer: &mut crate::sanitize::TerminalSanitizer,
    writer: &mut W,
    reasoning: &mut dyn FnMut(&str),
    pane: &mut Option<crate::tui::ContextPaneData>,
) -> std::io::Result<()> {
    match event {
        // The header is frontend STATE, not transcript: the loop that
        // owns the header applies it, so nothing is written here.
        WorkerEvent::Ready(_) => {}
        // Model ids are frontend state too: the picker owns them, and the
        // transcript path that prints them does not run through a worker.
        WorkerEvent::Models(_) => {}
        WorkerEvent::Session(event) => match event {
            ToolLoopEvent::TextDelta { text } => {
                writer.write_all(sanitizer.push(&text).as_bytes())?;
            }
            ToolLoopEvent::ResponseCompleted => {
                writer.write_all(sanitizer.flush().as_bytes())?;
                writer.write_all(b"\n")?;
            }
            ToolLoopEvent::ResponseCancelled => {
                writer.write_all(sanitizer.flush().as_bytes())?;
                writer.write_all(b"Response cancelled.\n")?;
            }
            ToolLoopEvent::ResponseFailed { message } => {
                writer.write_all(sanitizer.flush().as_bytes())?;
                let safe = crate::sanitize::sanitize_for_display(&message);
                let line = format!("Response failed: {safe}\n");
                writer.write_all(line.as_bytes())?;
            }
            ToolLoopEvent::ToolFailed { message, .. } => {
                writer.write_all(sanitizer.flush().as_bytes())?;
                let safe = crate::sanitize::sanitize_for_display(&message);
                let line = format!("Tool failed: {safe}\n");
                writer.write_all(line.as_bytes())?;
            }
            ToolLoopEvent::ReasoningDelta { text } => reasoning(&text),
            // Enumerated, not a catch-all: a new variant must be classified
            // deliberately rather than silently ignored.
            ToolLoopEvent::ResponseStarted
            | ToolLoopEvent::ToolStarted { .. }
            | ToolLoopEvent::ToolCompleted { .. }
            | ToolLoopEvent::ToolCancelled { .. }
            | ToolLoopEvent::ProviderPending
            | ToolLoopEvent::ContextPressure { .. } => {}
        },
        WorkerEvent::Pane(data) => *pane = Some(data),
        WorkerEvent::Report(text) => {
            writer.write_all(sanitizer.flush().as_bytes())?;
            let safe = crate::sanitize::sanitize_for_display(&text);
            writer.write_all(safe.as_bytes())?;
        }
        WorkerEvent::Failed(message) => {
            writer.write_all(sanitizer.flush().as_bytes())?;
            let safe = crate::sanitize::sanitize_for_display(&message);
            let line = format!("Worker failed: {safe}\n");
            writer.write_all(line.as_bytes())?;
        }
        WorkerEvent::TurnFinished | WorkerEvent::Stopped => {
            // A turn/worker boundary closes the stateful output stream even
            // when a producer omitted an explicit terminal event. Otherwise a
            // dangling escape sequence from one turn could suppress or alter
            // the first bytes of the next turn.
            writer.write_all(sanitizer.flush().as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod worker_source_tests {
    use super::{
        CancelFlag, EventSource, WorkerCommand, WorkerEvent, WorkerHandle,
        WorkerSource,
    };
    use siralos_core::tool::ToolLoopEvent;

    /// A handle whose far end the test still owns, so the worker side is
    /// scripted without a thread and without a session: a thread is not a
    /// capability (decision 167), and this test needs neither.
    pub(crate) struct Scripted {
        pub(crate) source: WorkerSource,
        pub(crate) events: std::sync::mpsc::SyncSender<WorkerEvent>,
        pub(crate) commands: std::sync::mpsc::Receiver<WorkerCommand>,
        pub(crate) cancel: CancelFlag,
    }

    /// Shared with the drain test in `interactive`, which drives the real
    /// drain from this scripted worker.
    pub(crate) fn scripted() -> Scripted {
        let (command_tx, commands) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(256);
        let (events, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(4096);
        let cancel = CancelFlag::new();
        let handle = WorkerHandle {
            commands: command_tx,
            events: event_rx,
            cancel: cancel.clone(),
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                false,
            )),
            join: None,
        };
        Scripted {
            source: WorkerSource::new(handle),
            events,
            commands,
            cancel,
        }
    }

    fn text(value: &str) -> WorkerEvent {
        WorkerEvent::Session(ToolLoopEvent::TextDelta {
            text: value.to_owned(),
        })
    }

    #[test]
    fn worker_source_feeds_the_drain_session_events_in_order() {
        let mut s = scripted();
        assert_eq!(
            s.source.poll_event(),
            None,
            "an empty channel yields nothing"
        );

        s.events.send(text("a")).expect("send");
        s.events.send(WorkerEvent::Report("ctx".to_owned())).expect("send");
        s.events.send(text("b")).expect("send");

        assert_eq!(
            s.source.poll_event(),
            Some(ToolLoopEvent::TextDelta { text: "a".to_owned() })
        );
        assert_eq!(
            s.source.poll_event(),
            Some(ToolLoopEvent::TextDelta { text: "b".to_owned() }),
            "a display event must not be mistaken for a session event"
        );
        assert_eq!(s.source.poll_event(), None);
    }

    #[test]
    fn worker_source_hands_back_everything_that_is_not_a_session_event() {
        let mut s = scripted();
        s.events.send(WorkerEvent::Report("tools".to_owned())).expect("send");
        s.events.send(WorkerEvent::TurnFinished).expect("send");
        s.events.send(WorkerEvent::Stopped).expect("send");

        // Draining the session side must not swallow the rest.
        assert_eq!(s.source.poll_event(), None);
        assert_eq!(
            s.source.take_pending(),
            vec![
                WorkerEvent::Report("tools".to_owned()),
                WorkerEvent::TurnFinished,
                WorkerEvent::Stopped,
            ],
            "pending events keep their order for apply_worker_event"
        );
        assert!(s.source.take_pending().is_empty(), "taking drains them");
    }

    #[test]
    fn worker_source_cancels_on_both_channels_and_stops_the_worker() {
        let mut s = scripted();
        assert!(!s.cancel.is_requested());
        s.source.cancel();
        assert!(
            s.cancel.is_requested(),
            "the flag reaches the worker poll seam"
        );
        assert_eq!(s.commands.try_recv(), Ok(WorkerCommand::Cancel));

        assert!(s.source.send(WorkerCommand::ContextReport));
        assert_eq!(s.commands.try_recv(), Ok(WorkerCommand::ContextReport));

        s.source.shutdown();
        assert_eq!(
            s.commands.try_recv(),
            Ok(WorkerCommand::Shutdown),
            "shutdown is the command that flushes the recordings"
        );
    }

    #[test]
    fn dropping_the_guard_stops_the_worker_exactly_once() {
        // Step 4: the worker is stopped on EVERY exit path, which the guard is
        // what makes true for the paths a reader is not looking at (an early
        // `?` return, a panic). The property is the DROP.
        let s = scripted();
        {
            let _guard = super::WorkerGuard::new(s.source);
        }
        assert_eq!(s.commands.try_recv(), Ok(WorkerCommand::Shutdown));
        assert_eq!(
            s.commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected),
            "the guard took the handle with it, so a second Shutdown is impossible"
        );
    }

    #[test]
    fn an_explicit_shutdown_is_not_repeated_when_the_guard_drops() {
        let s = scripted();
        {
            let mut guard = super::WorkerGuard::new(s.source);
            guard.shutdown();
        }
        assert_eq!(s.commands.try_recv(), Ok(WorkerCommand::Shutdown));
        assert_eq!(
            s.commands.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected),
            "the explicit exit path and the drop are one shutdown"
        );
    }

    #[test]
    fn a_wait_reports_idle_then_the_event_and_then_a_gone_worker() {
        // The frontend's turn loop reads exactly these three outcomes: `Idle` is
        // the tick that keeps the reveal moving while the model is silent, and
        // `Gone` is a worker that died before it answered.
        let s = scripted();
        assert!(
            matches!(
                s.source.wait(std::time::Duration::from_millis(1)),
                super::WorkerWait::Idle
            ),
            "an empty channel is a tick, not an error"
        );

        s.events.send(text("a")).expect("send");
        match s.source.wait(std::time::Duration::from_secs(5)) {
            super::WorkerWait::Event(event) => assert_eq!(event, text("a")),
            other => panic!("expected the event, got {other:?}"),
        }

        drop(s.events);
        assert!(
            matches!(
                s.source.wait(std::time::Duration::from_millis(1)),
                super::WorkerWait::Gone
            ),
            "a worker that vanished must be distinguishable from an idle one"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{CancelFlag, WorkerCommand, WorkerEvent};
    use siralos_core::tool::ToolLoopEvent;

    #[test]
    fn the_contract_crosses_a_real_thread() {
        // C2's whole premise: the UI and the worker exchange plain data. This
        // moves both directions through a real channel on a real thread.
        let (command_tx, command_rx) =
            std::sync::mpsc::channel::<WorkerCommand>();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<WorkerEvent>();
        let worker = std::thread::spawn(move || {
            while let Ok(command) = command_rx.recv() {
                match command {
                    WorkerCommand::Prompt(text) => {
                        event_tx
                            .send(WorkerEvent::Session(
                                ToolLoopEvent::ResponseStarted,
                            ))
                            .expect("event");
                        event_tx
                            .send(WorkerEvent::Report(format!("ran {text}")))
                            .expect("event");
                        event_tx
                            .send(WorkerEvent::TurnFinished)
                            .expect("event");
                    }
                    WorkerCommand::Shutdown => {
                        event_tx.send(WorkerEvent::Stopped).expect("event");
                        break;
                    }
                    other => {
                        event_tx
                            .send(WorkerEvent::Failed(format!("{other:?}")))
                            .expect("event");
                    }
                }
            }
        });
        command_tx
            .send(WorkerCommand::Prompt("hello".to_owned()))
            .expect("send");
        assert_eq!(
            event_rx.recv().expect("event"),
            WorkerEvent::Session(ToolLoopEvent::ResponseStarted)
        );
        assert_eq!(
            event_rx.recv().expect("event"),
            WorkerEvent::Report("ran hello".to_owned())
        );
        assert_eq!(event_rx.recv().expect("event"), WorkerEvent::TurnFinished);
        command_tx.send(WorkerCommand::Shutdown).expect("send");
        assert_eq!(event_rx.recv().expect("event"), WorkerEvent::Stopped);
        worker.join().expect("worker thread");
    }

    #[test]
    fn the_cancel_flag_is_shared_not_copied() {
        // Decision 167: the UI sets it and the worker sees it, which is what
        // lets a cancel land without waiting for a blocked read.
        let flag = CancelFlag::new();
        let seen = flag.clone();
        assert!(!seen.is_requested());
        flag.request();
        assert!(seen.is_requested(), "the clone observes the same flag");
        seen.clear();
        assert!(!flag.is_requested());
    }
}

#[cfg(test)]
mod loop_tests {
    #[test]
    fn the_bridge_keeps_the_sanitizer_as_the_output_boundary() {
        use super::{WorkerEvent, apply_worker_event};
        use crate::sanitize::TerminalSanitizer;
        use siralos_core::tool::ToolLoopEvent;
        let mut sanitizer = TerminalSanitizer::new();
        let mut out: Vec<u8> = Vec::new();
        let mut reasoning = |_text: &str| {};
        let mut pane = None;
        apply_worker_event(
            WorkerEvent::Session(ToolLoopEvent::TextDelta {
                text: "\u{1b}[31mred".to_owned(),
            }),
            &mut sanitizer,
            &mut out,
            &mut reasoning,
            &mut pane,
        )
        .expect("write");
        let text = String::from_utf8_lossy(&out);
        assert!(!text.contains('\u{1b}'), "escapes are stripped");
    }

    #[test]
    fn a_turn_boundary_resets_a_dangling_escape_state() {
        use super::{WorkerEvent, apply_worker_event};
        use crate::sanitize::TerminalSanitizer;
        use siralos_core::tool::ToolLoopEvent;
        let mut sanitizer = TerminalSanitizer::new();
        let mut out: Vec<u8> = Vec::new();
        let mut reasoning = |_text: &str| {};
        let mut pane = None;
        apply_worker_event(
            WorkerEvent::Session(ToolLoopEvent::TextDelta {
                text: "\u{1b}[".to_owned(),
            }),
            &mut sanitizer,
            &mut out,
            &mut reasoning,
            &mut pane,
        )
        .expect("write");
        apply_worker_event(
            WorkerEvent::TurnFinished,
            &mut sanitizer,
            &mut out,
            &mut reasoning,
            &mut pane,
        )
        .expect("write");
        apply_worker_event(
            WorkerEvent::Session(ToolLoopEvent::TextDelta {
                text: "31mnext".to_owned(),
            }),
            &mut sanitizer,
            &mut out,
            &mut reasoning,
            &mut pane,
        )
        .expect("write");
        assert_eq!(String::from_utf8(out).expect("utf8"), "31mnext");
    }

    #[test]
    fn the_bridge_routes_thinking_to_its_own_sink_and_reports_failures() {
        use super::{WorkerEvent, apply_worker_event};
        use crate::sanitize::TerminalSanitizer;
        use siralos_core::tool::ToolLoopEvent;
        let mut sanitizer = TerminalSanitizer::new();
        let mut out: Vec<u8> = Vec::new();
        let mut thinking = String::new();
        let mut pane = None;
        {
            let mut reasoning = |text: &str| thinking.push_str(text);
            apply_worker_event(
                WorkerEvent::Session(ToolLoopEvent::ReasoningDelta {
                    text: "weighing".to_owned(),
                }),
                &mut sanitizer,
                &mut out,
                &mut reasoning,
                &mut pane,
            )
            .expect("write");
            apply_worker_event(
                WorkerEvent::Session(ToolLoopEvent::ResponseFailed {
                    message: "provider exploded".to_owned(),
                }),
                &mut sanitizer,
                &mut out,
                &mut reasoning,
                &mut pane,
            )
            .expect("write");
            apply_worker_event(
                WorkerEvent::Pane(crate::tui::ContextPaneData {
                    counters: vec![("assembled".to_owned(), 7)],
                    ring: Vec::new(),
                    activity: Vec::new(),
                }),
                &mut sanitizer,
                &mut out,
                &mut reasoning,
                &mut pane,
            )
            .expect("write");
        }
        assert_eq!(thinking, "weighing");
        let text = String::from_utf8_lossy(&out);
        assert!(!text.contains("weighing"), "thinking is not transcript text");
        assert!(
            text.contains("Response failed: provider exploded"),
            "the failure wording is preserved"
        );
        assert_eq!(pane.expect("pane").counters.len(), 1);
    }

    #[test]
    fn a_spawned_worker_composes_its_own_session_and_stops_cleanly() {
        // C2 step 2 end to end: the session CANNOT cross threads (decision
        // 167), so it is constructed inside the worker. This drives a real
        // worker against a real (empty) workspace: no profile means the
        // deterministic fake provider, and shutdown must still stop cleanly.
        let dir = std::env::temp_dir().join(format!(
            "siralos-worker-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp workspace");
        let mut worker = super::spawn_worker(Some(dir.clone()), None);
        assert!(
            worker.send(WorkerCommand::Shutdown),
            "the worker accepts a command"
        );
        let mut stopped = false;
        let mut failed: Option<String> = None;
        while let Some(event) = worker.recv() {
            match event {
                WorkerEvent::Stopped => {
                    stopped = true;
                    break;
                }
                // A composition failure ALSO ends in Stopped, so the test
                // would pass on a broken worker: record it and fail below.
                WorkerEvent::Failed(message) => failed = Some(message),
                _ => {}
            }
        }
        assert_eq!(
            failed, None,
            "the worker composed a real session, not a failure"
        );
        assert!(stopped, "the worker flushed and reported Stopped");
        worker.shutdown();
        let _ = std::fs::remove_dir_all(dir);
    }

    use super::{
        CancelFlag, FlushError, FlushOutcome, SessionStatus, WorkerCommand,
        WorkerEvent, WorkerHandle, WorkerSession, WorkerStopOutcome,
        run_worker_loop, run_worker_loop_typed, run_worker_loop_with_stop,
    };
    use siralos_core::tool::ToolLoopEvent;

    /// A scripted session: records what the loop asked of it.
    #[derive(Default)]
    struct FakeSession {
        events: std::collections::VecDeque<ToolLoopEvent>,
        prompts: Vec<String>,
        cancels: usize,
        flushes: usize,
        refuse: Option<String>,
        responding: bool,
        stays_responding_after_cancel: bool,
        models: Vec<String>,
        reload_report: Option<String>,
        reload_refusal: Option<String>,
        settled: usize,
        pane: Option<crate::tui::ContextPaneData>,
        flush_result: Option<Result<FlushOutcome, FlushError>>,
    }

    impl WorkerSession for FakeSession {
        fn send_prompt(&mut self, prompt: &str) -> Result<(), String> {
            if let Some(message) = self.refuse.take() {
                return Err(message);
            }
            self.prompts.push(prompt.to_owned());
            self.responding = true;
            Ok(())
        }
        fn poll_event(&mut self) -> Option<ToolLoopEvent> {
            let next = self.events.pop_front();
            if next.is_none() && !self.stays_responding_after_cancel {
                self.responding = false;
            }
            next
        }
        fn is_responding(&self) -> bool {
            self.responding
        }
        fn pane(&self) -> Option<crate::tui::ContextPaneData> {
            self.pane.clone()
        }
        fn context_report(&self) -> String {
            "context".to_owned()
        }
        fn tools_report(&self) -> String {
            "tools".to_owned()
        }
        fn set_model(&mut self, model: &str) -> Result<(), String> {
            // Record it: a double that discards its argument cannot fail a test.
            self.models.push(model.to_owned());
            Ok(())
        }
        fn reload(&mut self) -> Result<String, String> {
            // Faithful to the REAL adapter: it re-reads the profile,
            // recomposes, applies and RETURNS the report, so a reload reaches
            // the frontend as a Report. `reload_refusal` exists because the
            // loop must still report a refusal truthfully if a session ever
            // has one -- the adapter does not today.
            if let Some(message) = self.reload_refusal.take() {
                return Err(message);
            }
            Ok(self.reload_report.take().unwrap_or_else(|| {
                "reload applied: nothing changed\n".to_owned()
            }))
        }
        fn cancel(&mut self) {
            self.cancels += 1;
            if !self.stays_responding_after_cancel {
                self.responding = false;
            }
        }
        fn turn_settled(&mut self) {
            self.settled += 1;
        }
        fn fetch_models(
            &mut self,
            _cancellation: &CancelFlag,
        ) -> Result<Vec<String>, String> {
            Ok(vec!["fake-model".to_owned()])
        }
        fn domains_add(&mut self, folder: &str) -> Result<String, String> {
            Ok(format!("added {folder}"))
        }
        fn domains_enable(&mut self, id: &str) -> Result<String, String> {
            Ok(format!("enabled {id}"))
        }
        fn domains_activate(&mut self, id: &str) -> Result<String, String> {
            Ok(format!("activated {id}"))
        }
        fn status(&self) -> SessionStatus {
            SessionStatus {
                status: "fake status".to_owned(),
                provider: Some("fake".to_owned()),
                model: Some("fake-model".to_owned()),
                endpoint: None,
                protocol: "openai-completions".to_owned(),
                credential_display: None,
                credential_resolved: false,
                live_model_switchable: true,
                context_suffix: String::new(),
            }
        }
        fn enable_progress_ticks(&mut self) {}
        fn flush(&mut self) {
            self.flushes += 1;
        }
        fn flush_result(&mut self) -> Result<FlushOutcome, FlushError> {
            self.flushes += 1;
            // A scripted session has no retaining recorder, which is typed
            // evidence in its own right -- not the LEGACY "a void flush ran"
            // ambiguity. The legacy path is pinned by its own test below.
            self.flush_result.take().unwrap_or(Ok(FlushOutcome::NoRecorder))
        }
    }

    fn run(
        commands: Vec<WorkerCommand>,
        session: &mut FakeSession,
        cancel: &CancelFlag,
    ) -> Vec<WorkerEvent> {
        let (command_tx, command_rx) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(256);
        let (event_tx, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(4096);
        for command in commands {
            command_tx.send(command).expect("send");
        }
        drop(command_tx);
        run_worker_loop(&command_rx, &event_tx, cancel, session);
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }
        events
    }
    #[test]
    fn a_domain_command_reports_what_the_session_did() {
        // Decision 168 R4: these mutate the session's domain registry, so the
        // report is the session's too -- the loop only relays it.
        let mut session = FakeSession::default();
        let cancel = CancelFlag::new();
        let events = run(
            vec![WorkerCommand::DomainsActivate("demo".to_owned())],
            &mut session,
            &cancel,
        );
        assert!(
            events.contains(&WorkerEvent::Report("activated demo".to_owned())),
            "got: {events:?}"
        );
    }

    #[test]
    fn a_model_list_reaches_the_frontend() {
        let mut session = FakeSession::default();
        let cancel = CancelFlag::new();
        let events =
            run(vec![WorkerCommand::ModelsFetch], &mut session, &cancel);
        assert!(
            events
                .contains(&WorkerEvent::Models(vec!["fake-model".to_owned()])),
            "the list is data for the picker, got: {events:?}"
        );
    }

    #[test]
    fn the_worker_announces_the_header_before_any_command() {
        let mut session = FakeSession::default();
        let cancel = CancelFlag::new();
        let events = run(vec![], &mut session, &cancel);
        assert!(
            matches!(events.first(), Some(WorkerEvent::Ready(_))),
            "the frontend cannot derive its header any more, so it arrives first: {events:?}"
        );
    }

    #[test]
    fn a_finished_turn_is_settled_once_by_the_loop() {
        let mut session = FakeSession::default();
        let cancel = CancelFlag::new();
        let events = run(
            vec![WorkerCommand::Prompt("hi".to_owned())],
            &mut session,
            &cancel,
        );
        assert_eq!(
            session.settled, 1,
            "the session settles its turn once, after the events"
        );
        assert!(
            events.contains(&WorkerEvent::TurnFinished),
            "and the frontend still hears the turn is over"
        );
    }

    #[test]
    fn a_reload_report_comes_from_the_session_not_the_loop() {
        let mut session = FakeSession {
            reload_report: Some(
                "reload applied: model=beta (restart to converge)\n"
                    .to_owned(),
            ),
            ..FakeSession::default()
        };
        let cancel = CancelFlag::new();
        let events = run(vec![WorkerCommand::Reload], &mut session, &cancel);
        assert_eq!(
            events,
            vec![
                WorkerEvent::Ready(SessionStatus {
                    status: "fake status".to_owned(),
                    provider: Some("fake".to_owned()),
                    model: Some("fake-model".to_owned()),
                    endpoint: None,
                    protocol: "openai-completions".to_owned(),
                    credential_display: None,
                    credential_resolved: false,
                    live_model_switchable: true,
                    context_suffix: String::new(),
                }),
                WorkerEvent::Report(
                    "reload applied: model=beta (restart to converge)\n"
                        .to_owned()
                ),
                WorkerEvent::Ready(SessionStatus {
                    status: "fake status".to_owned(),
                    provider: Some("fake".to_owned()),
                    model: Some("fake-model".to_owned()),
                    endpoint: None,
                    protocol: "openai-completions".to_owned(),
                    credential_display: None,
                    credential_resolved: false,
                    live_model_switchable: true,
                    context_suffix: String::new(),
                }),
                // The command channel closed without a Shutdown: the loop
                // still flushes once, and says so.
                WorkerEvent::Stopped,
            ],
            "the loop announces exactly what the session did, nothing of its own"
        );
    }

    #[test]
    fn a_model_switch_reaches_the_session_and_reannounces_the_header() {
        // The bridge command is driven through the loop: a double that
        // discarded its argument would let a broken wiring pass this.
        let mut session = FakeSession::default();
        let events = run(
            vec![
                WorkerCommand::SetModel("example/model-b".to_owned()),
                WorkerCommand::Shutdown,
            ],
            &mut session,
            &CancelFlag::new(),
        );
        assert_eq!(session.models, vec!["example/model-b".to_owned()]);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, WorkerEvent::Ready(_)))
                .count(),
            2,
            "the header is announced at startup and again when the switch moved the composition: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, WorkerEvent::Report(_))),
            "the frontend owns the persist and its message, so the worker adds no line of its own"
        );
    }

    #[test]
    fn a_reload_refusal_is_reported_not_swallowed() {
        let mut session = FakeSession {
            reload_refusal: Some("reload is not available".to_owned()),
            ..FakeSession::default()
        };
        let events =
            run(vec![WorkerCommand::Reload], &mut session, &CancelFlag::new());
        assert!(events.iter().any(|event| matches!(
            event,
            WorkerEvent::Failed(message) if message.contains("not available")
        )));
    }

    #[test]
    fn a_prompt_forwards_its_events_and_finishes() {
        let mut session = FakeSession {
            events: vec![
                ToolLoopEvent::ResponseStarted,
                ToolLoopEvent::ResponseCompleted,
            ]
            .into(),
            ..FakeSession::default()
        };
        let events = run(
            vec![
                WorkerCommand::Prompt("hi".to_owned()),
                WorkerCommand::Shutdown,
            ],
            &mut session,
            &CancelFlag::new(),
        );
        assert_eq!(session.prompts, vec!["hi".to_owned()]);
        assert!(
            events.contains(&WorkerEvent::Session(
                ToolLoopEvent::ResponseStarted
            ))
        );
        assert!(events.contains(&WorkerEvent::TurnFinished));
        assert_eq!(events.last(), Some(&WorkerEvent::Stopped));
    }

    #[test]
    fn a_refused_prompt_reports_failure_and_never_looks_like_success() {
        let mut session = FakeSession {
            refuse: Some("already responding".to_owned()),
            ..FakeSession::default()
        };
        let events = run(
            vec![
                WorkerCommand::Prompt("hi".to_owned()),
                WorkerCommand::Shutdown,
            ],
            &mut session,
            &CancelFlag::new(),
        );
        assert!(
            events.contains(&WorkerEvent::Failed(
                "already responding".to_owned()
            ))
        );
        assert_eq!(events.last(), Some(&WorkerEvent::Stopped));
    }

    #[test]
    fn the_cancel_flag_and_the_cancel_command_both_reach_the_session() {
        let cancel = CancelFlag::new();
        cancel.request();
        let mut session = FakeSession {
            events: vec![
                ToolLoopEvent::ResponseStarted,
                ToolLoopEvent::ResponseCancelled,
            ]
            .into(),
            ..FakeSession::default()
        };
        // The flag is set before the prompt: the loop clears it at turn start,
        // so the explicit Cancel command is what must land here.
        let events = run(
            vec![WorkerCommand::Cancel, WorkerCommand::Shutdown],
            &mut session,
            &cancel,
        );
        assert_eq!(session.cancels, 1, "Cancel reaches the session");
        assert_eq!(events.last(), Some(&WorkerEvent::Stopped));
    }

    #[test]
    fn an_idle_cancel_does_not_stick_to_the_next_prompt() {
        let mut session = FakeSession::default();
        let events = run(
            vec![
                WorkerCommand::Cancel,
                WorkerCommand::Prompt("hi".to_owned()),
            ],
            &mut session,
            &CancelFlag::new(),
        );
        assert_eq!(session.prompts, vec!["hi".to_owned()]);
        assert!(
            !events.iter().any(|event| matches!(
                event,
                WorkerEvent::Failed(message)
                    if message.contains("cancelled before it started")
            )),
            "the next prompt must not inherit an idle cancel"
        );
    }

    #[test]
    fn the_worker_pushes_the_startup_pane_and_a_settled_one() {
        // Decision 167 D1: the frontend no longer BUILDS the pane -- it cannot,
        // because the metrics and the history live here. It arrives with the
        // header, after every event, and once more after the turn settled (the
        // demand tick has run by then).
        let pane = crate::tui::ContextPaneData {
            counters: vec![("ticks_total".to_owned(), 3)],
            ring: Vec::new(),
            activity: Vec::new(),
        };
        let mut session = FakeSession {
            pane: Some(pane.clone()),
            events: vec![ToolLoopEvent::ResponseStarted].into(),
            ..FakeSession::default()
        };
        let events = run(
            vec![WorkerCommand::Prompt("hi".to_owned())],
            &mut session,
            &CancelFlag::new(),
        );
        assert_eq!(
            events,
            vec![
                WorkerEvent::Pane(pane.clone()),
                WorkerEvent::Ready(session.status()),
                WorkerEvent::Session(ToolLoopEvent::ResponseStarted),
                WorkerEvent::Pane(pane.clone()),
                WorkerEvent::Pane(pane),
                WorkerEvent::TurnFinished,
                WorkerEvent::Stopped,
            ],
            "the pane goes out before the header and again after the settled turn"
        );
    }

    #[test]
    fn a_second_shutdown_never_flushes_a_second_time() {
        let mut session = FakeSession::default();
        run(
            vec![WorkerCommand::Shutdown, WorkerCommand::Shutdown],
            &mut session,
            &CancelFlag::new(),
        );
        assert_eq!(session.flushes, 1, "the loop returns on the first one");
    }

    #[test]
    fn the_recordings_flush_exactly_once_before_the_terminal_guard_releases() {
        // Step 4's real question is not "was flush called" but "has the store
        // been written by the time the frontend restores the terminal". This
        // drives the REAL worker on a workspace that opted into record-replay
        // and observes the store on disk after the guard drops.
        let dir = std::env::temp_dir().join(format!(
            "siralos-worker-flush-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp workspace");
        std::fs::write(
            dir.join("siralos.toml"),
            "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-completions\"\nrecord-replay = true\n",
        )
        .expect("profile");
        let profile_bytes = std::fs::read(dir.join("siralos.toml"))
            .expect("read profile for approval");
        let digest = siralos_core::identity::sha256_hex(&profile_bytes);
        let config = dir.join(".test-user-config.json");
        std::fs::write(
            &config,
            format!("{{\"profileApproval\":\"{digest}\"}}"),
        )
        .expect("write test approval config");
        let store = dir.join(".siralos").join("replay-store.json");
        assert!(!store.exists(), "nothing is written before the shutdown");

        {
            let mut guard = super::WorkerGuard::new(super::WorkerSource::new(
                super::spawn_worker(Some(dir.clone()), Some(config.clone())),
            ));
            // The worker composes before it answers, so waiting for the header
            // makes the drop below a shutdown of a COMPOSED session.
            assert!(
                matches!(
                    guard.source().wait(std::time::Duration::from_secs(60)),
                    super::WorkerWait::Event(WorkerEvent::Ready(_))
                ),
                "the worker composed a session and announced its header"
            );
        }
        assert!(
            store.exists(),
            "the guard joined the worker, so the recordings were flushed"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn shutdown_flushes_exactly_once_and_a_closed_channel_flushes_anyway() {
        let mut session = FakeSession::default();
        run(vec![WorkerCommand::Shutdown], &mut session, &CancelFlag::new());
        assert_eq!(session.flushes, 1, "one owner, one flush");

        let mut session = FakeSession::default();
        run(Vec::new(), &mut session, &CancelFlag::new());
        assert_eq!(
            session.flushes, 1,
            "a channel that closes without a Shutdown still flushes once"
        );
    }

    #[test]
    fn a_flush_failure_is_a_typed_stop_failure_and_an_explicit_event() {
        let mut session = FakeSession {
            flush_result: Some(Err(FlushError::Persistence(
                "disk unavailable".to_owned(),
            ))),
            ..FakeSession::default()
        };
        let (command_tx, command_rx) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(8);
        let (event_tx, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(8);
        command_tx.send(WorkerCommand::Shutdown).expect("shutdown");
        drop(command_tx);
        let cancel = CancelFlag::new();
        let stop =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let outcome = run_worker_loop_with_stop(
            &command_rx,
            &event_tx,
            &cancel,
            &stop,
            &mut session,
        );
        assert_eq!(
            outcome,
            WorkerStopOutcome::FlushFailed(FlushError::Persistence(
                "disk unavailable".to_owned()
            ))
        );
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }
        assert!(events.iter().any(|event| matches!(
            event,
            WorkerEvent::Failed(message)
                if message.contains("replay flush failed")
                    && message.contains("disk unavailable")
        )));
        assert_eq!(events.last(), Some(&WorkerEvent::Stopped));
    }

    #[test]
    fn a_stop_that_cannot_quiesce_still_attempts_and_reports_the_flush() {
        let (command_tx, command_rx) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(2);
        let (event_tx, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(8);
        command_tx.send(WorkerCommand::Shutdown).expect("queue shutdown");
        let mut session = FakeSession {
            responding: true,
            stays_responding_after_cancel: true,
            flush_result: Some(Err(FlushError::Persistence(
                "store is read-only".to_owned(),
            ))),
            ..FakeSession::default()
        };
        let cancel = CancelFlag::new();
        let outcome = run_worker_loop_typed(
            &command_rx,
            &event_tx,
            &cancel,
            &mut session,
        );
        assert!(
            matches!(
                &outcome,
                WorkerStopOutcome::QuiesceFailed {
                    flush: Err(FlushError::Persistence(message)),
                    ..
                } if message.contains("read-only")
            ),
            "got {outcome:?}"
        );
        assert_eq!(session.flushes, 1, "the single flush is still attempted");
        let mut saw_failure = false;
        let mut saw_stop = false;
        while let Ok(event) = event_rx.try_recv() {
            match event {
                WorkerEvent::Failed(message) => {
                    saw_failure |= message.contains("did not quiesce")
                        && message.contains("replay flush failed");
                }
                WorkerEvent::Stopped => saw_stop = true,
                _ => {}
            }
        }
        assert!(saw_failure && saw_stop, "typed failure evidence is relayed");
    }

    #[test]
    fn the_handle_surfaces_flush_failure_instead_of_empty_shutdown_success() {
        let (command_tx, command_rx) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(8);
        let (event_tx, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(8);
        let cancel = CancelFlag::new();
        let thread_cancel = cancel.clone();
        let stop =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = std::sync::Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            let mut session = FakeSession {
                flush_result: Some(Err(FlushError::Persistence(
                    "store is read-only".to_owned(),
                ))),
                ..FakeSession::default()
            };
            run_worker_loop_with_stop(
                &command_rx,
                &event_tx,
                &thread_cancel,
                &thread_stop,
                &mut session,
            )
        });
        let mut handle = WorkerHandle {
            commands: command_tx,
            events: event_rx,
            cancel,
            stop,
            join: Some(join),
        };

        assert!(matches!(
            handle.shutdown_bounded(std::time::Duration::from_secs(1)),
            Err(message) if message.contains("replay flush failed")
        ));
    }

    #[test]
    fn a_legacy_flush_reports_missing_typed_evidence_once() {
        let (command_tx, command_rx) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(2);
        let (event_tx, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(8);
        command_tx.send(WorkerCommand::Shutdown).expect("queue shutdown");
        let mut session = FakeSession {
            flush_result: Some(Ok(FlushOutcome::Legacy)),
            ..FakeSession::default()
        };
        let cancel = CancelFlag::new();
        let outcome = run_worker_loop_typed(
            &command_rx,
            &event_tx,
            &cancel,
            &mut session,
        );
        assert_eq!(outcome, WorkerStopOutcome::FlushEvidenceMissing);
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }
        let missing = events
            .iter()
            .filter(|event| {
                matches!(event, WorkerEvent::Failed(message) if message
                    .contains("typed persistence evidence is missing"))
            })
            .count();
        assert_eq!(missing, 1, "reported exactly once, got: {events:?}");
        assert_eq!(events.last(), Some(&WorkerEvent::Stopped));
    }

    #[test]
    fn shutdown_details_are_single_line_bounded_and_secret_free() {
        // A plain lifecycle reason survives: the failure must stay visible.
        assert_eq!(
            super::safe_shutdown_detail(
                "worker shutdown timed out; recordings may be incomplete"
            ),
            "worker shutdown timed out; recordings may be incomplete"
        );
        // A secret, a URL, and an absolute workspace path do not.
        for hidden in [
            "replay flush failed: key:sk-live-abc",
            "provider said https://internal.example/v1 refused",
            r"could not open C:\Users\test\replay.jsonl",
        ] {
            assert_eq!(
                super::safe_shutdown_detail(hidden),
                "worker shutdown detail hidden",
                "got {:?}",
                super::safe_shutdown_detail(hidden)
            );
        }
        // Line structure is flattened, and the projection is bounded.
        let multiline = super::safe_shutdown_detail("first line\nsecond line");
        assert_eq!(multiline, "first line second line");
        let long = super::safe_shutdown_detail(&"x".repeat(4096));
        assert!(long.chars().count() <= 512, "got {}", long.chars().count());
    }

    #[test]
    fn display_reports_answer_their_requests() {
        let mut session = FakeSession::default();
        let events = run(
            vec![WorkerCommand::ContextReport, WorkerCommand::ToolsReport],
            &mut session,
            &CancelFlag::new(),
        );
        assert!(events.contains(&WorkerEvent::Report("context".to_owned())));
        assert!(events.contains(&WorkerEvent::Report("tools".to_owned())));
    }

    /// A session that behaves like a STALLED provider: every event costs
    /// `delay`, and every cancel is timestamped where it lands. The C4 evidence
    /// pack measures the two things a stall decides -- how many frames the
    /// frontend paints while nothing arrives, and how long a cancel takes to
    /// reach the session.
    struct StallingSession {
        delay: std::time::Duration,
        remaining: usize,
        prompt: Option<String>,
        cancels: std::sync::Arc<std::sync::Mutex<Vec<std::time::Instant>>>,
    }

    impl WorkerSession for StallingSession {
        fn send_prompt(&mut self, prompt: &str) -> Result<(), String> {
            self.prompt = Some(prompt.to_owned());
            Ok(())
        }
        fn poll_event(&mut self) -> Option<ToolLoopEvent> {
            if self.remaining == 0 {
                return None;
            }
            // The stall: the provider read blocks here, exactly where a real
            // one blocks, so nothing reaches the frontend while it does.
            std::thread::sleep(self.delay);
            self.remaining -= 1;
            Some(ToolLoopEvent::TextDelta { text: "x".to_owned() })
        }
        fn is_responding(&self) -> bool {
            self.remaining > 0
        }
        fn pane(&self) -> Option<crate::tui::ContextPaneData> {
            None
        }
        fn context_report(&self) -> String {
            String::new()
        }
        fn tools_report(&self) -> String {
            String::new()
        }
        fn set_model(&mut self, _model: &str) -> Result<(), String> {
            Ok(())
        }
        fn reload(&mut self) -> Result<String, String> {
            Ok(String::new())
        }
        fn turn_settled(&mut self) {}
        fn fetch_models(
            &mut self,
            _cancellation: &CancelFlag,
        ) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }
        fn domains_add(&mut self, _folder: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn domains_enable(&mut self, _id: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn domains_activate(&mut self, _id: &str) -> Result<String, String> {
            Ok(String::new())
        }
        fn status(&self) -> SessionStatus {
            SessionStatus {
                status: String::new(),
                provider: None,
                model: None,
                endpoint: None,
                protocol: String::new(),
                credential_display: None,
                credential_resolved: false,
                live_model_switchable: true,
                context_suffix: String::new(),
            }
        }
        fn cancel(&mut self) {
            self.cancels
                .lock()
                .expect("cancel log")
                .push(std::time::Instant::now());
        }
        fn enable_progress_ticks(&mut self) {}
        fn flush(&mut self) {}
    }

    #[test]
    fn cancel_reaches_a_stalled_turn_within_an_event_interval() {
        // C4 evidence: the cancel is an EXTERNAL flag polled between events, so
        // its callback latency is bounded by the event interval, not by the
        // turn. This deliberately does not claim that an already-blocked
        // provider transport is interrupted; that boundary is outside this
        // module. The numbers are printed because the point is the measurement.
        let delay = std::time::Duration::from_millis(20);
        let cancels = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut session = StallingSession {
            delay,
            remaining: 40, // ~800 ms of stalled turn
            prompt: None,
            cancels: std::sync::Arc::clone(&cancels),
        };
        let (command_tx, command_rx) =
            std::sync::mpsc::sync_channel::<WorkerCommand>(256);
        let (event_tx, event_rx) =
            std::sync::mpsc::sync_channel::<WorkerEvent>(4096);
        let cancel = CancelFlag::new();
        let thread_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            super::run_worker_loop(
                &command_rx,
                &event_tx,
                &thread_cancel,
                &mut session,
            );
        });

        command_tx
            .send(WorkerCommand::Prompt("stall".to_owned()))
            .expect("prompt");
        // Wait until the turn is genuinely running (two events drained).
        let mut drained = 0usize;
        while drained < 2 {
            match event_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(WorkerEvent::Session(_)) => drained += 1,
                Ok(_) => {}
                Err(error) => panic!("no events while stalling: {error}"),
            }
        }

        let requested = std::time::Instant::now();
        cancel.request();
        let mut landed = None;
        while requested.elapsed() < std::time::Duration::from_secs(5) {
            if let Some(at) = cancels.lock().expect("cancel log").first() {
                landed = Some(*at);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let latency = landed
            .expect("the flag reaches the stalled turn's poll seam")
            .duration_since(requested);
        println!(
            "cancel latency after a {delay:?} event interval: {latency:?} ({} events, {latency_events:.1} intervals)",
            drained,
            latency_events = latency.as_secs_f64() / delay.as_secs_f64(),
        );
        assert!(
            latency <= delay * 3,
            "the cancel must land within a few event intervals, took {latency:?}"
        );

        command_tx.send(WorkerCommand::Shutdown).expect("shutdown");
        worker.join().expect("worker thread");
    }
}
