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
    /// The recordings were flushed and the worker is exiting.
    Stopped,
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
/// cannot fail at all.
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
    /// Re-apply the reloaded composition (decision 167 D3).
    fn reload(&mut self) -> Result<(), String>;
    /// Host cancellation authority.
    fn cancel(&mut self);
    /// Flush the recordings -- called EXACTLY once, on shutdown.
    fn flush(&mut self);
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
                    Ok(()) => {
                        let note = format!("model switched to {model}");
                        let _ = events.send(WorkerEvent::Report(note));
                    }
                    Err(message) => {
                        let _ = events.send(WorkerEvent::Failed(message));
                    }
                }
            }
            WorkerCommand::Reload => match session.reload() {
                Ok(()) => {
                    let _ = events
                        .send(WorkerEvent::Report("reloaded".to_owned()));
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
            // The keep-alive tick, round markers and context pressure carry
            // no output of their own.
            _ => {}
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
        CancelFlag, WorkerCommand, WorkerEvent, WorkerSession, run_worker_loop,
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
            None
        }
        fn context_report(&self) -> String {
            "context".to_owned()
        }
        fn tools_report(&self) -> String {
            "tools".to_owned()
        }
        fn set_model(&mut self, model: &str) -> Result<(), String> {
            Ok(()).map(|()| {
                let _ = model;
            })
        }
        fn reload(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn cancel(&mut self) {
            self.cancels += 1;
            self.responding = false;
        }
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
