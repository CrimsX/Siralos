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
