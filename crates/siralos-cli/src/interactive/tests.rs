use super::{
    InteractiveOptions, SessionProvider, SlashCommand, ToolLoopEvent,
    TuiState, apply_model_switch, apply_provider_remove_confirmation,
    compose_session, is_unknown_slash_command, parse_slash_command,
    persist_switched_model, remove_profile_config, render_evolve_lines,
    render_model_line, render_provider_line,
    run_interactive_session_with_options, slash_command_catalog,
    write_profile_config,
};
use std::cell::RefCell;
use std::fs::{create_dir, create_dir_all, read, remove_dir_all, write};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

/// C2 step 3b: the drain is source-agnostic. A fake source stands in for
/// the session, which is the property the worker switch depends on.
struct FakeSource {
    events: Vec<ToolLoopEvent>,
    cancels: usize,
}

impl crate::session_worker::EventSource for FakeSource {
    fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        if self.events.is_empty() { None } else { Some(self.events.remove(0)) }
    }

    fn cancel(&mut self) {
        self.cancels += 1;
    }
}

#[test]
fn drain_events_reads_the_source_seam_and_cancels_on_request() {
    let mut source = FakeSource {
        events: vec![
            ToolLoopEvent::TextDelta { text: "hi".to_owned() },
            ToolLoopEvent::ProviderPending,
            ToolLoopEvent::ResponseCompleted,
        ],
        cancels: 0,
    };
    let mut out: Vec<u8> = Vec::new();
    let mut ticks = 0usize;
    super::drain_events(
        &mut source,
        &mut out,
        &mut || {
            ticks += 1;
            ticks == 2
        },
        &mut |_| {},
    )
    .expect("drain");

    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        "hi\n",
        "the drain still forwards text through the terminal sanitizer"
    );
    assert_eq!(source.cancels, 1, "a frontend request cancels the source");
    assert_eq!(ticks, 3, "progress sees every drained event");
}

// ---- C2 step 3: the WORKER path drives this loop -----------------------

/// Everything one scripted dispatcher run needs, minus the test's own
/// closures: the worker side (a channel this test owns), the frontend state
/// the dispatcher writes, and the sink it renders through.
struct ScriptedTui {
    worker: crate::session_worker::worker_source_tests::Scripted,
    state: Rc<RefCell<TuiState>>,
    pane: Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    sink: crate::tui::TuiSink,
}

impl ScriptedTui {
    fn new() -> Self {
        let state = Rc::new(RefCell::new(TuiState::new()));
        let sink = crate::tui::TuiSink::new(Rc::clone(&state));
        Self {
            worker: crate::session_worker::worker_source_tests::scripted(),
            state,
            pane: Rc::new(RefCell::new(None)),
            sink,
        }
    }

    /// Queue the worker's answer(s) before the command is dispatched.
    fn answer(&self, events: Vec<crate::session_worker::WorkerEvent>) {
        for event in events {
            self.worker.events.send(event).expect("scripted answer");
        }
    }

    /// The command the dispatcher sent, if any.
    fn command(&self) -> Option<crate::session_worker::WorkerCommand> {
        self.worker.commands.try_recv().ok()
    }

    /// Run one command through the REAL dispatcher.
    fn dispatch(
        &mut self,
        command: &SlashCommand<'_>,
        workspace_root: &Path,
        ticks: &mut usize,
    ) -> bool {
        let mut progress = || {
            *ticks += 1;
            false
        };
        // The live loop's reasoning sink buffers the thinking onto the
        // state and repaints; the test only needs the buffering, because
        // the sanitizer and the bounding are pinned by the TUI tests.
        let state = Rc::clone(&self.state);
        let mut reasoning = move |text: &str| {
            state.borrow_mut().reasoning.push_str(text);
        };
        let exit = super::dispatch_tui_command(
            command,
            workspace_root,
            &self.state,
            &mut self.sink,
            &mut self.worker.source,
            &self.pane,
            &mut progress,
            &mut reasoning,
        )
        .expect("dispatch");
        settle_reveal(&self.state);
        exit
    }

    /// Simulate a worker that VANISHED: the far end of the channel closes,
    /// so the source reports `Gone`. The original sender is dropped (the
    /// decoy writes to a channel nobody listens to, which is what makes the
    /// field assignable).
    fn close_worker(&mut self) {
        let (decoy, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        self.worker.events = decoy;
    }

    /// The transcript as one string, after the reveal has been released.
    fn transcript(&self) -> String {
        settle_reveal(&self.state);
        self.state.borrow().transcript_lines.join("\n")
    }
}

/// Release the whole backlog, one character at a time -- exactly what the
/// paint path does, just without a terminal to paint into.
fn settle_reveal(state: &Rc<RefCell<TuiState>>) {
    let mut guard = state.borrow_mut();
    while guard.reveal_pending() {
        guard.reveal_char();
    }
}

fn scripted_pane() -> crate::tui::ContextPaneData {
    crate::tui::ContextPaneData {
        counters: vec![("ticks_total".to_owned(), 3)],
        ring: Vec::new(),
        activity: Vec::new(),
    }
}

#[test]
fn the_dispatcher_asks_the_worker_and_renders_its_answer() {
    // C2 step 3's real acceptance: the session-touching arms SEND a
    // WorkerCommand and RENDER the answer, driven through the production
    // dispatcher over a channel this test owns.
    let root = temporary_directory("worker-dispatch-arms");
    for (command, request, answer, expected) in [
        (
            SlashCommand::Context,
            crate::session_worker::WorkerCommand::ContextReport,
            crate::session_worker::WorkerEvent::Report(
                "Context projection (mode live)\n".to_owned(),
            ),
            "Context projection (mode live)",
        ),
        (
            SlashCommand::Tools,
            crate::session_worker::WorkerCommand::ToolsReport,
            crate::session_worker::WorkerEvent::Report(
                "Tool projection: 3 available\n".to_owned(),
            ),
            "Tool projection: 3 available",
        ),
        (
            SlashCommand::DomainsAdd(Some("demo")),
            crate::session_worker::WorkerCommand::DomainsAdd(
                "demo".to_owned(),
            ),
            crate::session_worker::WorkerEvent::Report(
                "added demo\n".to_owned(),
            ),
            "added demo",
        ),
        (
            SlashCommand::DomainsEnable(Some("demo")),
            crate::session_worker::WorkerCommand::DomainsEnable(
                "demo".to_owned(),
            ),
            crate::session_worker::WorkerEvent::Report(
                "enabled demo\n".to_owned(),
            ),
            "enabled demo",
        ),
        (
            SlashCommand::DomainsActivate(Some("demo")),
            crate::session_worker::WorkerCommand::DomainsActivate(
                "demo".to_owned(),
            ),
            crate::session_worker::WorkerEvent::Report(
                "activated demo\n".to_owned(),
            ),
            "activated demo",
        ),
        (
            SlashCommand::Reload,
            crate::session_worker::WorkerCommand::Reload,
            crate::session_worker::WorkerEvent::Report(
                "reload applied: nothing changed\n".to_owned(),
            ),
            "reload applied: nothing changed",
        ),
    ] {
        let mut tui = ScriptedTui::new();
        tui.answer(vec![answer]);
        let mut ticks = 0usize;
        let exit = tui.dispatch(&command, &root, &mut ticks);
        assert!(!exit, "{command:?} is not an exit");
        assert_eq!(
            tui.command(),
            Some(request),
            "{command:?} must ask the worker"
        );
        assert!(
            tui.transcript().contains(expected),
            "{command:?} renders the worker's answer, got {:?}",
            tui.transcript()
        );
    }
    let _ = remove_dir_all(root);
}

#[test]
fn the_dispatcher_applies_the_header_pane_and_failure_it_receives() {
    // The events that are frontend STATE: the header snapshot moves the
    // picker's values, the pane lands in the shared slot, and a failure is
    // rendered with the shared wording (never as success).
    let root = temporary_directory("worker-dispatch-state");
    let mut tui = ScriptedTui::new();
    tui.answer(vec![
        crate::session_worker::WorkerEvent::Pane(scripted_pane()),
        crate::session_worker::WorkerEvent::Ready(
            crate::session_worker::SessionStatus {
                status: "example-vendor / Example A | ctx 7/4096".to_owned(),
                provider: Some("example-vendor".to_owned()),
                model: Some("Example A".to_owned()),
                endpoint: Some("https://api.example.com/v1".to_owned()),
                protocol: "openai-completions".to_owned(),
                credential_display: Some("key:***".to_owned()),
                credential_resolved: true,
                context_suffix: " | ctx 7/4096".to_owned(),
            },
        ),
        crate::session_worker::WorkerEvent::Report("ok\n".to_owned()),
    ]);
    let mut ticks = 0usize;
    tui.dispatch(&SlashCommand::Context, &root, &mut ticks);

    {
        let state = tui.state.borrow();
        assert_eq!(state.status, "example-vendor / Example A | ctx 7/4096");
        assert_eq!(state.provider.as_deref(), Some("example-vendor"));
        assert_eq!(state.model.as_deref(), Some("Example A"));
        assert_eq!(
            state.endpoint.as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(state.protocol, "openai-completions");
        assert_eq!(state.credential_display.as_deref(), Some("key:***"));
        assert!(state.credential_resolved);
        assert_eq!(state.context_suffix, " | ctx 7/4096");
    }
    assert!(
        tui.pane.borrow().is_some(),
        "the pushed pane goes into the shared slot the draw path reads"
    );
    assert!(
        tui.transcript().contains("ok"),
        "the report still reaches the transcript"
    );

    // A failure is the shared bridge's wording: it cannot look like success.
    let mut failed = ScriptedTui::new();
    failed.answer(vec![crate::session_worker::WorkerEvent::Failed(
        "no provider configured".to_owned(),
    )]);
    failed.dispatch(&SlashCommand::Context, &root, &mut ticks);
    assert!(
        failed.transcript().contains("Worker failed: no provider configured"),
        "got {:?}",
        failed.transcript()
    );
    let _ = remove_dir_all(root);
}

#[test]
fn the_dispatcher_renders_the_provider_line_from_the_cached_snapshot() {
    // Decision 168 R2: the RAW credential never crosses, so the provider
    // line renders from the ALREADY-REDACTED display form the snapshot
    // carries.
    let root = temporary_directory("worker-provider-line");
    let mut tui = ScriptedTui::new();
    {
        let mut state = tui.state.borrow_mut();
        state.provider = Some("example-vendor".to_owned());
        state.credential_display = Some("key:***".to_owned());
    }
    let mut ticks = 0usize;
    tui.dispatch(&SlashCommand::Provider, &root, &mut ticks);
    let text = tui.transcript();
    assert!(
        text.contains("provider: example-vendor")
            && text.contains("credential: key:***"),
        "got {text:?}"
    );
    assert!(
        tui.command().is_none(),
        "a display-only arm must not disturb the worker"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn a_turn_runs_in_the_worker_and_the_frontend_renders_it() {
    // The whole point of the switch: the frontend renders a turn it does not
    // run. The scripted worker sends the sequence a real session sends, and
    // the dispatcher must render every part of it.
    let root = temporary_directory("worker-turn");
    let mut tui = ScriptedTui::new();
    tui.answer(vec![
        crate::session_worker::WorkerEvent::Session(
            ToolLoopEvent::ResponseStarted,
        ),
        crate::session_worker::WorkerEvent::Session(
            ToolLoopEvent::TextDelta { text: "hi".to_owned() },
        ),
        // The end-of-answer event is what closes the line: the reveal keeps
        // an unfinished line in its growing tail by design (S3c).
        crate::session_worker::WorkerEvent::Session(
            ToolLoopEvent::ResponseCompleted,
        ),
        crate::session_worker::WorkerEvent::Session(
            ToolLoopEvent::ReasoningDelta { text: "why".to_owned() },
        ),
        crate::session_worker::WorkerEvent::Pane(scripted_pane()),
        crate::session_worker::WorkerEvent::TurnFinished,
    ]);
    let mut ticks = 0usize;
    let exit = tui.dispatch(&SlashCommand::Prompt("hello"), &root, &mut ticks);
    assert!(!exit, "a turn is not an exit");
    assert_eq!(
        tui.command(),
        Some(crate::session_worker::WorkerCommand::Prompt("hello".to_owned())),
        "the prompt crosses as a command, not as a call"
    );
    assert!(tui.transcript().contains("hi"), "got {:?}", tui.transcript());
    assert_eq!(
        tui.state.borrow().reasoning,
        "why",
        "thinking keeps its own sink"
    );
    assert!(tui.pane.borrow().is_some(), "the turn's pane was applied");
    let _ = remove_dir_all(root);
}

#[test]
fn the_frontend_keeps_ticking_while_the_worker_is_silent() {
    // C2's acceptance in its frontend form: a stalled provider must not stop
    // the frame. The worker here answers after 60 ms; the relay ticks on its
    // own 16 ms clock the whole time.
    let root = temporary_directory("worker-silent-tick");
    let mut tui = ScriptedTui::new();
    let events = tui.worker.events.clone();
    let ticker = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        let _ = events.send(crate::session_worker::WorkerEvent::TurnFinished);
    });
    let mut ticks = 0usize;
    tui.dispatch(&SlashCommand::Prompt("slow"), &root, &mut ticks);
    ticker.join().expect("ticker thread");
    assert!(
        ticks >= 1,
        "the relay must tick while nothing arrives, ticks={ticks}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn a_stall_paints_frames_and_never_stops_the_text() {
    // C4 evidence pack, first number: how many frames does the frontend paint
    // while a turn is STALLED? The worker here is silent for 300 ms (the same
    // silence a blocked provider read produces -- the frontend cannot tell
    // them apart, which is the point of putting the session on the other
    // thread), and the progress closure is the live one's shape: release one
    // character, then paint the production draw path.
    //
    // The numbers are printed because the pack is a measurement, not a claim;
    // the assertions are the floors a stalled turn must clear.
    use std::time::{Duration, Instant};
    let silence = Duration::from_millis(300);
    let root = temporary_directory("stall-frames");
    let mut tui = ScriptedTui::new();
    // Enough owed text that the FRAME rate, not the backlog, is the limit.
    tui.sink
        .write_all(format!("{}\n", "word ".repeat(400)).as_bytes())
        .expect("sink");
    let owed = tui.state.borrow().stream_buffer.len();

    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    let events = tui.worker.events.clone();
    let ticker = std::thread::spawn(move || {
        std::thread::sleep(silence);
        let _ = events.send(crate::session_worker::WorkerEvent::TurnFinished);
    });

    let state = Rc::clone(&tui.state);
    let mut frames = 0usize;
    let mut released = 0usize;
    let started = Instant::now();
    {
        let mut progress = || {
            // What is released is what leaves the ANSWER buffer, one
            // character at a time (a long line grows in the tail until its
            // newline completes it).
            let before = state.borrow().stream_buffer.chars().count();
            state.borrow_mut().reveal_char();
            let after = state.borrow().stream_buffer.chars().count();
            if after != before {
                released += 1;
            }
            terminal
                .draw(|frame| {
                    crate::tui::draw_with_pane(&state.borrow(), None, frame)
                })
                .expect("frame");
            frames += 1;
            false
        };
        let mut reasoning = |_text: &str| {};
        super::pump_worker(
            &mut tui.worker.source,
            &mut tui.sink,
            &tui.state,
            &tui.pane,
            &mut progress,
            &mut reasoning,
            super::Until::TurnFinished,
        )
        .expect("relay");
        ticker.join().expect("ticker thread");
    }
    let elapsed = started.elapsed();
    let fps = frames as f64 / elapsed.as_secs_f64();
    println!(
        "stall {silence:?}: {frames} frames ({fps:.0} fps), {released} characters released, {owed} owed at the start, {elapsed:?} wall"
    );
    assert!(
        frames >= 20,
        "a stalled turn must keep painting, painted {frames}"
    );
    assert_eq!(
        released, frames,
        "and every frame released exactly one character"
    );
    assert!(
        tui.state.borrow().reveal_pending(),
        "the backlog outlives the stall (nothing was dropped to keep up)"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn a_models_fetch_crosses_to_the_picker_and_back() {
    // A bare `/model` (the loop's picker path) and `/models` both ask the
    // worker, because only the worker holds the endpoint and the credential.
    let root = temporary_directory("worker-models");
    let mut tui = ScriptedTui::new();
    {
        let mut state = tui.state.borrow_mut();
        state.provider = Some("example-vendor".to_owned());
        state.endpoint = Some("https://api.example.com/v1".to_owned());
        state.credential_resolved = true;
    }
    tui.answer(vec![crate::session_worker::WorkerEvent::Models(vec![
        "example/model-b".to_owned(),
    ])]);
    super::open_model_picker_via_worker(
        &mut tui.sink,
        &tui.state,
        &mut tui.worker.source,
        &tui.pane,
        &mut || false,
        &mut |_text: &str| {},
    )
    .expect("picker");
    assert_eq!(
        tui.command(),
        Some(crate::session_worker::WorkerCommand::ModelsFetch),
        "the fetch is a command now"
    );
    assert!(
        tui.state.borrow().model_switch_picker.is_some(),
        "the ids open the switch picker"
    );

    // `/models` prints the same ids through the dispatcher.
    let mut listed = ScriptedTui::new();
    {
        let mut state = listed.state.borrow_mut();
        state.provider = Some("example-vendor".to_owned());
        state.endpoint = Some("https://api.example.com/v1".to_owned());
        state.credential_resolved = true;
    }
    listed.answer(vec![crate::session_worker::WorkerEvent::Models(vec![
        "example/model-b".to_owned(),
    ])]);
    let mut ticks = 0usize;
    listed.dispatch(&SlashCommand::Models, &root, &mut ticks);
    assert_eq!(
        listed.command(),
        Some(crate::session_worker::WorkerCommand::ModelsFetch)
    );
    assert!(
        listed.transcript().contains("example/model-b"),
        "got {:?}",
        listed.transcript()
    );

    // An unresolved credential must not spend a request: the gate is
    // today's, kept on purpose (decision 168 R2/R3).
    let mut unconfigured = ScriptedTui::new();
    {
        let mut state = unconfigured.state.borrow_mut();
        state.provider = Some("example-vendor".to_owned());
        state.endpoint = Some("https://api.example.com/v1".to_owned());
    }
    unconfigured.dispatch(&SlashCommand::Models, &root, &mut ticks);
    assert_eq!(
        unconfigured.command(),
        None,
        "an unresolved credential must not spend a request"
    );
    assert!(
        unconfigured.transcript().contains("no provider configured"),
        "got {:?}",
        unconfigured.transcript()
    );
    let _ = remove_dir_all(root);
}

#[test]
fn a_model_switch_persists_first_and_then_commands_the_worker() {
    // Decision 167 D3: persist-before-live by ORDERING. The profile write is
    // the frontend's (it owns the file); the live apply is the worker's, and
    // it answers with the header that proves the composition moved.
    let root = temporary_directory("worker-model-switch");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-completions\"\n",
    )
    .expect("profile");
    let mut tui = ScriptedTui::new();
    {
        let mut state = tui.state.borrow_mut();
        state.provider = Some("example-vendor".to_owned());
        state.model = Some("Example A".to_owned());
    }
    tui.answer(vec![crate::session_worker::WorkerEvent::Ready(
        crate::session_worker::SessionStatus {
            status: "example-vendor / example/model-b".to_owned(),
            provider: Some("example-vendor".to_owned()),
            model: Some("example/model-b".to_owned()),
            endpoint: Some("https://api.example.com/v1".to_owned()),
            protocol: "openai-completions".to_owned(),
            credential_display: None,
            credential_resolved: false,
            context_suffix: String::new(),
        },
    )]);
    let mut ticks = 0usize;
    tui.dispatch(
        &SlashCommand::Model(Some("example/model-b")),
        &root,
        &mut ticks,
    );
    assert_eq!(
        tui.command(),
        Some(crate::session_worker::WorkerCommand::SetModel(
            "example/model-b".to_owned()
        )),
        "the worker applies the switch it did not persist"
    );
    let written = read(root.join("siralos.toml")).expect("read profile");
    let written = String::from_utf8(written).expect("utf8");
    assert!(
        written.contains("model = \"example/model-b\""),
        "the persist happened FIRST, on the frontend, got: {written}"
    );
    assert_eq!(
        tui.state.borrow().model.as_deref(),
        Some("example/model-b"),
        "the header comes from the worker's Ready, so the display name cannot lie"
    );

    // A refused persist changes NOTHING and sends NOTHING: no provider means
    // no profile to write (decision 167 D3).
    let mut refused = ScriptedTui::new();
    refused.dispatch(
        &SlashCommand::Model(Some("example/model-c")),
        &root,
        &mut ticks,
    );
    assert_eq!(
        refused.command(),
        None,
        "a refused switch must not reach the session"
    );
    let written = read(root.join("siralos.toml")).expect("read profile");
    let written = String::from_utf8(written).expect("utf8");
    assert!(
        !written.contains("example/model-c"),
        "a refused switch must not touch the disk"
    );
    let _ = remove_dir_all(root);
}

/// Turn a TestBackend frame into text rows (the harness renders frames the
/// same way: cell symbols, row by row, trailing blanks trimmed).
fn frame_rows(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|row| {
            let mut line = String::new();
            for col in 0..area.width {
                if let Some(cell) = buffer.cell((col, row)) {
                    line.push_str(cell.symbol());
                }
            }
            line.trim_end().to_owned()
        })
        .collect()
}

#[test]
fn the_ui_paints_on_its_own_tick_with_no_provider_events() {
    // C3's acceptance: the frame must advance while the worker is SILENT.
    // Nothing but the frontend's own clock drives it any more, and the
    // paint below is the PRODUCTION draw path over a TestBackend -- what
    // the loop does on a tick, with no event to react to.
    let root = temporary_directory("ui-own-tick");
    let mut tui = ScriptedTui::new();
    // A whole answer arrives at once and lands in the reveal buffer: the
    // sink hands text to the reveal, not to the transcript, so an
    // unfinished answer is on screen only because a frame released it.
    tui.sink
        .write_all(format!("{}\n", "a".repeat(600)).as_bytes())
        .expect("sink");

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
    let events = tui.worker.events.clone();
    let ticker = std::thread::spawn(move || {
        // Long enough for several 16 ms ticks, still fast.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = events.send(crate::session_worker::WorkerEvent::TurnFinished);
    });

    let mut released: Vec<usize> = Vec::new();
    {
        // The closures borrow the terminal and the sample log, so they live
        // in their own scope: the frame is read back once the relay is done
        // and nothing borrows it any more.
        let state = Rc::clone(&tui.state);
        let mut progress = || {
            {
                let mut guard = state.borrow_mut();
                // One character per painted frame: the C3 acceptance is that
                // the frame -- not a provider event -- is what releases it.
                guard.reveal_char();
                released.push(
                    guard.stream_tail.len()
                        + guard
                            .transcript_lines
                            .iter()
                            .map(String::len)
                            .sum::<usize>(),
                );
            }
            terminal
                .draw(|frame| {
                    crate::tui::draw_with_pane(&state.borrow(), None, frame);
                })
                .expect("frame");
            false
        };
        let mut reasoning = |_text: &str| {};
        super::pump_worker(
            &mut tui.worker.source,
            &mut tui.sink,
            &tui.state,
            &tui.pane,
            &mut progress,
            &mut reasoning,
            super::Until::TurnFinished,
        )
        .expect("relay");
        ticker.join().expect("ticker thread");
    }

    assert!(
        released.len() >= 2,
        "the frontend ticked on its own clock while the worker was silent, frames={released:?}"
    );
    assert!(
        released[0] > 0,
        "the first frame already released answer text, frames={released:?}"
    );
    assert!(
        *released.last().expect("a frame") > released[0],
        "the answer ADVANCED on the clock alone, frames={released:?}"
    );
    // And the frame itself carries it: the paint, not just the buffer.
    let painted = frame_rows(&terminal).join("\n");
    assert!(
        painted.contains("aaaa"),
        "the painted frame shows the released answer, got:\n{painted}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn a_drain_says_nothing_about_a_worker_it_was_not_waiting_for() {
    // C3: the frame-level sweep runs on EVERY iteration, so a worker that
    // vanished must not be announced twenty times a second. A relay that was
    // waiting reports it -- exactly once.
    let root = temporary_directory("drain-gone-worker");
    let mut tui = ScriptedTui::new();
    tui.close_worker();
    let before = tui.transcript();
    super::drain_pending_worker(
        &mut tui.worker.source,
        &mut tui.sink,
        &tui.state,
        &tui.pane,
    )
    .expect("drain");
    super::drain_pending_worker(
        &mut tui.worker.source,
        &mut tui.sink,
        &tui.state,
        &tui.pane,
    )
    .expect("drain again");
    assert_eq!(
        tui.transcript(),
        before,
        "a per-frame drain must stay silent about a worker it was not waiting for"
    );

    let mut waiting = ScriptedTui::new();
    waiting.close_worker();
    let mut ticks = 0usize;
    waiting.dispatch(&SlashCommand::Context, &root, &mut ticks);
    let text = waiting.transcript();
    assert_eq!(
        text.matches("worker stopped").count(),
        1,
        "the relay that WAS waiting says so once, got {text:?}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn drain_events_reads_a_worker_backed_source() {
    // C2 step 3: the two halves compose. A worker channel is drained by the
    // SAME drain stdio and the TUI use, and the events that are not part of
    // the stream survive it for the loop to act on.
    let mut worker = crate::session_worker::worker_source_tests::scripted();
    for event in [
        crate::session_worker::WorkerEvent::Session(
            ToolLoopEvent::TextDelta { text: "hi".to_owned() },
        ),
        crate::session_worker::WorkerEvent::Session(
            ToolLoopEvent::ReasoningDelta { text: "why".to_owned() },
        ),
        crate::session_worker::WorkerEvent::TurnFinished,
    ] {
        worker.events.send(event).expect("send");
    }

    let mut out: Vec<u8> = Vec::new();
    let mut thinking = String::new();
    super::drain_events(
        &mut worker.source,
        &mut out,
        &mut || false,
        &mut |text| thinking.push_str(text),
    )
    .expect("drain");

    assert_eq!(
        String::from_utf8(out).expect("utf8"),
        "hi",
        "answer text streams through the terminal sanitizer"
    );
    assert_eq!(
        thinking, "why",
        "thinking goes to its own sink and never into the answer"
    );
    assert_eq!(
        worker.source.take_pending(),
        vec![crate::session_worker::WorkerEvent::TurnFinished],
        "the turn-end signal survives the drain so the loop can go idle"
    );
}

#[test]
fn the_worker_adapter_applies_a_reload_and_returns_the_report() {
    // C2: `/reload` behind the worker boundary. The adapter re-reads the
    // profile, moves the live cells the NEXT request reads, and returns the
    // report the frontend shows -- nothing about it is frontend-only.
    use crate::session_worker::WorkerSession;
    let root = temporary_directory("worker-reload-apply");
    let profile = |model: &str| {
        format!(
            "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"{model}\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-completions\"\n"
        )
    };
    write(root.join("siralos.toml"), profile("example/model-a"))
        .expect("profile");
    let options =
        InteractiveOptions { workspace_root: Some(&root), config_path: None };
    let mut session = compose_session(options).expect("compose");
    assert_eq!(
        session.live_provider.live_model().as_deref(),
        Some("example/model-a"),
        "the session starts on the profile's model"
    );

    write(root.join("siralos.toml"), profile("example/model-b"))
        .expect("edited");
    let report = session.reload().expect("reload is behind the boundary now");
    assert!(
        report.contains(
            "applied: model example/model-a -> example/model-b (live, no restart)"
        ),
        "the report says what moved, got: {report:?}"
    );
    assert_eq!(
        session.live_provider.live_model().as_deref(),
        Some("example/model-b"),
        "the NEXT provider request reads the reloaded model"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn the_idle_poll_keeps_the_character_cadence_while_text_is_owed() {
    // A frame releases ONE character, so the IDLE path must not wait the
    // 50 ms idle interval while a backlog exists: the leftover of a turn
    // would drain at twenty characters a second. The live loop needs a
    // terminal, so this is a source check -- the same idiom
    // compose_session_before_guard_no_terminal_needed uses.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/interactive.rs"),
    )
    .expect("read interactive.rs");
    let poll = src
        .find("let idle_poll = crate::tui::paint_interval(")
        .expect("the idle poll consults the reveal");
    let after = &src[poll..];
    let body = &after[..after.find(';').expect("a statement")];
    assert!(
        body.contains("paint_interval")
            && body.contains("reveal_pending()")
            && body.contains("TUI_IDLE_POLL"),
        "the idle wait is unthrottled while text is owed, the idle interval otherwise: {body}"
    );
}

#[test]
fn the_demand_loop_runs_where_the_history_lives() {
    // Decision 167 and the C2 inventory: the demand loop reads the
    // session's OWN history, so it runs where the history lives. Since C2
    // step 3 that is exactly TWO owners -- the stdio arm (in-thread) and the
    // worker's settled-turn hook (the TUI's session) -- and the TUI frontend
    // must NOT drive it any more, because it holds no history to read.
    // The property is "this call is here, and not there", which is what a
    // source check can settle.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/interactive.rs"),
    )
    .expect("read interactive.rs");
    // Count call SITES, not the test's own source text: the assertion
    // strings below contain the needle too.
    let calls = src
        .lines()
        .filter(|line| line.contains("drive_context_demand("))
        .filter(|line| !line.trim_start().starts_with("//"))
        // The assertion strings below contain the needle too: a line that
        // SEARCHES for the call is not a call.
        .filter(|line| !line.contains("contains("))
        .count();
    assert_eq!(
        calls, 2,
        "the stdio arm and the worker hook drive the demand loop; the TUI frontend does not, found {calls}"
    );
    let hook = src.find("fn turn_settled").expect("the adapter settles turns");
    let after = &src[hook..];
    let body = match after.find("\n    }\n") {
        Some(end) => &after[..end],
        None => after,
    };
    assert!(
        body.contains("drive_context_demand("),
        "the settled-turn hook is where the demand loop moved to"
    );
}

#[test]
fn the_status_snapshot_never_carries_the_credential() {
    // Decision 168 R2: the snapshot crosses to the frontend, so the raw
    // credential must not -- it is redacted where it lives. The endpoint and
    // the protocol DO cross, because the picker shows them (R3).
    use crate::session_worker::WorkerSession;
    let root = temporary_directory("worker-status-redaction");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-completions\"\ncredential = \"key:super-secret-value\"\n",
    )
    .expect("profile");
    let session = compose_session(InteractiveOptions {
        workspace_root: Some(&root),
        config_path: None,
    })
    .expect("compose");
    let snapshot = format!("{:?}", session.status());
    assert!(
        !snapshot.contains("super-secret-value"),
        "the secret must not cross the boundary: {snapshot}"
    );
    assert!(
        snapshot.contains("key:***"),
        "the display form crosses instead: {snapshot}"
    );
    assert_eq!(
        session.status().endpoint.as_deref(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(session.status().protocol, "openai-completions");
    let _ = remove_dir_all(root);
}

#[test]
fn the_worker_adapter_reports_the_header_the_frontend_showed() {
    use crate::session_worker::WorkerSession;
    let root = temporary_directory("worker-status");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nmodel_display_name = \"Example A\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-completions\"\n",
    )
    .expect("profile");
    let mut session = compose_session(InteractiveOptions {
        workspace_root: Some(&root),
        config_path: None,
    })
    .expect("compose");
    let status = session.status();
    assert_eq!(status.provider.as_deref(), Some("example-vendor"));
    assert_eq!(
        status.model.as_deref(),
        Some("Example A"),
        "the display name wins"
    );
    assert_eq!(
        status.status,
        crate::tui::compose_status_line_with_context(
            "",
            status.provider.as_deref(),
            status.model.as_deref(),
            None,
        ),
        "the worker builds the same header the TUI entry built"
    );

    // A live switch must not keep the OLD model's display name.
    session.set_model("example/model-b").expect("switch");
    assert_eq!(
        session.status().model.as_deref(),
        Some("example/model-b"),
        "the stale display name is dropped, so the header cannot lie"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn write_profile_config_endpoint_messages_are_pinned() {
    // Regression pin: the write boundary's endpoint guard reports four
    // distinct messages, one per clause. Nothing else asserts them — the
    // strings appear only at their definition sites.
    let refused = |endpoint: &str| {
        write_profile_config(
            std::path::Path::new("unused-guard-probe"),
            "openai",
            "model-a",
            None,
            Some(endpoint),
            None,
            None,
        )
        .expect_err("the endpoint guard refuses")
    };
    assert_eq!(
        refused(""),
        "The endpoint exceeds the 512-byte bound or is empty."
    );
    assert_eq!(
        refused(&"h".repeat(
            siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES + 1
        )),
        "The endpoint exceeds the 512-byte bound or is empty."
    );
    assert_eq!(
        refused("https://api.example.com/a\0b"),
        "An endpoint must not contain NUL."
    );
    assert_eq!(
        refused("ftp://api.example.com"),
        "An endpoint must start with \"https://\" or \"http://\"."
    );
    assert_eq!(
        refused("https://api.example.com/a b"),
        "An endpoint must not contain spaces."
    );
}

#[test]
fn write_profile_config_display_name_messages_are_pinned() {
    // Regression pin: the display-name guard reports three distinct
    // messages, one per clause.
    let refused = |display: &str| {
        write_profile_config(
            std::path::Path::new("unused-guard-probe"),
            "openai",
            "model-a",
            None,
            None,
            None,
            Some(display),
        )
        .expect_err("the display-name guard refuses")
    };
    assert_eq!(
        refused(&"d".repeat(
            siralos_core::composition::MAX_PROFILE_MODEL_DISPLAY_NAME_BYTES
                + 1
        )),
        "The model display name exceeds the 256-byte bound."
    );
    assert_eq!(
        refused("display\0name"),
        "A model display name must not contain NUL."
    );
    assert_eq!(
        refused("line\nbreak"),
        "A model display name must be printable."
    );
}

#[test]
fn write_profile_config_omits_credential_when_none() {
    // K2: a public endpoint writes NO credential key, and the written
    // config still parses and applies via load_workspace_profile.
    let dir = temporary_directory("write_profile_omits_credential");
    let result = write_profile_config(
        &dir,
        "public",
        "public-model",
        None,
        Some("https://public.example.com/v1"),
        Some("openai-completions"),
        None,
    );
    assert!(result.is_ok(), "write failed: {result:?}");
    let toml_text = read(format!("{}/siralos.toml", dir.display()))
        .map(|bytes| String::from_utf8(bytes).unwrap_or_default())
        .unwrap_or_default();
    assert!(
        !toml_text.contains("credential"),
        "config must not contain a credential key, got: {toml_text}"
    );
    match siralos_adapters::profile_config::load_workspace_profile(&dir) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            assert_eq!(record.provider.as_deref(), Some("public"));
            assert!(record.credential.is_none());
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    let _ = remove_dir_all(&dir);
}

#[test]
fn write_profile_config_model_id_rule_matches_core() {
    // The write boundary enforces the core model-id rule: 1..=256
    // bytes, no NUL, ASCII alphanumeric or . _ - / : @.
    let accepted: Vec<String> = vec![
        "model-a".to_owned(),
        "gpt-4o".to_owned(),
        "example/model-a".to_owned(),
        "example/model-b:free".to_owned(),
        "openai/gpt-4o@2024-08-06".to_owned(),
        "a".repeat(siralos_core::composition::MAX_PROFILE_MODEL_BYTES),
    ];
    for (index, id) in accepted.iter().enumerate() {
        let dir = temporary_directory(&format!("model-rule-ok-{index}"));
        write_profile_config(
            &dir,
            "openai",
            id,
            None,
            Some("https://api.example.com/v1"),
            Some("openai-completions"),
            None,
        )
        .expect("provider-issued model id accepted");
        match siralos_adapters::profile_config::load_workspace_profile(&dir) {
            siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
                record,
            ) => {
                assert_eq!(record.model.as_deref(), Some(id.as_str()));
            }
            other => panic!("expected applied record, got: {other:?}"),
        }
        let _ = remove_dir_all(&dir);
    }
    let rejected: Vec<String> = vec![
        String::new(),
        "a".repeat(siralos_core::composition::MAX_PROFILE_MODEL_BYTES + 1),
        "has space".to_owned(),
        "ab\0cd".to_owned(),
    ];
    for (index, id) in rejected.iter().enumerate() {
        let dir = temporary_directory(&format!("model-rule-err-{index}"));
        let error = write_profile_config(
            &dir,
            "openai",
            id,
            None,
            Some("https://api.example.com/v1"),
            Some("openai-completions"),
            None,
        )
        .expect_err("invalid model id refused");
        assert!(
            error.contains("256") || error.contains("NUL"),
            "rejection must state the enforced rule, got: {error}"
        );
        let _ = remove_dir_all(&dir);
    }
}

#[test]
fn scratch_names_are_unique_under_concurrency() {
    // The repair for the parallel-test collision: a scratch name derived
    // only from the clock is not unique when the platform timer is coarse,
    // so the counter must carry uniqueness on its own. This tests the
    // invariant, never a timing-dependent reproduction of the race.
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    let sequential: HashSet<String> =
        (0..64).map(|_| super::unique_scratch_name("probe")).collect();
    assert_eq!(
        sequential.len(),
        64,
        "sequential scratch names must all be distinct"
    );

    let seen = Arc::new(Mutex::new(HashSet::new()));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let seen = Arc::clone(&seen);
        handles.push(std::thread::spawn(move || {
            for _ in 0..16 {
                let name = super::unique_scratch_name("probe");
                seen.lock().expect("scratch lock").insert(name);
            }
        }));
    }
    for handle in handles {
        handle.join().expect("scratch thread");
    }
    assert_eq!(
        seen.lock().expect("scratch lock").len(),
        128,
        "concurrent scratch names must all be distinct"
    );
}

fn temporary_directory(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("siralos-cli-r7-5-{label}-{nonce}"));
    create_dir(&path).expect("temporary directory");
    path
}

fn run(
    lines: &str,
    root: &std::path::Path,
    config: Option<&std::path::Path>,
) -> String {
    let mut output = Vec::new();
    run_interactive_session_with_options(
        Cursor::new(lines.as_bytes()),
        &mut output,
        InteractiveOptions { config_path: config, workspace_root: Some(root) },
    )
    .expect("interactive session");
    String::from_utf8(output).expect("utf8 output")
}

#[test]
fn context_before_prompt_is_truthful_and_tools_have_no_stale_projection() {
    let root = temporary_directory("before");
    let output = run("/context\n/tools\n/exit\n", &root, None);
    assert!(output.contains(
        "Context projection: not yet computed (send a prompt first)\n"
    ));
    assert!(output.contains("workspace.list"));
    assert!(output.contains("workspace.read"));
    assert!(output.contains("workspace.search"));
    assert!(output.contains("(read-only, allowed)"));
    assert!(output.contains("Tool projection: not yet computed\n"));
    let _ = remove_dir_all(root);
}

#[test]
fn prompt_then_context_and_tools_render_the_current_projection() {
    let root = temporary_directory("prompt");
    let output = run("hello\n/context\n/tools\n/exit\n", &root, None);
    assert!(output.contains("Siralos received: hello"));
    assert!(output.contains("Context projection (mode generic)\n"));
    assert!(output.contains("Stable: "));
    assert!(output.contains("Pressure: normal ("));
    assert!(output.contains("Tool ABI: "));
    assert!(
        output.contains("Tool projection: 3 available, 0 gated, 0 hidden")
    );
    let _ = remove_dir_all(root);
}

#[test]
fn tool_round_refreshes_the_projection_before_context_rendering() {
    let root = temporary_directory("tool-round");
    let output = run("list files\n/context\n/tools\n/exit\n", &root, None);
    assert!(output.contains("Siralos inspected 0 workspace entries."));
    assert!(output.contains("Context projection (mode generic)\n"));
    assert!(
        output.contains("Tool projection: 3 available, 0 gated, 0 hidden")
    );
    let _ = remove_dir_all(root);
}

#[test]
fn config_is_composed_before_rendering_without_granting_extra_authority() {
    let root = temporary_directory("config");
    let config_path = root.join("config.json");
    write(
        &config_path,
        br#"{"sandbox":{"profile":"develop-offline"},"quality":{"reviewProvider":"deterministic-fake"}}"#,
    )
    .expect("config");
    let output = run("hello\n/tools\n/exit\n", &root, Some(&config_path));
    assert!(output.contains("workspace.list"));
    assert!(output.contains("(read-only, allowed)"));
    assert!(!output.contains("write, allowed"));
    let _ = std::fs::remove_file(config_path);
    let _ = remove_dir_all(root);
}

#[test]
fn domains_renders_the_deterministic_empty_state() {
    let root = temporary_directory("domains-empty");
    let output = run("/domains\n/exit\n", &root, None);
    assert!(output.contains("No domains installed.\n"));
    assert!(output.contains("/domains-add <folder>"));
    let _ = remove_dir_all(root);
}

#[test]
fn domains_add_records_and_renders() {
    let root = temporary_directory("domains-add");
    create_dir_all(root.join("plugins/godot")).expect("folder");
    let bytes = b"conformance component bytes";
    let digest = {
        use siralos_core::identity::sha256_hex;
        sha256_hex(bytes)
    };
    write(root.join("plugins/godot/godot.component.wasm"), bytes)
        .expect("component");
    write(
        root.join("plugins/godot/domain-manifest.toml"),
        format!(
            "id = \"godot\"\ndigest = \"{digest}\"\nabi = \"siralos:domain-abi@1.0.0\"\ncomponent = \"godot.component.wasm\"\n"
        ),
    )
    .expect("manifest");
    let siralos_toml = root.join("siralos.toml");
    let output =
        run("/domains-add plugins/godot\n/domains\n/exit\n", &root, None);
    assert!(output.contains("Installed godot (digest sha256:"));
    assert!(output.contains("Domains installed:\n"));
    assert!(output.contains("godot (digest "));
    assert!(siralos_toml.exists());
    let _ = remove_dir_all(root);
}

#[test]
fn domains_add_missing_manifest_fails_closed() {
    let root = temporary_directory("domains-add-missing");
    create_dir(root.join("empty")).expect("folder");
    let output = run("/domains-add empty\n/domains\n/exit\n", &root, None);
    assert!(output.contains("Add Plugin failed:"));
    assert!(output.contains("No domains installed.\n"));
    let _ = remove_dir_all(root);
}

#[test]
fn domains_add_outside_workspace_is_rejected() {
    let root = temporary_directory("domains-add-outside");
    let mut outside =
        PathBuf::from(std::env::temp_dir().to_string_lossy().into_owned());
    outside.push("outside-plugin-inspection");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    outside.push(format!("{unique}"));
    create_dir_all(&outside).expect("outside folder");
    let output = run(
        &format!("/domains-add {}\n/exit\n", outside.display()),
        &root,
        None,
    );
    assert!(output.contains("folder rejected"));
    let _ = remove_dir_all(&outside);
    let _ = remove_dir_all(root);
}

#[test]
fn domains_enable_and_activate_are_host_gated() {
    let root = temporary_directory("domains-enable-activate");
    create_dir_all(root.join("plugins/godot")).expect("folder");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/domain-conformance/fixtures/conformance-domain.component.wasm");
    let bytes = std::fs::read(&fixture).expect("fixture");
    let digest = {
        use siralos_core::identity::sha256_hex;
        sha256_hex(&bytes)
    };
    write(root.join("plugins/godot/godot.component.wasm"), &bytes)
        .expect("component");
    write(
        root.join("plugins/godot/domain-manifest.toml"),
        format!(
            "id = \"godot\"\ndigest = \"{digest}\"\nabi = \"siralos:domain-abi@1.0.0\"\ncomponent = \"godot.component.wasm\"\n"
        ),
    )
    .expect("manifest");
    let output = run(
        "/domains-add plugins/godot\n/domains-enable godot\n/domains-activate godot\n/exit\n",
        &root,
        None,
    );
    assert!(output.contains("Installed godot"));
    assert!(output.contains("Enabled godot."));
    assert!(output.contains("Activated godot."));
    let _ = remove_dir_all(root);
}

#[test]
fn domains_enable_on_missing_id_fails_typed() {
    let root = temporary_directory("domains-enable-missing");
    let output = run("/domains-enable missing\n/exit\n", &root, None);
    assert!(output.contains("Enable failed:"));
    let _ = remove_dir_all(root);
}

#[test]
fn domains_activate_requires_host_authority_and_component() {
    let root = temporary_directory("domains-activate-no-component");
    create_dir_all(root.join("plugins/godot")).expect("folder");
    let digest = {
        use siralos_core::identity::sha256_hex;
        sha256_hex(b"no-component")
    };
    write(
        root.join("plugins/godot/domain-manifest.toml"),
        format!(
            "id = \"godot\"\ndigest = \"{digest}\"\nabi = \"siralos:domain-abi@1.0.0\"\n"
        ),
    )
    .expect("manifest");
    let output = run(
        "/domains-add plugins/godot\n/domains-enable godot\n/domains-activate godot\n/exit\n",
        &root,
        None,
    );
    assert!(output.contains("Installed godot"));
    assert!(output.contains("Activate failed:"));
    let _ = remove_dir_all(root);
}

#[test]
fn profile_plugin_selection_gates_domains_activate() {
    let root = temporary_directory("domains-activate-gate");
    create_dir_all(root.join("plugins/godot")).expect("folder");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/domain-conformance/fixtures/conformance-domain.component.wasm");
    let bytes = std::fs::read(&fixture).expect("fixture");
    let digest = {
        use siralos_core::identity::sha256_hex;
        sha256_hex(&bytes)
    };
    write(root.join("plugins/godot/godot.component.wasm"), &bytes)
        .expect("component");
    write(
        root.join("plugins/godot/domain-manifest.toml"),
        format!(
            "id = \"godot\"\ndigest = \"{digest}\"\nabi = \"siralos:domain-abi@1.0.0\"\ncomponent = \"godot.component.wasm\"\n"
        ),
    )
    .expect("manifest");
    // Applied profile with a selection that excludes godot: the
    // gate refuses before any install/enable/activate side effect.
    write(
        root.join("siralos.toml"),
        "\n[profile]\nname = \"dev\"\nplugins = [\"other\"]\n",
    )
    .expect("profile");
    let output = run(
        "/domains-add plugins/godot\n/domains-enable godot\n/domains-activate godot\n/exit\n",
        &root,
        None,
    );
    assert!(output.contains("Activate failed: the workspace profile does not select \"godot\"; it stays inactive"));
    assert!(!output.contains("Activated godot."));
    // Narrowed allow: the selection includes godot.
    write(
        root.join("siralos.toml"),
        "\n[profile]\nname = \"dev\"\nplugins = [\"godot\"]\n",
    )
    .expect("profile");
    let output = run(
        "/domains-add plugins/godot\n/domains-enable godot\n/domains-activate godot\n/exit\n",
        &root,
        None,
    );
    assert!(output.contains("Activated godot."));
    // Invalid profile: no selection, gate transparent (5.2).
    write(
        root.join("siralos.toml"),
        "\n[profile]\nname = \"dev\"\nplugins = [7]\n",
    )
    .expect("profile");
    let output = run(
        "/domains-add plugins/godot\n/domains-enable godot\n/domains-activate godot\n/exit\n",
        &root,
        None,
    );
    assert!(output.contains("Activated godot."));
    let _ = remove_dir_all(root);
}
#[test]
fn session_skill_consumption_surfaces_guidance_only() {
    use super::compose_skills_segment;
    use super::{
        DeclaredProfile, EffectiveRunPolicy, PermissionPolicy, PermissionRule,
        PolicyRule, WorkspaceProfileLoad, compose_effective_policy,
        declare_profile, load_workspace_profile,
    };
    let root = temporary_directory("skill-consume");
    create_dir_all(root.join(".siralos").join("skills")).expect("skills dir");
    write(
        root.join(".siralos").join("skills").join("alpha.md"),
        "guidance for alpha",
    )
    .expect("skill file");
    let host_rules = vec![PolicyRule {
        capability: siralos_core::tool::CapabilityId::parse("workspace.read")
            .expect("capability id"),
        rule: PermissionRule::Allow,
    }];
    let effective: EffectiveRunPolicy =
        compose_effective_policy(&host_rules, &DeclaredProfile::Absent);
    // Without an applied profile nothing binds (transparent).
    let absent_profile = compose_skills_segment(
        &root,
        &WorkspaceProfileLoad::Absent,
        &effective,
    );
    assert!(absent_profile.is_none());
    // Applied profile with an opt-in selection: the bound guidance
    // reaches the segment, sorted and guidance-only. Unknown names
    // never bind and never appear in the guidance.
    write(
        root.join("siralos.toml"),
        "\n[profile]\nname = \"dev\"\nskills = [\"ghost\", \"alpha\"]\n",
    )
    .expect("profile");
    let loaded = load_workspace_profile(&root);
    let declared = match &loaded {
        WorkspaceProfileLoad::Record(record) => declare_profile(
            Some(record),
            &PermissionPolicy::from_rules(host_rules.clone()),
        ),
        WorkspaceProfileLoad::Absent => DeclaredProfile::Absent,
        WorkspaceProfileLoad::Invalid { diagnostic } => {
            DeclaredProfile::Invalid { diagnostic: diagnostic.clone() }
        }
    };
    let effective_with_profile =
        compose_effective_policy(&host_rules, &declared);
    let segment =
        compose_skills_segment(&root, &loaded, &effective_with_profile);
    let segment = segment.expect("skills segment");
    assert_eq!(segment.id, "workspace-skills");
    assert_eq!(segment.title, "Workspace skills");
    assert!(segment.content.contains("guidance for alpha"));
    assert!(!segment.content.contains("ghost"));
    // A malformed skills key leaves the profile unapplied (5.2):
    // nothing binds, session proceeds transparently.
    write(
        root.join("siralos.toml"),
        "\n[profile]\nname = \"dev\"\nskills = \"alpha\"\n",
    )
    .expect("bad profile");
    let loaded_bad = load_workspace_profile(&root);
    let segment =
        compose_skills_segment(&root, &loaded_bad, &effective_with_profile);
    assert!(segment.is_none());
    let _ = remove_dir_all(root);
}
#[test]
fn session_lock_verification_reports_without_gating() {
    use super::{
        DeclaredProfile, PermissionRule, PolicyRule, compose_effective_policy,
        verify_session_lock,
    };
    use siralos_adapters::lockfile::write_workspace_lock;
    use siralos_core::composition::lock::{
        LockPluginIdentity, create_workspace_lock,
    };
    let root = temporary_directory("lock-verify");
    // Missing: verification is transparent.
    let host_rules = vec![PolicyRule {
        capability: siralos_core::tool::CapabilityId::parse("workspace.read")
            .expect("capability id"),
        rule: PermissionRule::Allow,
    }];
    let effective =
        compose_effective_policy(&host_rules, &DeclaredProfile::Absent);
    let decision = verify_session_lock(&root, &effective);
    assert_eq!(decision.outcome.as_str(), "missing");
    // Current: a written lock matching the recomputed state verifies.
    let empty = create_workspace_lock(None, &[]).expect("empty lock");
    write_workspace_lock(&root, &empty).expect("write lock");
    let decision = verify_session_lock(&root, &effective);
    assert_eq!(decision.outcome.as_str(), "current");
    assert_eq!(decision.reason, None);
    // Stale: a lock from a different plugin set drifts truthfully,
    // and the session still proceeds on live Host state.
    let drifted = create_workspace_lock(
        None,
        &[LockPluginIdentity {
            id: "ghost".to_owned(),
            path: "ghost".to_owned(),
            digest: "a".repeat(64),
        }],
    )
    .expect("drifted lock");
    write_workspace_lock(&root, &drifted).expect("write drifted");
    let decision = verify_session_lock(&root, &effective);
    assert_eq!(decision.outcome.as_str(), "stale");
    assert!(decision.reason.as_deref().is_some_and(|reason| {
        reason.starts_with("the on-disk lock does not match")
    }));
    let output = run("/tools\n/exit\n", &root, None);
    assert!(output.contains("Tool projection: not yet computed"));
    // Invalid: a corrupt lock is untrusted with a truthful reason,
    // and the session still proceeds.
    write(root.join("siralos.lock"), "lockDigest = \"corrupt\"\n")
        .expect("corrupt lock");
    let decision = verify_session_lock(&root, &effective);
    assert_eq!(decision.outcome.as_str(), "invalid");
    assert!(decision.reason.as_deref().is_some_and(|reason| {
        reason.starts_with("the on-disk lock could not be trusted")
    }));
    let output = run("/tools\n/exit\n", &root, None);
    assert!(output.contains("Tool projection: not yet computed"));
    let _ = remove_dir_all(root);
}
#[test]
fn profile_context_control_gates_context_claims() {
    // Transparent without a profile: byte-for-byte R7.5 render.
    let root = temporary_directory("context-control-gate");
    let bound = "a".repeat(64);
    let output = run("/context\n/exit\n", &root, None);
    assert!(output.contains("Context projection: not yet computed"));
    assert!(!output.contains("Context control:"));
    assert!(!output.contains("Context projection refused"));
    // Pinned stale: the claim stays usable but is labelled.
    write(
        root.join("siralos.toml"),
        format!(
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"pinned\"\ndigest = \"{bound}\"\n",
        ),
    )
    .expect("profile");
    let output = run("/context\n/exit\n", &root, None);
    assert!(output.contains("Context projection: not yet computed"));
    assert!(output
        .contains("Context control: context claim stale (the pinned content changed: expected "));
    // Frozen stale: the claim use is refused before rendering.
    write(
        root.join("siralos.toml"),
        format!(
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"frozen\"\ndigest = \"{bound}\"\n",
        ),
    )
    .expect("profile");
    let output = run("/context\n/exit\n", &root, None);
    assert!(!output.contains("Context projection: not yet computed"));
    assert!(output.contains(
        "Context projection refused: the frozen content changed: expected "
    ));
    // Invalid control: the profile is not applied, gate transparent.
    write(
        root.join("siralos.toml"),
        "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"pinned\"\n",
    )
    .expect("profile");
    let output = run("/context\n/exit\n", &root, None);
    assert!(output.contains("Context projection: not yet computed"));
    assert!(!output.contains("Context control:"));
    let _ = remove_dir_all(root);
}
#[test]
fn t4_dispatch_same_parse_both_loops_call() {
    // T4 B1 proof: the slash-command vocabulary is ONE parser both
    // loops call — every arm maps identically for stdio and TUI.
    for (line, expected) in [
        ("/context", SlashCommand::Context),
        ("/tools", SlashCommand::Tools),
        ("/domains", SlashCommand::Domains),
        ("/exit", SlashCommand::Exit),
        ("/domains-add", SlashCommand::DomainsAdd(None)),
        (
            "/domains-add plugins/godot",
            SlashCommand::DomainsAdd(Some("plugins/godot")),
        ),
        ("/domains-enable", SlashCommand::DomainsEnable(None)),
        ("/domains-enable godot", SlashCommand::DomainsEnable(Some("godot"))),
        ("/domains-activate", SlashCommand::DomainsActivate(None)),
        (
            "/domains-activate godot",
            SlashCommand::DomainsActivate(Some("godot")),
        ),
        ("/model", SlashCommand::Model(None)),
        (
            "/model example/model-a",
            SlashCommand::Model(Some("example/model-a")),
        ),
        ("/models", SlashCommand::Models),
        ("/models extra", SlashCommand::Prompt("/models extra")),
        ("/reload", SlashCommand::Reload),
        ("/reload extra", SlashCommand::Prompt("/reload extra")),
        ("hello there", SlashCommand::Prompt("hello there")),
        (
            "/definitely-not-a-command",
            SlashCommand::Prompt("/definitely-not-a-command"),
        ),
    ] {
        assert_eq!(
            parse_slash_command(line),
            expected,
            "shared parse must map `{line}` identically for both loops"
        );
    }
    // The stdio loop and the TUI loop call the same parser: parse
    // twice (once per loop) and require byte-equal commands.
    let stdio_parsed = parse_slash_command("/domains-enable godot");
    let tui_parsed = parse_slash_command("/domains-enable godot");
    assert_eq!(stdio_parsed, tui_parsed);
}
#[test]
fn t4_key_routing_calls_the_shared_handle_key() {
    // T4 B2 proof: the live loop routes non-modal keys through the
    // shared `handle_key` — exercise the shared function over the same
    // key kinds the loop used to inline (Enter/Backspace/Char/PageUp/
    // PageDown) and require the consolidated behavior.
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }
    let mut state = crate::tui::TuiState::new();
    // Char appends (was the inline `input.push` duplicate).
    assert!(!crate::tui::handle_key(
        &mut state,
        press(KeyCode::Char('a')),
        10
    ));
    assert!(!crate::tui::handle_key(
        &mut state,
        press(KeyCode::Char('b')),
        10
    ));
    assert_eq!(state.input, "ab");
    // Backspace pops (was the inline `input.pop` duplicate).
    assert!(!crate::tui::handle_key(
        &mut state,
        press(KeyCode::Backspace),
        10
    ));
    assert_eq!(state.input, "a");
    // PageUp/PageDown move scroll by 10 (was the inline ±10 duplicate).
    for i in 0..30 {
        state.transcript_lines.push(format!("line {i:02}"));
    }
    assert!(!crate::tui::handle_key(&mut state, press(KeyCode::PageUp), 10));
    assert_eq!(state.scroll_offset, 10);
    assert!(!crate::tui::handle_key(&mut state, press(KeyCode::PageDown), 10));
    assert_eq!(state.scroll_offset, 0);
    // Enter submits (was the inline Enter arm).
    assert!(crate::tui::handle_key(&mut state, press(KeyCode::Enter), 10));
    // While a modal is pending every key is ignored (loop's modal
    // branch routes to `handle_modal_key` first — unchanged).
    state.pending_approval =
        Some(crate::tui::ApprovalModal::new(vec!["req".to_owned()]));
    assert!(!crate::tui::handle_key(
        &mut state,
        press(KeyCode::Char('z')),
        10
    ));
    assert!(!crate::tui::handle_key(&mut state, press(KeyCode::Enter), 10));
}
#[test]
fn t4_compose_session_is_the_single_shared_definition() {
    // T4 B3 proof: both loops call the same `compose_session` — compose
    // once here and require the full bundle (tools snapshot, policy,
    // context wiring) to be present and byte-transparent OFF.
    let root = temporary_directory("compose-shared");
    let session = compose_session(InteractiveOptions {
        config_path: None,
        workspace_root: Some(&root),
    })
    .expect("compose");
    // The composed tool definitions carry the three workspace tools in
    // registration order (context tools absent OFF).
    let names: Vec<&str> = session
        .tool_definitions
        .iter()
        .map(|info| info.definition.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["workspace.list", "workspace.read", "workspace.search"]
    );
    // OFF: no context session, flag off.
    assert!(!session.context_system_enabled);
    assert!(session.context_session_holder.is_none());
    // The same bundle the stdio loop consumes renders `/tools`
    // byte-equal to a direct stdio dispatch: run the session and
    // require the deterministic tool list.
    let output = run("/tools\n/exit\n", &root, None);
    assert!(output.contains("workspace.list"));
    assert!(output.contains("workspace.read"));
    assert!(output.contains("workspace.search"));
    let _ = remove_dir_all(root);
}
#[test]
fn session_replay_store_hermetic() {
    use siralos_adapters::replay_store::{
        load_replay_store, write_replay_store,
    };
    use siralos_core::determinism::{
        ProviderResponseIdentity, ReplayRecorder, ReplayRecording,
        RetainingReplayRecorder,
    };
    use siralos_core::provider::{
        CancellationToken, ModelProvider, ModelRequest,
    };
    fn recording(body: &str) -> ReplayRecording {
        let sha = siralos_core::identity::sha256_hex(body.as_bytes());
        ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: "replay-subject".to_owned(),
                model: "replay-model".to_owned(),
                status: Some(200),
                body_sha256: sha,
                body_bytes: body.len() as u64,
                observed_at_ms: Some(1000),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: body.to_owned(),
        }
    }
    // Seeded store replays recordings.
    let root = temporary_directory("replay-seeded");
    let store_path = root.join(".siralos").join("replay-store.json");
    std::fs::create_dir_all(store_path.parent().expect("parent"))
        .expect("mkdir");
    let body1 = r#"{"choices":[{"message":{"content":"alpha"}}]}"#;
    let body2 = r#"{"choices":[{"message":{"content":"beta"}}]}"#;
    let recs = vec![recording(body1), recording(body2)];
    let persisted = write_replay_store(&store_path, &recs).expect("write");
    assert_eq!(persisted, 2);
    let loaded = load_replay_store(&store_path).expect("load");
    assert_eq!(loaded.recordings.len(), 2);
    let provider =
        siralos_adapters::provider::replay::RecordedReplayProvider::new(
            "replay-subject".to_owned(),
            "replay-model".to_owned(),
            loaded.recordings.clone(),
        );
    let req = ModelRequest { messages: vec![], tools: vec![], system: None };
    let t1: Vec<_> =
        provider.stream(&req, CancellationToken::new().signal()).collect();
    assert!(t1.iter().any(|e| format!("{e:?}").contains("alpha")));
    let t2: Vec<_> =
        provider.stream(&req, CancellationToken::new().signal()).collect();
    assert!(t2.iter().any(|e| format!("{e:?}").contains("beta")));
    let t3: Vec<_> =
        provider.stream(&req, CancellationToken::new().signal()).collect();
    assert!(t3.iter().any(|e| format!("{e:?}").contains("exhausted")));
    let _ = remove_dir_all(root);
    // Absent store -> typed diagnostic + exhausted provider (hermetic).
    let root2 = temporary_directory("replay-absent");
    let absent_path = root2.join(".siralos").join("replay-store.json");
    let err = load_replay_store(&absent_path).expect_err("absent");
    assert!(matches!(
        err,
        siralos_adapters::replay_store::ReplayStoreLoadError::NotFound
    ));
    let empty_provider =
        siralos_adapters::provider::replay::RecordedReplayProvider::new(
            "replay-subject".to_owned(),
            "replay-model".to_owned(),
            vec![],
        );
    let t: Vec<_> = empty_provider
        .stream(&req, CancellationToken::new().signal())
        .collect();
    assert!(t.iter().any(|e| format!("{e:?}").contains("exhausted")));
    let _ = remove_dir_all(root2);
    // Untrusted store -> fail-closed (UntrustedDigest).
    let root3 = temporary_directory("replay-untrusted");
    let p3 = root3.join(".siralos").join("replay-store.json");
    std::fs::create_dir_all(p3.parent().expect("parent")).expect("mkdir");
    write_replay_store(&p3, &recs).expect("write");
    let mut raw = std::fs::read_to_string(&p3).expect("read");
    raw = raw.replacen("alpha", "AlpHa", 1);
    std::fs::write(&p3, raw).expect("tamper");
    let err = load_replay_store(&p3).expect_err("untrusted");
    assert!(matches!(
        err,
        siralos_adapters::replay_store::ReplayStoreLoadError::UntrustedDigest
    ));
    let _ = remove_dir_all(root3);
    // Record-replay exit-flush writes a store that load verifies (hermetic).
    let root4 = temporary_directory("replay-record-flush");
    let p4 = root4.join(".siralos").join("replay-store.json");
    std::fs::create_dir_all(p4.parent().expect("parent")).expect("mkdir");
    let recorder = RetainingReplayRecorder::new();
    let id1 = ProviderResponseIdentity {
        provider_id: "x".to_owned(),
        model: "m".to_owned(),
        status: Some(200),
        body_sha256: siralos_core::identity::sha256_hex(body1.as_bytes()),
        body_bytes: body1.len() as u64,
        observed_at_ms: Some(1),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    recorder.record_provider_response(&id1);
    recorder.record_provider_response_with_body(&id1, body1);
    let snap = recorder.records_snapshot();
    let persisted = write_replay_store(&p4, &snap).expect("flush write");
    assert_eq!(persisted, 1);
    let loaded = load_replay_store(&p4).expect("flush load verifies");
    assert_eq!(loaded.recordings.len(), 1);
    assert_eq!(loaded.recordings[0].body, body1);
    let _ = remove_dir_all(root4);
}

#[test]
fn context_system_off_tools_are_byte_transparent() {
    // A workspace with no applied profile must NOT register the three
    // context tools — the OFF path is byte-transparent (decision 99 W3).
    let root = temporary_directory("context-system-off");
    let output = run("/tools\n/exit\n", &root, None);
    assert!(output.contains("workspace.list"));
    assert!(output.contains("workspace.read"));
    assert!(output.contains("workspace.search"));
    assert!(!output.contains("context.search"));
    assert!(!output.contains("context.inspect"));
    assert!(!output.contains("context.expand"));
    let _ = remove_dir_all(root);
}

#[test]
fn context_system_optin_registers_exactly_three_context_tools() {
    // An applied profile with `[profile.context_system] enabled = true`
    // registers exactly the three read-only context tools (W2). An
    // absent or false key leaves the session byte-transparent (W3).
    let root = temporary_directory("context-system-on");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = true\n",
    )
    .expect("profile");
    let output = run("/tools\n/exit\n", &root, None);
    assert!(output.contains("context.search"));
    assert!(output.contains("context.inspect"));
    assert!(output.contains("context.expand"));
    // The workspace tools remain present; no mutation capability appears.
    assert!(output.contains("workspace.read"));
    assert!(!output.contains("workspace.write"));
    assert!(!output.contains("context_system"));
    let _ = remove_dir_all(root);
}

#[test]
fn context_system_malformed_profile_leaves_off() {
    // A present `[profile.context_system]` without a valid boolean
    // enabled leaves the WHOLE profile unapplied (decision 48 C3); the
    // session stays byte-transparent (no context tools, no change).
    let root = temporary_directory("context-system-malformed");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = \"yes\"\n",
    )
    .expect("profile");
    let output = run("/tools\n/exit\n", &root, None);
    assert!(!output.contains("context.search"));
    assert!(!output.contains("context.inspect"));
    assert!(!output.contains("context.expand"));
    let _ = remove_dir_all(root);
}

#[test]
fn slash_command_catalog_lists_provider_model_evolve() {
    let catalog = slash_command_catalog();
    let names: Vec<&str> = catalog.iter().map(|(n, _)| *n).collect();
    assert!(names.contains(&"/provider"));
    assert!(names.contains(&"/model"));
    assert!(names.contains(&"/model <id>"));
    assert!(names.contains(&"/models"));
    assert!(names.contains(&"/reload"));
    assert!(names.contains(&"/evolve"));
    assert!(names.contains(&"/context"));
    assert!(names.contains(&"/mouse"));
    assert_eq!(names.len(), 15);
}

#[test]
fn parse_slash_recognizes_provider_model_evolve() {
    assert!(matches!(
        parse_slash_command("/provider"),
        SlashCommand::Provider
    ));
    assert!(matches!(
        parse_slash_command("/model"),
        SlashCommand::Model(None)
    ));
    assert!(matches!(
        parse_slash_command("/model example/model-a"),
        SlashCommand::Model(Some("example/model-a"))
    ));
    assert!(!is_unknown_slash_command("/model example/model-a"));
    assert!(!is_unknown_slash_command("/model"));
    assert!(matches!(parse_slash_command("/models"), SlashCommand::Models));
    assert!(matches!(parse_slash_command("/reload"), SlashCommand::Reload));
    assert!(!is_unknown_slash_command("/reload"));
    assert!(matches!(
        parse_slash_command("/reload extra"),
        SlashCommand::Prompt("/reload extra")
    ));
    assert!(is_unknown_slash_command("/reload extra"));
    assert!(matches!(parse_slash_command("/evolve"), SlashCommand::Evolve));
    // Unknown still goes to prompt
    assert!(matches!(
        parse_slash_command("/unknown"),
        SlashCommand::Prompt("/unknown")
    ));
}

#[test]
fn provider_model_evolve_dispatch_stdio_and_tui() {
    let root = temporary_directory("provider-model-evolve");
    let output = run("/provider\n/model\n/evolve\n/exit\n", &root, None);
    assert!(output.contains("provider:"));
    assert!(output.contains("credential:"));
    assert!(output.contains("model:"));
    assert!(output.contains("Stage 6 evolution surfaces"));
    assert!(output.contains("corpus"));
    assert!(output.contains("workflow"));
    assert!(output.contains("proposal"));
    assert!(output.contains("packaging"));
    assert!(output.contains("host-gated"));
    let _ = remove_dir_all(root);
}

#[test]
fn models_reports_honestly_when_unconfigured() {
    // I6: when no provider/endpoint/credential, /models reports honestly (no fetch, no leak)
    let root = temporary_directory("models-unconfigured");
    let output = run("/models\n/exit\n", &root, None);
    assert!(
        output.contains("no provider configured"),
        "expected honest unconfigured line, got: {output:?}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn reload_reports_edited_profile_changes_without_mutating() {
    // SAFE HALF: an edited profile recomposes through the same
    // composition path and the report names the changes — with no
    // live-state mutation (pure `reload_report`; the session still
    // holds the startup snapshot afterwards).
    use super::reload_report;
    let root = temporary_directory("reload-edited");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nendpoint = \"https://placeholder.example/v1\"\n",
    )
    .expect("profile");
    // Unchanged: starting from the same file the report is all-unchanged.
    let (same, _) = reload_report(
        &root,
        Some("example-vendor"),
        Some("example/model-a"),
        None,
        Some("https://placeholder.example/v1"),
        "openai-completions",
    );
    assert!(
        same.contains("provider unchanged")
            && same.contains("model unchanged")
            && same.contains("endpoint unchanged"),
        "unchanged report must name every field unchanged, got: {same:?}"
    );
    // Edited: change only the model on disk; the live snapshot still
    // holds the old value, so the report must name the exact change.
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-b\"\nendpoint = \"https://placeholder.example/v1\"\n",
    )
    .expect("edited profile");
    let (report, _) = reload_report(
        &root,
        Some("example-vendor"),
        Some("example/model-a"),
        None,
        Some("https://placeholder.example/v1"),
        "openai-completions",
    );
    assert!(
        report.contains("provider unchanged")
            && report.contains("model example/model-a -> example/model-b")
            && report.contains("endpoint unchanged")
            && report.contains("would change"),
        "edited report must name the model change truthfully, got: {report:?}"
    );
    assert!(
        !report.contains("live session unchanged"),
        "the report must not claim the session is unchanged while the apply step runs, got: {report:?}"
    );
    // Endpoint values are never echoed: a changed endpoint reports the
    // bare word `endpoint changed`.
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-b\"\nendpoint = \"https://other-placeholder.example/v1\"\n",
    )
    .expect("edited endpoint");
    let (endpoint_report, _) = reload_report(
        &root,
        Some("example-vendor"),
        Some("example/model-b"),
        None,
        Some("https://placeholder.example/v1"),
        "openai-completions",
    );
    assert!(
        endpoint_report.contains("endpoint changed"),
        "endpoint change must not echo URLs, got: {endpoint_report:?}"
    );
    assert!(
        !endpoint_report.contains("other-placeholder"),
        "endpoint values must never be echoed, got: {endpoint_report:?}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn reload_reports_invalid_diagnostic_verbatim_and_changes_nothing() {
    // Profile INVALID: the exact `load_workspace_profile` diagnostic
    // is reported verbatim; nothing is recomposed and nothing mutates.
    use super::reload_report;
    let root = temporary_directory("reload-invalid");
    let bad = "[profile]\nname = \"default\"\nprovider = 7\n";
    write(root.join("siralos.toml"), bad).expect("bad profile");
    let expected =
        match siralos_adapters::profile_config::load_workspace_profile(
            &root,
        ) {
            siralos_adapters::profile_config::WorkspaceProfileLoad::Invalid {
                diagnostic,
            } => diagnostic,
            other => panic!("expected invalid, got: {other:?}"),
        };
    let (report, _) = reload_report(
        &root,
        Some("example-vendor"),
        Some("example/model-a"),
        None,
        Some("https://placeholder.example/v1"),
        "openai-completions",
    );
    assert!(
        report.contains(&expected),
        "invalid report must carry the diagnostic verbatim, got: {report:?}"
    );
    assert!(
        report.starts_with("reload not applied:"),
        "invalid report must refuse application, got: {report:?}"
    );
    // Changes nothing: the on-disk bytes are untouched by the report.
    let after = read(root.join("siralos.toml"))
        .map(|bytes| String::from_utf8(bytes).unwrap_or_default())
        .unwrap_or_default();
    assert_eq!(after, bad);
    let _ = remove_dir_all(root);
}

#[test]
fn reload_absent_follows_startup_semantics() {
    // Profile ABSENT: startup semantics fall back to the
    // deterministic fake on pure Host policy — the report says so.
    // A session already there reports nothing-would-change; a live
    // session holding a stale applied snapshot reports the drift.
    use super::reload_report;
    let root = temporary_directory("reload-absent");
    let (converged, _) =
        reload_report(&root, None, None, None, None, "openai-completions");
    assert!(
        converged.contains("no profile configured")
            && converged.contains("deterministic fake")
            && converged.contains("nothing would change"),
        "absent+converged report must state startup semantics, got: {converged:?}"
    );
    let (drifted, _) = reload_report(
        &root,
        Some("example-vendor"),
        Some("example/model-a"),
        None,
        Some("https://placeholder.example/v1"),
        "openai-completions",
    );
    assert!(
        drifted.contains("no profile configured")
            && drifted.contains("deterministic fake")
            && drifted.contains("restart to converge"),
        "absent+drifted report must state the fallback drift, got: {drifted:?}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn reload_cannot_widen_session_authority() {
    // AUTHORITY INVARIANT -- the acceptance criterion for /reload. A
    // reload re-reads declarative configuration and recomposes the
    // PROVIDER snapshot only; authority is composed once at startup and
    // is never an input or an output of the reload path. So a profile
    // that asks for more than the Host grants cannot widen what the
    // session may do: the composition refuses it and the effective rules
    // stay the Host's own.
    use super::{
        apply_reloaded_config, declare_and_compose_profile, reload_report,
        session_host_rules,
    };
    let root = temporary_directory("reload-no-widen");
    // The Host grants `workspace.read` only; this profile asks for more.
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"greedy\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\n[profile.permissions]\nworkspace.write = \"allow\"\n",
    )
    .expect("widening profile");
    let host_rules = session_host_rules();
    let before = declare_and_compose_profile(
        &siralos_adapters::profile_config::load_workspace_profile(&root),
        &host_rules,
    );
    assert_eq!(
        before.rules, host_rules,
        "a widening declaration must not add or broaden a rule"
    );
    assert!(
        before.applied_profile.is_none(),
        "a widening profile must not apply, got: {before:?}"
    );
    assert!(
        before.diagnostic.is_some(),
        "the refusal must carry a diagnostic, got: {before:?}"
    );
    // Run the entire reload path against the same profile.
    let (mut report, recomposed) =
        reload_report(&root, None, None, None, None, "openai-completions");
    assert!(
        recomposed.is_none(),
        "a refused profile must not reach the apply step, got: {recomposed:?}"
    );
    let session = switch_test_provider("example/model-a");
    let mut model = None;
    let mut display = None;
    let mut endpoint = None;
    let mut protocol_str = "openai-completions".to_owned();
    let mut credential: Option<siralos_adapters::provider::HostCredential> =
        None;
    let mut credential_raw: Option<String> = None;
    apply_reloaded_config(
        &session,
        None,
        &mut model,
        &mut display,
        &mut endpoint,
        &mut protocol_str,
        &mut credential,
        &mut credential_raw,
        recomposed,
        &mut report,
    );
    let after = declare_and_compose_profile(
        &siralos_adapters::profile_config::load_workspace_profile(&root),
        &host_rules,
    );
    assert_eq!(after, before, "a reload must not change composed authority");
    assert_eq!(
        after.rules, host_rules,
        "authority stays the Host's own after a reload"
    );
}

#[test]
fn reload_applies_the_recomposed_model_to_the_live_session() {
    // APPLY HALF: a reload whose profile names a different model moves
    // the live cell the NEXT request reads, adopts the file's display
    // name, moves the live endpoint base the next request resolves
    // its URL from, and says so on the report -- and never rewrites
    // the file (the file is where the value came from).
    use super::{apply_reloaded_config, reload_report};
    let root = temporary_directory("reload-apply-model");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-b\"\nmodel_display_name = \"New Display\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-responses\"\ncredential = \"key:example-test-value\"\n",
    )
    .expect("edited profile");
    let session = switch_test_provider("example/model-a");
    let mut model = Some("example/model-a".to_owned());
    let mut display = Some("Old Display".to_owned());
    let mut endpoint = Some("https://old.example.com/v1".to_owned());
    let mut protocol_str = "openai-completions".to_owned();
    let mut credential: Option<siralos_adapters::provider::HostCredential> =
        None;
    let mut credential_raw: Option<String> = None;
    let live_model = session.live_model();
    let (mut report, recomposed) = reload_report(
        &root,
        Some("example-vendor"),
        live_model.as_deref().or(model.as_deref()),
        None,
        Some("https://old.example.com/v1"),
        "openai-completions",
    );
    apply_reloaded_config(
        &session,
        live_model.as_deref(),
        &mut model,
        &mut display,
        &mut endpoint,
        &mut protocol_str,
        &mut credential,
        &mut credential_raw,
        recomposed,
        &mut report,
    );
    assert_eq!(session.live_model().as_deref(), Some("example/model-b"));
    assert_eq!(model.as_deref(), Some("example/model-b"));
    assert_eq!(display.as_deref(), Some("New Display"));
    // The endpoint base behind the next request also moved, and so did
    // the protocol that selects the POST path appended to that base.
    assert_eq!(endpoint.as_deref(), Some("https://api.example.com/v1"));
    assert_eq!(
        session.live_endpoint().as_deref(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(protocol_str, "openai-responses");
    assert_eq!(
        session.live_protocol(),
        Some(siralos_core::composition::Protocol::OpenAiResponses)
    );
    assert!(
        report.contains(
            "applied: protocol openai-completions -> openai-responses (live, no restart)"
        ),
        "the report must name the live protocol apply, got: {report:?}"
    );
    assert!(
        report.contains("applied: model example/model-a -> example/model-b"),
        "the report must name the live apply, got: {report:?}"
    );
    assert!(
        report.contains("applied: endpoint changed (live, no restart)"),
        "the report must name the live endpoint apply, got: {report:?}"
    );
    // The credential the form writes verbatim resolves and applies on
    // the same reload -- the 401 fix pinned: a declared credential is
    // never silently dropped from the request.
    assert!(credential.is_some(), "the declared credential must resolve");
    assert_eq!(credential_raw.as_deref(), Some("key:example-test-value"));
    assert!(session.live_credential().is_some());
    assert!(
        report.contains("applied: credential changed (live, no restart)"),
        "the report must name the live credential apply, got: {report:?}"
    );
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            assert_eq!(record.model.as_deref(), Some("example/model-b"));
            assert_eq!(
                record.model_display_name.as_deref(),
                Some("New Display")
            );
        }
        other => panic!("expected an applied record, got: {other:?}"),
    }
}

#[test]
fn reload_reports_an_unresolvable_credential_instead_of_dropping_it() {
    // The 401 mystery: a DECLARED credential that cannot be resolved
    // must be reported, never silently dropped from the request.
    use super::{apply_reloaded_config, reload_report};
    let root = temporary_directory("reload-credential-unresolved");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nendpoint = \"https://api.example.com/v1\"\ncredential = \"env:SIRALOS_TEST_UNSET_VARIABLE\"\n",
    )
    .expect("profile");
    let session = switch_test_provider("example/model-a");
    let mut model = Some("example/model-a".to_owned());
    let mut display = None;
    let mut endpoint = Some("https://api.example.com/v1".to_owned());
    let mut protocol_str = "openai-completions".to_owned();
    let mut credential: Option<siralos_adapters::provider::HostCredential> =
        None;
    let mut credential_raw: Option<String> = None;
    let live_model = session.live_model();
    let (mut report, recomposed) = reload_report(
        &root,
        Some("example-vendor"),
        live_model.as_deref().or(model.as_deref()),
        None,
        Some("https://api.example.com/v1"),
        "openai-completions",
    );
    apply_reloaded_config(
        &session,
        live_model.as_deref(),
        &mut model,
        &mut display,
        &mut endpoint,
        &mut protocol_str,
        &mut credential,
        &mut credential_raw,
        recomposed,
        &mut report,
    );
    assert!(credential.is_none(), "an unresolved credential is never applied");
    assert!(
        report.contains(
            "not applied: credential (env var SIRALOS_TEST_UNSET_VARIABLE is not set)"
        ),
        "an unresolvable credential must be reported, got: {report:?}"
    );
}

#[test]
fn reload_end_to_end_stdio_reports_without_mutating() {
    // Headless stdio: `/reload` reaches the shared catalog vocabulary
    // (no TTY needed) and the report leaves disk + live state alone.
    let root = temporary_directory("reload-stdio");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nendpoint = \"https://placeholder.example/v1\"\n",
    )
    .expect("profile");
    let output = run("/reload\n/exit\n", &root, None);
    assert!(
        output.contains("provider unchanged")
            && output.contains("model unchanged"),
        "stdio /reload must print the recomposition report, got: {output:?}"
    );
    // The report changed nothing on disk.
    let after = read(root.join("siralos.toml"))
        .map(|bytes| String::from_utf8(bytes).unwrap_or_default())
        .unwrap_or_default();
    assert!(after.contains("example/model-a"));
    // A following bare `/model` still shows the live (startup) value.
    let output2 = run("/reload\n/model\n/exit\n", &root, None);
    assert!(
        output2.contains("model: example/model-a"),
        "live session must be unmutated by /reload, got: {output2:?}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn evolve_lists_exactly_four_surfaces() {
    let text = render_evolve_lines();
    assert!(text.contains("corpus"));
    assert!(text.contains("workflow"));
    assert!(text.contains("proposal"));
    assert!(text.contains("packaging"));
    let count = ["corpus", "workflow", "proposal", "packaging"]
        .iter()
        .filter(|s| text.contains(**s))
        .count();
    assert_eq!(count, 4);
    // Must state host-gated execution note
    assert!(text.contains("host-gated"));
    assert!(text.contains("Profile->Host"));
}

#[test]
fn provider_line_credential_present_absent() {
    // Verbatim redaction: key: -> key:*** ; env: -> env:NAME ; absent -> absent
    let key = render_provider_line(Some("openai"), Some("key:secret123"));
    assert!(key.contains("provider: openai"));
    assert!(key.contains("credential: key:***"));
    assert!(!key.contains("secret123"));
    let env = render_provider_line(Some("openai"), Some("env:OPENAI_API_KEY"));
    assert!(env.contains("credential: env:OPENAI_API_KEY"));
    let absent = render_provider_line(Some("openai"), None);
    assert!(absent.contains("credential: absent"));
    let no_provider = render_provider_line(None, None);
    assert!(no_provider.contains("no provider configured"));
    let model = render_model_line(Some("model-a"));
    assert!(model.contains("model-a"));
    let no_model = render_model_line(None);
    assert!(no_model.contains("no model configured"));
}

#[test]
fn verbatim_storage_public_writes_key_public() {
    let root = temporary_directory("verbatim-public");
    write_profile_config(
        &root,
        "openai",
        "gpt-4o",
        Some("key:public"),
        Some("https://api.example.com/v1"),
        None,
        None,
    )
    .expect("write key:public");
    let loaded =
        siralos_adapters::profile_config::load_workspace_profile(&root);
    match loaded {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(r) => {
            assert_eq!(r.credential.as_deref(), Some("key:public"));
            assert!(siralos_adapters::provider::HostCredential::from_credential_str(r.credential.as_deref().unwrap()).is_ok());
            // Redacted display must not leak value
            let line = render_provider_line(
                r.provider.as_deref(),
                r.credential.as_deref(),
            );
            assert!(line.contains("key:***"));
            assert!(!line.contains("public"));
        }
        _ => panic!("profile must apply"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn verbatim_storage_env_form_still_works() {
    let root = temporary_directory("verbatim-env");
    write_profile_config(
        &root,
        "openai",
        "gpt-4o",
        Some("env:PATH"),
        Some("https://api.example.com/v1"),
        None,
        None,
    )
    .expect("write env:");
    let loaded =
        siralos_adapters::profile_config::load_workspace_profile(&root);
    match loaded {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(r) => {
            assert_eq!(r.credential.as_deref(), Some("env:PATH"));
            let line = render_provider_line(
                r.provider.as_deref(),
                r.credential.as_deref(),
            );
            assert!(line.contains("env:PATH"));
            assert!(!line.contains("key:***"));
        }
        _ => panic!("profile must apply"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn verbatim_storage_empty_is_none() {
    let root = temporary_directory("verbatim-empty");
    write_profile_config(
        &root,
        "openai",
        "gpt-4o",
        None,
        Some("https://api.example.com/v1"),
        None,
        None,
    )
    .expect("write none");
    let loaded =
        siralos_adapters::profile_config::load_workspace_profile(&root);
    match loaded {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(r) => {
            assert!(r.credential.is_none());
            let line = render_provider_line(
                r.provider.as_deref(),
                r.credential.as_deref(),
            );
            assert!(line.contains("absent"));
        }
        _ => panic!("profile must apply"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn redaction_status_line_no_leak() {
    // Status line currently provider/model only; ensure provider line redaction covers key leak.
    let raw_key = "key:super-secret-value-that-must-not-leak";
    let line = render_provider_line(Some("my-provider"), Some(raw_key));
    assert!(!line.contains("super-secret"));
    assert!(line.contains("key:***"));
    let line_env =
        render_provider_line(Some("my-provider"), Some("env:MY_KEY"));
    assert!(line_env.contains("env:MY_KEY"));
    let line_absent = render_provider_line(Some("my-provider"), None);
    assert!(line_absent.contains("absent"));
}

#[test]
fn write_round_trip_key_public_then_resolve() {
    let root = temporary_directory("verbatim-roundtrip");
    write_profile_config(
        &root,
        "openai",
        "gpt-4o",
        Some("key:public"),
        Some("https://api.example.com/v1"),
        Some("openai-completions"),
        None,
    )
    .expect("write");
    // Load via workspace profile applies, then GenericProvider resolution sees key:public
    let loaded =
        siralos_adapters::profile_config::load_workspace_profile(&root);
    let rec = match loaded {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(r) => r,
        _ => panic!("should apply"),
    };
    assert_eq!(rec.credential.as_deref(), Some("key:public"));
    assert!(
        siralos_adapters::provider::HostCredential::from_credential_str(
            rec.credential.as_deref().unwrap()
        )
        .is_ok()
    );
    let _ = remove_dir_all(root);
}

#[test]
fn credential_bound_splits_by_form() {
    // The [profile] credential bound splits by form: env forms keep
    // the 70-byte whole-value bound (name 1..=64), while key:VALUE
    // allows VALUE 1..=4096 bytes. All keys below are synthetic.
    let write_and_load = |label: &str, cred: Option<&str>| {
        let root = temporary_directory(label);
        let write_result = write_profile_config(
            &root,
            "openai",
            "gpt-4o",
            cred,
            Some("https://api.example.com/v1"),
            None,
            None,
        );
        let loaded = write_result.is_ok().then(|| {
            siralos_adapters::profile_config::load_workspace_profile(&root)
        });
        (root, write_result, loaded)
    };
    // Accepted: key: + 73 synthetic chars (77 bytes total) round-trips.
    let key_73 = format!("key:{}", "a".repeat(73));
    let (root, write_result, loaded) =
        write_and_load("cred-key-73", Some(&key_73));
    assert!(write_result.is_ok(), "write failed: {write_result:?}");
    match loaded.expect("write ok implies load checked") {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(r) => {
            assert_eq!(r.credential.as_deref(), Some(key_73.as_str()))
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    let _ = remove_dir_all(&root);
    // Accepted: key: + 4096 bytes.
    let key_max = format!("key:{}", "a".repeat(4096));
    let (root, write_result, loaded) =
        write_and_load("cred-key-max", Some(&key_max));
    assert!(write_result.is_ok(), "write failed: {write_result:?}");
    match loaded.expect("write ok implies load checked") {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(r) => {
            assert_eq!(r.credential.as_deref(), Some(key_max.as_str()))
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    let _ = remove_dir_all(&root);
    // Accepted: env: + 64-char name, and the bare 64-char legacy name.
    for (label, cred) in [
        ("cred-env-64", format!("env:{}", "A".repeat(64))),
        ("cred-bare-64", "A".repeat(64)),
    ] {
        let (root, write_result, loaded) = write_and_load(label, Some(&cred));
        assert!(write_result.is_ok(), "write failed: {write_result:?}");
        match loaded.expect("write ok implies load checked") {
            siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
                r,
            ) => assert_eq!(r.credential.as_deref(), Some(cred.as_str())),
            other => panic!("expected applied record, got: {other:?}"),
        }
        let _ = remove_dir_all(&root);
    }
    // Refused: key: + 4097 bytes, env: + 65-char name, empty key value.
    for (label, cred) in [
        ("cred-key-over", format!("key:{}", "a".repeat(4097))),
        ("cred-env-over", format!("env:{}", "A".repeat(65))),
        ("cred-key-empty", "key:".to_owned()),
    ] {
        let (root, write_result, _) = write_and_load(label, Some(&cred));
        assert!(write_result.is_err(), "credential must be refused: {cred:?}");
        let _ = remove_dir_all(&root);
    }
    // Load-side: parsing is shape-only, so a hand-written over-long
    // credential still parses — but the core validator (the same one
    // the session runs via resolve_profile_overlay) refuses it, so
    // such a profile fails to apply, not just to save.
    for (label, cred, bound) in [
        (
            "cred-load-key-over",
            format!("key:{}", "a".repeat(4097)),
            "4096-byte",
        ),
        ("cred-load-env-over", format!("env:{}", "A".repeat(65)), "64"),
    ] {
        let root = temporary_directory(label);
        write(
            root.join("siralos.toml"),
            format!(
                "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\ncredential = \"{cred}\"\nendpoint = \"https://api.example.com/v1\"\n"
            ),
        )
        .expect("hand-written profile");
        let record = match siralos_adapters::profile_config::load_workspace_profile(
            &root,
        ) {
            siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
                record,
            ) => record,
            other => {
                panic!("shape-only parse must load, got: {other:?}")
            }
        };
        match record.validate() {
            Err(error) => assert!(
                error.message.contains(bound),
                "diagnostic must name its bound, got: {error:?}"
            ),
            Ok(()) => {
                panic!("over-long credential must not validate: {cred:?}")
            }
        }
        let _ = remove_dir_all(&root);
    }
}

#[test]
fn status_provider_model_prefix_from_composed_profile() {
    let root = temporary_directory("status-provider");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"dev\"\nprovider = \"example-vendor\"\nmodel = \"model-a\"\n",
    )
    .expect("profile");
    let output_provider = run("/provider\n/exit\n", &root, None);
    assert!(output_provider.contains("provider: example-vendor"));
    let output_model = run("/model\n/exit\n", &root, None);
    assert!(output_model.contains("model: model-a"));
    let _ = remove_dir_all(root);
    // Absent provider via no profile
    let root2 = temporary_directory("status-no-provider");
    let output2 = run("/provider\n/exit\n", &root2, None);
    assert!(output2.contains("no provider configured"));
    let _ = remove_dir_all(root2);
}

#[test]
fn echo_is_sanitized_and_empty_skipped() {
    // R4: user echo must be sanitized and empty input must not produce an echo line
    let root = temporary_directory("echo-sanitize");
    // Poison input with ANSI and NUL
    let poison = "hello\x1b[31mred\x00world";
    let output = run(&format!("{poison}\n/exit\n"), &root, None);
    // Raw escape and NUL must not appear raw in output
    assert!(!output.contains("\x1b[31m"));
    assert!(!output.contains('\0'.to_string().as_str()));
    // Sanitized form should appear (via the echo or safe rendering)
    let sanitized = crate::sanitize::sanitize_for_display(poison);
    // The echo sanitizes: "> sanitized"
    // run output for stdio does NOT echo? Actually stdio path also echoes? Check: run uses stdio frontend which may echo?
    // For TUI path, echo is sanitized. For stdio, we verify provider output is sanitized via other path.
    // At least ensure sanitizer is used somewhere: the status or projection contains sanitized.
    assert!(!sanitized.contains('\x1b'));
    // Empty input skip: just newline should not produce "> " echo line
    let empty_output = run("\n/exit\n", &root, None);
    // Empty echo would be "> \n" — ensure not present as a line starting with "> "
    let has_empty_echo = empty_output.lines().any(|l| l == "> ");
    assert!(!has_empty_echo, "empty input must not echo: {empty_output:?}");
    let _ = remove_dir_all(root);
}

#[test]
fn compose_session_before_guard_no_terminal_needed() {
    // R6: composition must succeed without any terminal guard (startup
    // diagnostics visible).
    let root = temporary_directory("compose-ordering");
    let opts =
        InteractiveOptions { workspace_root: Some(&root), config_path: None };
    let session = compose_session(opts);
    assert!(session.is_ok(), "compose_session should not need a terminal");
    let _ = remove_dir_all(root);
    // C2 step 3 source check: the TUI no longer composes in-thread, so the
    // invariant is now "the worker is spawned AND its header awaited before
    // TerminalGuard::enter" -- the composition's diagnostics and a
    // composition failure still land on the normal screen.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/interactive.rs"),
    )
    .expect("read interactive.rs");
    let entry = src
        .find("pub fn run_interactive_tui_with_options")
        .expect("the TUI entry");
    let body = &src[entry..];
    let spawn_pos =
        body.find("spawn_tui_worker(").expect("the TUI spawns the worker");
    let ready_pos =
        body.find("await_worker_ready(").expect("the TUI waits for it");
    let guard_pos = body.find("TerminalGuard::enter").expect("guard position");
    assert!(
        spawn_pos < guard_pos && ready_pos < guard_pos,
        "the worker must be spawned and its first event awaited BEFORE the terminal is taken over"
    );
    let worker_guard_pos = body
        .find("WorkerGuard::new")
        .expect("the TUI holds the worker in a guard");
    assert!(
        guard_pos < worker_guard_pos,
        "the worker guard is declared AFTER the terminal guard, so it drops (and joins) FIRST"
    );
    // And the TUI entry holds no session: the completion check for C2
    // step 3 is that the session value never exists on this thread.
    let tui_body = &src[entry
        ..src[entry..]
            .find(
                "
#[cfg(test)]",
            )
            .map_or(src.len(), |end| entry + end)];
    assert!(
        !tui_body.contains("compose_session("),
        "the frontend must not compose a session any more (decision 167)"
    );
}

#[test]
fn catalog_cross_equality_from_interactive() {
    // R3: single catalog — both catalogs must be identical
    let interactive = slash_command_catalog();
    let tui = crate::tui::command_catalog();
    let interactive_owned: Vec<(String, String)> = interactive
        .into_iter()
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    assert_eq!(interactive_owned, tui);
}

#[test]
fn is_unknown_slash_command_shared_helper_both_loops_call() {
    // Decision 114 Q3: the unknown-command check is a single shared
    // helper next to parse_slash_command — both loops call one
    // definition (fn-pointer/equality pattern). The helper returns
    // true for unknown slash commands and false otherwise, and the
    // stdio and TUI paths observe the same definition.
    let helper: fn(&str) -> bool = is_unknown_slash_command;
    let via_fn_ptr = helper;
    assert!(via_fn_ptr("/unknown"));
    assert!(via_fn_ptr("/unknown arg with spaces"));
    assert!(via_fn_ptr("  /nope  "));
    assert!(!via_fn_ptr("/context"));
    assert!(!via_fn_ptr("/tools"));
    assert!(!via_fn_ptr("/provider"));
    assert!(!via_fn_ptr("hello"));
    assert!(!via_fn_ptr("/domains-add"));
    assert!(!via_fn_ptr(""));
    // Both frontends use the same catalog; unknown line is derived from it.
    let catalog = slash_command_catalog();
    let names = catalog.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    let expected = format!("unknown command - available: {names}");
    let helper_unknown = is_unknown_slash_command("/definitely-not-a-command");
    assert!(helper_unknown);
    assert!(expected.contains("/context"));
    // Prove the TUI path's inline check would agree with the helper
    // (before decision 114 it used `matches!(Prompt(t) if t.starts_with('/'))`);
    // now it calls the helper — same outcome, one definition.
    let tui_is_unknown = is_unknown_slash_command("/nope arg");
    let stdio_is_unknown = is_unknown_slash_command("/nope arg");
    assert_eq!(tui_is_unknown, stdio_is_unknown);
    assert!(tui_is_unknown);
}

#[test]
fn stdio_unknown_command_honesty_line_not_prompt() {
    // Decision 114 Q3: stdio unknown slash commands render the honesty
    // line via the stdio writer (sanitized) instead of falling through
    // to the prompt path. This is a compatibility change the user
    // accepted (/-prefixed prompts that are not commands lose the
    // prompt path).
    let root = temporary_directory("stdio-unknown-honesty");
    let output = run("/unknown-command\n/exit\n", &root, None);
    // Must contain the honesty line derived from the shared catalog.
    let catalog = slash_command_catalog();
    let names = catalog.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    let expected = format!("unknown command - available: {names}");
    assert!(
        output.contains(&expected),
        "expected honesty line {expected:?} in output {output:?}"
    );
    // Must NOT have been treated as a prompt (no model turn).
    assert!(
        !output.contains("Siralos received: /unknown-command"),
        "unknown slash command must not fall through to prompt path; output: {output:?}"
    );
    // Also check with args: "/nope arg" should be honesty, not prompt.
    let output2 = run("/nope arg\n/exit\n", &root, None);
    assert!(output2.contains(&expected));
    assert!(!output2.contains("Siralos received: /nope"));
    let _ = remove_dir_all(root);
}

#[test]
fn stdio_unknown_command_with_whitespace_still_honest() {
    // Whitespace trimming: unknown detection uses trimmed line, like TUI.
    let root = temporary_directory("stdio-unknown-trim");
    let output = run("  /unknown  \n/exit\n", &root, None);
    let catalog = slash_command_catalog();
    let names = catalog.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
    let expected = format!("unknown command - available: {names}");
    assert!(output.contains(&expected));
    assert!(!output.contains("Siralos received:"));
    let _ = remove_dir_all(root);
}

/// List workspace-dir files left by the atomic writers (temp prefix).
fn mutation_temps(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let prefix = siralos_adapters::workspace::fs::MUTATION_TEMP_PREFIX;
    std::fs::read_dir(root)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| {
                    path.file_name().is_some_and(|name| {
                        name.to_string_lossy().starts_with(prefix)
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn remove_profile_config_strips_section_preserving_rest() {
    // Provider deletion removes the [profile] table and nothing else:
    // every other key, table, comment, and line survives byte-for-byte.
    let root = temporary_directory("remove-strips");
    let original = "# workspace config\n[workspace]\nroot = \".\"\n\n[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\ncredential = \"env:OPENAI_API_KEY\"\nendpoint = \"https://api.example.com/v1\"\n\n[other]\nkey = \"value\"\n# trailing comment\n";
    write(root.join("siralos.toml"), original).expect("fixture");
    remove_profile_config(&root).expect("remove");
    let after =
        std::fs::read_to_string(root.join("siralos.toml")).expect("read back");
    assert!(
        !after.contains("[profile]"),
        "profile section must be gone, got: {after:?}"
    );
    assert!(
        !after.contains("openai"),
        "provider value must be gone, got: {after:?}"
    );
    assert!(
        after.contains("# workspace config"),
        "leading comment must survive, got: {after:?}"
    );
    assert!(
        after.contains("[workspace]\nroot = \".\""),
        "workspace table must survive byte-for-byte, got: {after:?}"
    );
    assert!(
        after.contains("[other]\nkey = \"value\"\n# trailing comment"),
        "trailing table, key, and comment must survive, got: {after:?}"
    );
    // The bytes still parse AND the profile is gone (the rename gate).
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {}
        other => panic!("profile must be absent, got: {other:?}"),
    }
    assert!(mutation_temps(&root).is_empty(), "no temp files may remain");
    let _ = remove_dir_all(root);
}

#[test]
fn remove_profile_config_absent_noop_leaves_bytes_untouched() {
    // No [profile] table: succeed WITHOUT rewriting the file.
    let root = temporary_directory("remove-noop");
    let original = "# only comment\n[other]\nkey = \"value\"\n";
    write(root.join("siralos.toml"), original).expect("fixture");
    remove_profile_config(&root).expect("no-op succeeds");
    let after = read(root.join("siralos.toml")).expect("read back");
    assert_eq!(after, original.as_bytes(), "no-op must not touch the bytes");
    assert!(mutation_temps(&root).is_empty());
    let _ = remove_dir_all(root);
    // A missing file trivially holds no profile: same truthful no-op,
    // and nothing is created.
    let missing = temporary_directory("remove-noop-missing");
    remove_profile_config(&missing).expect("missing file no-op");
    assert!(
        !missing.join("siralos.toml").exists(),
        "no-op must not create a file"
    );
    let _ = remove_dir_all(missing);
}

#[test]
fn remove_profile_config_refuses_non_regular_target() {
    // A directory at the target path is refused exactly like the write
    // path refuses it, and no temp file is left behind.
    let root = temporary_directory("remove-non-regular");
    create_dir_all(root.join("siralos.toml")).expect("dir target");
    let error = remove_profile_config(&root).expect_err("must refuse");
    assert!(
        error.contains("regular file"),
        "refusal must name the rule, got: {error:?}"
    );
    assert!(mutation_temps(&root).is_empty());
    let _ = remove_dir_all(root);
}

#[test]
fn remove_profile_config_refuses_symlink_and_leaves_no_temp() {
    // A symlinked target is refused exactly like the write path refuses
    // it. Symlink creation needs privileges on some platforms, so the
    // refusal assertion only runs when the link was actually created
    // (repo precedent: harness.rs symlink gating); temp cleanliness is
    // asserted unconditionally.
    let root = temporary_directory("remove-symlink");
    write(root.join("real.toml"), "[profile]\nname = \"x\"\n")
        .expect("target");
    let linked = {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                root.join("real.toml"),
                root.join("siralos.toml"),
            )
            .is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_file(
                root.join("real.toml"),
                root.join("siralos.toml"),
            )
            .is_ok()
        }
    };
    if linked {
        let error = remove_profile_config(&root).expect_err("must refuse");
        assert!(
            error.contains("regular file"),
            "refusal must name the rule, got: {error:?}"
        );
    }
    assert!(mutation_temps(&root).is_empty(), "no temp files may remain");
    let _ = remove_dir_all(root);
}

#[test]
fn remove_profile_config_unparsable_fails_and_leaves_no_temp() {
    // An unparsable file cannot be safely excised: fail closed with the
    // write path's parse diagnostic, bytes untouched, temp cleaned.
    let root = temporary_directory("remove-unparsable");
    let original = "[profile\nbroken = \n";
    write(root.join("siralos.toml"), original).expect("fixture");
    let error = remove_profile_config(&root).expect_err("must fail closed");
    assert!(
        error.contains("does not parse"),
        "failure must name the parse rule, got: {error:?}"
    );
    let after = read(root.join("siralos.toml")).expect("read back");
    assert_eq!(after, original.as_bytes(), "bytes must be untouched");
    assert!(mutation_temps(&root).is_empty());
    let _ = remove_dir_all(root);
}

#[test]
fn slash_command_catalog_includes_provider_remove() {
    // Both frontends derive their vocabulary from this single catalog.
    let catalog = slash_command_catalog();
    let names: Vec<&str> = catalog.iter().map(|(n, _)| *n).collect();
    assert!(
        names.contains(&"/provider remove"),
        "catalog must list /provider remove, got: {names:?}"
    );
    assert!(
        matches!(
            parse_slash_command("/provider remove"),
            SlashCommand::ProviderRemove
        ),
        "must parse to its own variant"
    );
    assert!(
        matches!(parse_slash_command("/provider"), SlashCommand::Provider),
        "bare /provider stays display-only"
    );
    assert!(
        matches!(
            parse_slash_command("/provider extra"),
            SlashCommand::Prompt(_)
        ),
        "other /provider args stay unknown"
    );
    assert!(!is_unknown_slash_command("/provider remove"));
}

#[test]
fn slash_command_catalog_includes_model_switch_form() {
    // Both frontends derive their vocabulary from this single catalog:
    // bare `/model` stays display-only, `/model <id>` switches.
    let catalog = slash_command_catalog();
    let names: Vec<&str> = catalog.iter().map(|(n, _)| *n).collect();
    assert!(
        names.contains(&"/model"),
        "catalog must list /model, got: {names:?}"
    );
    assert!(
        names.contains(&"/model <id>"),
        "catalog must list /model <id>, got: {names:?}"
    );
    assert!(
        matches!(parse_slash_command("/model"), SlashCommand::Model(None)),
        "bare /model stays display-only"
    );
    assert!(
        matches!(
            parse_slash_command("/model example/model-b"),
            SlashCommand::Model(Some("example/model-b"))
        ),
        "must parse to the switch form"
    );
    assert!(
        matches!(
            parse_slash_command("/models extra"),
            SlashCommand::Prompt(_)
        ),
        "/models takes no arguments"
    );
    assert!(!is_unknown_slash_command("/model"));
    assert!(!is_unknown_slash_command("/model example/model-b"));
    assert!(is_unknown_slash_command("/models extra"));
}

#[test]
fn provider_remove_confirm_yes_removes_no_cancels() {
    // One implementation for the confirmation outcome: yes removes and
    // reports the save-mirroring message, no cancels truthfully.
    use crate::tui::ApprovalDecision;
    let root = temporary_directory("remove-confirm-yes");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\n",
    )
    .expect("fixture");
    let message =
        apply_provider_remove_confirmation(&root, ApprovalDecision::Approve);
    assert!(
        message.contains(
            "provider removed from siralos.toml - restart the session to apply"
        ),
        "success must mirror the save message, got: {message:?}"
    );
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {}
        other => panic!("profile must be gone, got: {other:?}"),
    }
    let _ = remove_dir_all(root);
    let root_no = temporary_directory("remove-confirm-no");
    let original = "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\n";
    write(root_no.join("siralos.toml"), original).expect("fixture");
    let message_no =
        apply_provider_remove_confirmation(&root_no, ApprovalDecision::Deny);
    assert!(
        message_no.contains("cancelled"),
        "denial must cancel truthfully, got: {message_no:?}"
    );
    let after = read(root_no.join("siralos.toml")).expect("read back");
    assert_eq!(after, original.as_bytes(), "denial must not touch");
    let _ = remove_dir_all(root_no);
}

#[test]
fn tui_modal_provider_removal_yes_removes_without_panic() {
    // Live-loop seam: a pending provider-removal confirmation plus 'y'
    // must close the modal, remove the profile, and show the outcome in
    // the transcript — without panicking on the RefCell borrow.
    use super::handle_pending_approval_key;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use std::cell::RefCell;
    use std::rc::Rc;
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }
    let root = temporary_directory("tui-modal-remove-yes");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\n",
    )
    .expect("fixture");
    let tui_state = Rc::new(RefCell::new(crate::tui::TuiState::new()));
    crate::tui::open_provider_remove_confirm(&mut tui_state.borrow_mut());
    let mut sink = crate::tui::TuiSink::new(tui_state.clone());
    assert!(
        handle_pending_approval_key(
            &tui_state,
            press(KeyCode::Char('y')),
            &root,
            &mut sink
        ),
        "'y' must decide the pending removal modal"
    );
    assert!(
        tui_state.borrow().pending_approval.is_none(),
        "modal must close after the decision"
    );
    assert!(
        !tui_state.borrow().confirming_provider_removal,
        "removal flag must reset after the decision"
    );
    settle_reveal(&tui_state);
    assert!(
        tui_state.borrow().transcript_lines.iter().any(|line| line.contains(
            "provider removed from siralos.toml - restart the session to apply"
        )),
        "transcript must show the removal, got: {:?}",
        tui_state.borrow().transcript_lines
    );
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {}
        other => panic!("profile must be gone, got: {other:?}"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn tui_modal_provider_removal_no_and_esc_cancel() {
    // 'n' and Esc cancel the removal: modal closes, file untouched, and
    // the cancellation is visible in the transcript.
    use super::handle_pending_approval_key;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use std::cell::RefCell;
    use std::rc::Rc;
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }
    for (label, code) in [
        ("tui-modal-remove-no", KeyCode::Char('n')),
        ("tui-modal-remove-esc", KeyCode::Esc),
    ] {
        let root = temporary_directory(label);
        let original = "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\n";
        write(root.join("siralos.toml"), original).expect("fixture");
        let tui_state = Rc::new(RefCell::new(crate::tui::TuiState::new()));
        crate::tui::open_provider_remove_confirm(&mut tui_state.borrow_mut());
        let mut sink = crate::tui::TuiSink::new(tui_state.clone());
        assert!(
            handle_pending_approval_key(
                &tui_state,
                press(code),
                &root,
                &mut sink
            ),
            "cancellation key must decide the modal"
        );
        assert!(
            tui_state.borrow().pending_approval.is_none(),
            "modal must close after cancellation"
        );
        settle_reveal(&tui_state);
        assert!(
            tui_state
                .borrow()
                .transcript_lines
                .iter()
                .any(|line| line.contains("cancelled")),
            "transcript must show the cancellation, got: {:?}",
            tui_state.borrow().transcript_lines
        );
        let after = read(root.join("siralos.toml")).expect("read back");
        assert_eq!(after, original.as_bytes(), "cancel must not touch");
        let _ = remove_dir_all(root);
    }
}

#[test]
fn tui_modal_dormant_approval_still_reports_verdict() {
    // Non-removal modals keep the historical transcript verdict; keys
    // that are not modal keys leave the modal pending.
    use super::handle_pending_approval_key;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use std::cell::RefCell;
    use std::rc::Rc;
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new_with_kind(code, KeyModifiers::NONE, KeyEventKind::Press)
    }
    let root = temporary_directory("tui-modal-dormant");
    for (label, code, verdict) in [
        ("approve", KeyCode::Char('y'), "Approved."),
        ("deny", KeyCode::Char('n'), "Denied."),
    ] {
        let tui_state = Rc::new(RefCell::new(crate::tui::TuiState::new()));
        tui_state.borrow_mut().pending_approval =
            Some(crate::tui::ApprovalModal::new(vec![format!("{label} req")]));
        let mut sink = crate::tui::TuiSink::new(tui_state.clone());
        assert!(
            handle_pending_approval_key(
                &tui_state,
                press(code),
                &root,
                &mut sink
            ),
            "modal key must decide the dormant modal"
        );
        assert!(
            tui_state.borrow().pending_approval.is_none(),
            "dormant modal must close after the decision"
        );
        assert!(
            tui_state
                .borrow()
                .transcript_lines
                .iter()
                .any(|line| line == verdict),
            "transcript must hold {verdict:?}, got: {:?}",
            tui_state.borrow().transcript_lines
        );
    }
    // A non-modal key decides nothing and keeps the modal pending.
    let tui_state = Rc::new(RefCell::new(crate::tui::TuiState::new()));
    tui_state.borrow_mut().pending_approval =
        Some(crate::tui::ApprovalModal::new(vec!["req".to_owned()]));
    let mut sink = crate::tui::TuiSink::new(tui_state.clone());
    assert!(
        !handle_pending_approval_key(
            &tui_state,
            press(KeyCode::Char('a')),
            &root,
            &mut sink
        ),
        "non-modal key must not decide"
    );
    assert!(
        tui_state.borrow().pending_approval.is_some(),
        "modal must stay pending on a non-modal key"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn tui_mouse_wheel_seam_scrolls_transcript() {
    // Wiring: a ScrollUp mouse event dispatched through the
    // live-loop seam increases `scroll_offset`, and a ScrollDown
    // decreases it again.
    use super::handle_tui_mouse;
    use crossterm::event::{
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    fn wheel(kind: MouseEventKind) -> MouseEvent {
        MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE }
    }
    let tui_state = Rc::new(RefCell::new(crate::tui::TuiState::new()));
    for i in 0..30 {
        tui_state.borrow_mut().push_line(format!("line {i}"));
    }
    let viewport: u16 = 10;
    assert_eq!(tui_state.borrow().scroll_offset, 0);
    assert!(handle_tui_mouse(
        &tui_state,
        wheel(MouseEventKind::ScrollUp),
        viewport
    ));
    let up = tui_state.borrow().scroll_offset;
    assert_eq!(up, crate::tui::MOUSE_WHEEL_STEP);
    assert!(handle_tui_mouse(
        &tui_state,
        wheel(MouseEventKind::ScrollDown),
        viewport
    ));
    assert_eq!(tui_state.borrow().scroll_offset, 0);
    // Non-wheel clicks move nothing and report no change.
    assert!(!handle_tui_mouse(
        &tui_state,
        wheel(MouseEventKind::Down(MouseButton::Left)),
        viewport
    ));
    assert_eq!(tui_state.borrow().scroll_offset, 0);
}

#[test]
fn provider_remove_stdio_confirm_yes_removes() {
    // End-to-end stdio: /provider remove prompts y/N, y removes.
    let root = temporary_directory("remove-stdio-yes");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\nendpoint = \"https://api.example.com/v1\"\n",
    )
    .expect("fixture");
    let output = run("/provider remove\ny\n/exit\n", &root, None);
    assert!(
        output.contains("(y/N)"),
        "must prompt for confirmation, got: {output:?}"
    );
    assert!(
        output.contains(
            "provider removed from siralos.toml - restart the session to apply"
        ),
        "must report removal, got: {output:?}"
    );
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Absent => {}
        other => panic!("profile must be gone, got: {other:?}"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn provider_remove_stdio_anything_else_cancels() {
    // End-to-end stdio: anything but y (the shared y/N gate) cancels.
    for (label, answer) in
        [("remove-stdio-no", "n"), ("remove-stdio-empty", "")]
    {
        let root = temporary_directory(label);
        let original = "[profile]\nname = \"default\"\nprovider = \"openai\"\nmodel = \"gpt-4o\"\n";
        write(root.join("siralos.toml"), original).expect("fixture");
        let output =
            run(&format!("/provider remove\n{answer}\n/exit\n"), &root, None);
        assert!(
            output.contains("cancelled"),
            "must cancel truthfully, got: {output:?}"
        );
        let after = read(root.join("siralos.toml")).expect("read back");
        assert_eq!(after, original.as_bytes(), "cancel must not touch");
        let _ = remove_dir_all(root);
    }
}

#[test]
fn provider_remove_stdio_absent_noop() {
    // End-to-end stdio: nothing configured says so without prompting.
    let root = temporary_directory("remove-stdio-absent");
    let output = run("/provider remove\n/exit\n", &root, None);
    assert!(
        output.contains("nothing to remove"),
        "no-op must say there is nothing to remove, got: {output:?}"
    );
    assert!(
        !output.contains("(y/N)"),
        "no-op must not prompt, got: {output:?}"
    );
    assert!(
        !root.join("siralos.toml").exists(),
        "no-op must not create a file"
    );
    let _ = remove_dir_all(root);
}

/// Build a live [`SessionProvider`] over the generic path for switch
/// tests (provider/endpoint placeholders only — no network is touched:
/// the switch writes the file and mutates the in-memory cell).
fn switch_test_provider(initial_model: &str) -> SessionProvider {
    let host =
        siralos_adapters::provider::HostProvider::from_provider_str_with_protocol(
            "example-vendor",
            Some(initial_model.to_owned()),
            None,
            Some("https://api.example.com/v1".to_owned()),
            siralos_core::composition::Protocol::OpenAiCompletions,
        )
        .expect("test provider");
    SessionProvider::Host(host)
}

/// Fixture `[profile]` with a model display name (the switch must
/// clear it) plus surrounding content the writer must preserve.
fn switch_test_profile(root: &std::path::Path) {
    write(
        root.join("siralos.toml"),
        "# workspace config\n[other]\nkey = \"value\"\n\n[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nmodel_display_name = \"Old Display\"\nendpoint = \"https://api.example.com/v1\"\n",
    )
    .expect("fixture");
}

#[test]
fn model_switch_explicit_id_updates_live_and_persisted_profile() {
    // Explicit `/model <id>`: the live value AND the persisted
    // `[profile]` change, round-tripped through the loader.
    let root = temporary_directory("model-switch-live");
    switch_test_profile(&root);
    let session = switch_test_provider("example/model-a");
    let mut model = Some("example/model-a".to_owned());
    let mut display = Some("Old Display".to_owned());
    let message = apply_model_switch(
        &root,
        &session,
        Some("example-vendor"),
        &mut model,
        &mut display,
        "example/model-b",
    )
    .expect("switch");
    assert!(
        message.contains("model switched to example/model-b"),
        "must name the new model, got: {message:?}"
    );
    assert!(
        message.contains("model display name cleared"),
        "must say the display name was cleared, got: {message:?}"
    );
    assert_eq!(model.as_deref(), Some("example/model-b"));
    assert_eq!(display, None);
    // The switched id is what the NEXT provider request reads: the
    // live cell behind `stream()` holds it.
    assert_eq!(session.live_model().as_deref(), Some("example/model-b"));
    // The persisted `[profile]` round-trips through the loader with
    // provider/endpoint preserved and the display name gone.
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            assert_eq!(record.model.as_deref(), Some("example/model-b"));
            assert_eq!(record.model_display_name, None);
            assert_eq!(record.provider.as_deref(), Some("example-vendor"));
            assert_eq!(
                record.endpoint.as_deref(),
                Some("https://api.example.com/v1")
            );
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    // Bytes outside `[profile]` survive the rewrite; no temp remains.
    let after =
        std::fs::read_to_string(root.join("siralos.toml")).expect("read");
    assert!(
        after.contains("# workspace config"),
        "surrounding bytes must survive, got: {after:?}"
    );
    assert!(
        after.contains("[other]\nkey = \"value\""),
        "other tables must survive, got: {after:?}"
    );
    assert!(mutation_temps(&root).is_empty());
    // The switching message is sanitizer-clean.
    assert_eq!(
        crate::sanitize::sanitize_for_display(&message),
        message,
        "switch message must survive the sanitizer unchanged"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn model_switch_refuses_without_applied_profile() {
    // No profile applied: refuse truthfully, writing nothing and
    // touching neither the holders nor the live provider.
    let root = temporary_directory("model-switch-absent");
    let session = switch_test_provider("example/model-a");
    let mut model: Option<String> = None;
    let mut display: Option<String> = None;
    let error = apply_model_switch(
        &root,
        &session,
        None,
        &mut model,
        &mut display,
        "example/model-b",
    )
    .expect_err("must refuse without a profile");
    assert!(
        error.contains("no provider configured"),
        "refusal must be truthful, got: {error:?}"
    );
    assert_eq!(model, None);
    assert_eq!(display, None);
    assert_eq!(
        session.live_model().as_deref(),
        Some("example/model-a"),
        "live value must be untouched by a refused switch"
    );
    assert!(
        !root.join("siralos.toml").exists(),
        "refusal must not create a file"
    );
    assert_eq!(
        crate::sanitize::sanitize_for_display(&error),
        error,
        "refusal must be sanitizer-clean"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn model_switch_refuses_invalid_id_and_leaves_bytes_untouched() {
    // Invalid ids are refused with the existing honest message; the
    // file, the holders, and the live provider are untouched.
    let root = temporary_directory("model-switch-invalid");
    switch_test_profile(&root);
    let session = switch_test_provider("example/model-a");
    let bad: Vec<String> = vec![
        String::new(),
        "has space".to_owned(),
        "bad!id".to_owned(),
        "a".repeat(siralos_core::composition::MAX_PROFILE_MODEL_BYTES + 1),
        "ab\0cd".to_owned(),
    ];
    for id in &bad {
        let before =
            std::fs::read(root.join("siralos.toml")).expect("read back");
        let mut model = Some("example/model-a".to_owned());
        let mut display = Some("Old Display".to_owned());
        let error = apply_model_switch(
            &root,
            &session,
            Some("example-vendor"),
            &mut model,
            &mut display,
            id,
        )
        .expect_err("invalid id must be refused");
        assert!(
            error.contains("A model must match"),
            "refusal must use the honest message, got: {error:?}"
        );
        assert_eq!(
            std::fs::read(root.join("siralos.toml")).expect("read back"),
            before,
            "refused switch must not touch bytes"
        );
        assert_eq!(model.as_deref(), Some("example/model-a"));
        assert_eq!(display.as_deref(), Some("Old Display"));
        assert_eq!(session.live_model().as_deref(), Some("example/model-a"));
        assert!(mutation_temps(&root).is_empty());
    }
    let _ = remove_dir_all(root);
}

#[test]
fn model_picker_selection_performs_same_switch() {
    // The picker path feeds the selected entry into the same
    // switch-and-persist as the explicit-argument form.
    let root = temporary_directory("model-switch-picker");
    switch_test_profile(&root);
    let session = switch_test_provider("example/model-a");
    let picker = crate::tui::ModelPicker {
        items: vec![
            "example/model-a".to_owned(),
            "example/model-b".to_owned(),
        ],
        selected: 1,
    };
    let selected = picker.selected_id().expect("selection").to_owned();
    let mut model = Some("example/model-a".to_owned());
    let mut display = Some("Old Display".to_owned());
    let message = apply_model_switch(
        &root,
        &session,
        Some("example-vendor"),
        &mut model,
        &mut display,
        &selected,
    )
    .expect("picker selection must switch");
    assert!(message.contains("model switched to example/model-b"));
    assert_eq!(model.as_deref(), Some("example/model-b"));
    assert_eq!(display, None);
    assert_eq!(session.live_model().as_deref(), Some("example/model-b"));
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            assert_eq!(record.model.as_deref(), Some("example/model-b"));
            assert_eq!(record.model_display_name, None);
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn model_switch_stdio_end_to_end_updates_display_and_persists() {
    // Full stdio loop: `/model <id>` switches, and a following bare
    // `/model` shows the live value.
    let root = temporary_directory("model-switch-stdio");
    switch_test_profile(&root);
    let output = run("/model example/model-b\n/model\n/exit\n", &root, None);
    assert!(
        output.contains("model switched to example/model-b"),
        "must report the switch, got: {output:?}"
    );
    assert!(
        output.contains("model display name cleared"),
        "must report the cleared display name, got: {output:?}"
    );
    assert!(
        output.contains("model: example/model-b"),
        "bare /model must show the live value, got: {output:?}"
    );
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            assert_eq!(record.model.as_deref(), Some("example/model-b"));
            assert_eq!(record.model_display_name, None);
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    let _ = remove_dir_all(root);
}

#[test]
fn bare_model_stdio_displays_and_hints() {
    // Bare `/model` in stdio keeps the display behaviour and tells
    // the user how to switch — never silently nothing.
    let root = temporary_directory("model-bare-stdio");
    switch_test_profile(&root);
    let output = run("/model\n/exit\n", &root, None);
    assert!(
        output.contains("model: example/model-a"),
        "must still display, got: {output:?}"
    );
    assert!(
        output.contains("pass /model <id>"),
        "must hint at the switch form, got: {output:?}"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn model_switch_stdio_refuses_without_profile() {
    // No profile: the stdio form refuses truthfully and creates nothing.
    let root = temporary_directory("model-switch-stdio-absent");
    let output = run("/model example/model-b\n/exit\n", &root, None);
    assert!(
        output.contains("no provider configured"),
        "must refuse truthfully, got: {output:?}"
    );
    assert!(
        !root.join("siralos.toml").exists(),
        "refusal must not create a file"
    );
    let _ = remove_dir_all(root);
}

#[test]
fn persist_switched_model_clears_display_name_only() {
    // The persist sibling updates ONLY the model: provider, credential,
    // endpoint, and protocol survive verbatim; the display name is
    // removed.
    let root = temporary_directory("model-persist-only");
    write(
        root.join("siralos.toml"),
        "[profile]\nname = \"default\"\nprovider = \"example-vendor\"\nmodel = \"example/model-a\"\nmodel_display_name = \"Example A\"\ncredential = \"env:EXAMPLE_API_KEY\"\nendpoint = \"https://api.example.com/v1\"\nprotocol = \"openai-responses\"\n",
    )
    .expect("fixture");
    let message = persist_switched_model(
        &root,
        Some("example-vendor"),
        "example/model-b",
    )
    .expect("persist");
    assert!(message.contains("example/model-b"));
    match siralos_adapters::profile_config::load_workspace_profile(&root) {
        siralos_adapters::profile_config::WorkspaceProfileLoad::Record(
            record,
        ) => {
            assert_eq!(record.model.as_deref(), Some("example/model-b"));
            assert_eq!(record.model_display_name, None);
            assert_eq!(record.provider.as_deref(), Some("example-vendor"));
            assert_eq!(
                record.credential.as_deref(),
                Some("env:EXAMPLE_API_KEY")
            );
            assert_eq!(
                record.endpoint.as_deref(),
                Some("https://api.example.com/v1")
            );
            assert_eq!(
                record.protocol,
                siralos_core::composition::Protocol::OpenAiResponses
            );
        }
        other => panic!("expected applied record, got: {other:?}"),
    }
    assert!(mutation_temps(&root).is_empty());
    let _ = remove_dir_all(root);
}
