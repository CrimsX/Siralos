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
//! This module is the contract only -- no thread is spawned here. C2's wiring
//! (the worker loop, the cancel flag, the single replay flush) is written
//! against it.

use siralos_core::tool::ToolLoopEvent;

/// A command from the UI thread to the worker.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerCommand {
    /// Run one prompt turn.
    Prompt(String),
    /// Cancel the active turn. The worker ALSO watches an external flag, so a
    /// blocked provider read is not the only escape.
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
    /// The recordings were flushed and the worker is exiting.
    Stopped,
}

/// The header the frontend shows: the composed status segment plus the two
/// names it is built from.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionStatus {
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
    /// The context-usage suffix the status line carries (` | ctx N/4096`), empty
    /// when the context subsystem is off. It crosses so a frontend can
    /// re-render a TRANSIENT status -- the add-flow's "fetching models..." --
    /// with the same suffix instead of dropping a readout it cannot compute.
    pub context_suffix: String,
}

/// The external cancel flag (decision 167): the UI sets it, the worker polls it
/// between events, so cancellation does not wait for a blocked read to return.
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
}

// The compile-time proof that the contract can cross threads. If a future edit
// puts an \`Rc\` (or any other \`!Send\` value) in a message, this stops compiling.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<WorkerCommand>();
    assert_send::<WorkerEvent>();
    assert_send::<CancelFlag>();
};

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
    fn fetch_models(&mut self) -> Result<Vec<String>, String>;
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
    /// Flush the recordings -- called EXACTLY once, on shutdown.
    fn flush(&mut self);
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
/// between commands AND between drained events, so a cancel need not wait for a
/// blocked read to return.
///
/// `flush` is called exactly once, on shutdown, which is decision 78's
/// single-owner rule made mechanical.
pub fn run_worker_loop<S: WorkerSession>(
    commands: &std::sync::mpsc::Receiver<WorkerCommand>,
    events: &std::sync::mpsc::Sender<WorkerEvent>,
    cancel: &CancelFlag,
    session: &mut S,
) {
    // The startup pane snapshot goes FIRST. The frontend used to BUILD it
    // before its loop and cannot any more -- the metrics and the history live
    // here (decision 167 D1) -- and a frontend that waits for the header must
    // already hold the pane it will draw with that header. Sending it after the
    // header would make the first frame a race.
    if let Some(pane) = session.pane() {
        let _ = events.send(WorkerEvent::Pane(pane));
    }
    // The frontend cannot derive its header any more: it arrives before any
    // command, and again whenever a command moves the composition under it.
    let _ = events.send(WorkerEvent::Ready(session.status()));
    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::Prompt(prompt) => {
                cancel.clear();
                match session.send_prompt(&prompt) {
                    Ok(()) => {
                        while let Some(event) = session.poll_event() {
                            let _ = events.send(WorkerEvent::Session(event));
                            if cancel.is_requested() {
                                session.cancel();
                            }
                            if let Some(pane) = session.pane() {
                                let _ = events.send(WorkerEvent::Pane(pane));
                            }
                        }
                        session.turn_settled();
                        // The settled pane: the demand loop has just run, so
                        // this is the first snapshot that can show its effect.
                        if let Some(pane) = session.pane() {
                            let _ = events.send(WorkerEvent::Pane(pane));
                        }
                        let _ = events.send(WorkerEvent::TurnFinished);
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                        let _ = events.send(WorkerEvent::TurnFinished);
                    }
                }
            }
            WorkerCommand::Cancel => session.cancel(),
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
            WorkerCommand::ModelsFetch => match session.fetch_models() {
                Ok(models) => {
                    let _ = events.send(WorkerEvent::Models(models));
                }
                Err(message) => {
                    let _ = events.send(WorkerEvent::Failed(message));
                }
            },
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
                }
            },
            WorkerCommand::Shutdown => {
                if session.is_responding() {
                    session.cancel();
                }
                session.flush();
                let _ = events.send(WorkerEvent::Stopped);
                return;
            }
        }
    }
    // The command channel closed without a Shutdown: flush once anyway, so a
    // vanished UI cannot lose the recordings silently.
    session.flush();
    let _ = events.send(WorkerEvent::Stopped);
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
/// Dropping it does NOT stop the worker: `shutdown` sends the command and joins,
/// which is the only path that guarantees the recordings flushed before the
/// process restores the terminal.
pub struct WorkerHandle {
    commands: std::sync::mpsc::Sender<WorkerCommand>,
    events: std::sync::mpsc::Receiver<WorkerEvent>,
    cancel: CancelFlag,
    join: Option<std::thread::JoinHandle<()>>,
}

impl WorkerHandle {
    /// Send one command (`false` when the worker is already gone).
    pub fn send(&self, command: WorkerCommand) -> bool {
        self.commands.send(command).is_ok()
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

    /// Everything the worker has already produced, without blocking.
    pub fn try_recv_all(&self) -> Vec<WorkerEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }

    /// The shared cancel flag the worker polls between events.
    #[must_use]
    pub fn cancel_flag(&self) -> &CancelFlag {
        &self.cancel
    }

    /// Stop the worker and WAIT for it: the recordings are flushed before this
    /// returns (decision 78's single-owner rule, decision 167 step 4).
    pub fn shutdown(mut self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
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
}

impl WorkerSource {
    /// Wrap a handle (from \`spawn_worker\`) as a drainable source.
    #[must_use]
    pub fn new(handle: WorkerHandle) -> Self {
        Self {
            handle,
            ready: std::collections::VecDeque::new(),
            pending: Vec::new(),
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

    /// The next worker event, WAITING for it (C2 step 3).
    ///
    /// Only for the one moment a frontend can afford to wait with nothing to
    /// draw: the startup handshake, before its terminal exists. Inside a turn
    /// the frontend uses `wait`, because its clock must keep running.
    pub fn recv(&self) -> Option<WorkerEvent> {
        self.handle.recv()
    }

    /// Stop the worker and WAIT for it: the recordings are flushed before this
    /// returns (decision 78's single-owner rule, decision 167 step 4).
    pub fn shutdown(self) {
        self.handle.shutdown();
    }
}

impl EventSource for WorkerSource {
    fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        for event in self.handle.try_recv_all() {
            match event {
                WorkerEvent::Session(inner) => self.ready.push_back(inner),
                other => self.pending.push(other),
            }
        }
        self.ready.pop_front()
    }

    fn cancel(&mut self) {
        // Both halves: the flag reaches the worker BETWEEN events (it does not
        // wait for a blocked read to return), the command reaches it when it is
        // not in a turn.
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
    let (command_tx, command_rx) = std::sync::mpsc::channel::<WorkerCommand>();
    let (event_tx, event_rx) = std::sync::mpsc::channel::<WorkerEvent>();
    let cancel = CancelFlag::new();
    let thread_cancel = cancel.clone();
    let join = std::thread::spawn(move || {
        let options = crate::interactive::InteractiveOptions {
            config_path: config_path.as_deref(),
            workspace_root: workspace_root.as_deref(),
        };
        match crate::interactive::compose_session(options) {
            Ok(mut session) => {
                session.enable_progress_ticks();
                run_worker_loop(
                    &command_rx,
                    &event_tx,
                    &thread_cancel,
                    &mut session,
                );
            }
            Err(error) => {
                let _ = event_tx.send(WorkerEvent::Failed(error.to_string()));
                let _ = event_tx.send(WorkerEvent::Stopped);
            }
        }
    });
    WorkerHandle {
        commands: command_tx,
        events: event_rx,
        cancel,
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
            let safe = crate::sanitize::sanitize_for_display(&text);
            writer.write_all(safe.as_bytes())?;
        }
        WorkerEvent::Failed(message) => {
            writer.write_all(sanitizer.flush().as_bytes())?;
            let safe = crate::sanitize::sanitize_for_display(&message);
            let line = format!("Worker failed: {safe}\n");
            writer.write_all(line.as_bytes())?;
        }
        WorkerEvent::TurnFinished | WorkerEvent::Stopped => {}
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
        pub(crate) events: std::sync::mpsc::Sender<WorkerEvent>,
        pub(crate) commands: std::sync::mpsc::Receiver<WorkerCommand>,
        pub(crate) cancel: CancelFlag,
    }

    /// Shared with the drain test in `interactive`, which drives the real
    /// drain from this scripted worker.
    pub(crate) fn scripted() -> Scripted {
        let (command_tx, commands) = std::sync::mpsc::channel();
        let (events, event_rx) = std::sync::mpsc::channel();
        let cancel = CancelFlag::new();
        let handle = WorkerHandle {
            commands: command_tx,
            events: event_rx,
            cancel: cancel.clone(),
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
        assert!(s.cancel.is_requested(), "the flag reaches a blocked read");
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
        let worker = super::spawn_worker(Some(dir.clone()), None);
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
        CancelFlag, SessionStatus, WorkerCommand, WorkerEvent, WorkerSession,
        run_worker_loop,
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
        models: Vec<String>,
        reload_report: Option<String>,
        reload_refusal: Option<String>,
        settled: usize,
        pane: Option<crate::tui::ContextPaneData>,
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
            if next.is_none() {
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
            self.responding = false;
        }
        fn turn_settled(&mut self) {
            self.settled += 1;
        }
        fn fetch_models(&mut self) -> Result<Vec<String>, String> {
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
                context_suffix: String::new(),
            }
        }
        fn enable_progress_ticks(&mut self) {}
        fn flush(&mut self) {
            self.flushes += 1;
        }
    }

    fn run(
        commands: Vec<WorkerCommand>,
        session: &mut FakeSession,
        cancel: &CancelFlag,
    ) -> Vec<WorkerEvent> {
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
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
}
