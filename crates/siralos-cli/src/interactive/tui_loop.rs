//! The TUI event loop: the TUI slash-command dispatch and the
//! terminal-loop runner that drives the shared worker session.

use super::{
    InteractiveError, InteractiveOptions, ModelListingGate, Path, Rc, RefCell,
    SlashCommand, TuiState, Until, VecDeque, WorkerCommand, WorkerEvent,
    WorkerGuard, WorkerSource, Write, ask_worker, ask_worker_effects,
    await_worker_ready, drain_pending_worker, drain_ui_spec_form,
    execute_ui_remove, handle_pending_approval_key_outcome, handle_tui_mouse,
    io, is_unknown_slash_command, load_sourced_user_specs,
    load_workspace_profile_write_token, model_listing_diagnostic,
    model_listing_gate, open_model_picker_via_worker, parse_slash_command,
    relay_suspended_turn, render_answer, render_evolve_lines,
    render_model_line, render_modules, render_provider_line_display,
    render_ui_list_text, resolve_workspace_root, safe_alias_for_display,
    sanitize_for_display, slash_command_catalog, spawn_tui_worker,
    switch_model_via_worker, transient_status, write_profile_config_at,
};

/// UI panels refresh (ADR 0039 P4): entering the view with no
/// staged snapshot fetches one panel report; the pump arm stages
/// the snapshot and clears nothing else. Returns whether a fetch
/// ran (test seam; the loop ignores the answer either way).
pub(super) fn refresh_panels_if_requested(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
) -> bool {
    let refresh = {
        let mut guard = state.borrow_mut();
        if guard.panels_view
            && guard.panels.is_none()
            && guard.panels_refresh_pending
        {
            guard.panels_refresh_pending = false;
            true
        } else {
            false
        }
    };
    if !refresh {
        return false;
    }
    let _ = ask_worker(
        worker,
        sink,
        state,
        pane,
        progress,
        reasoning,
        WorkerCommand::PanelsReport,
        Until::Answer,
    );
    true
}

/// Dispatch one parsed [`SlashCommand`] to the TUI sink — the second thin
/// per-frontend writer over the shared parse + render helpers, and since C2
/// step 3 the frontend half of the worker boundary.
///
/// The session is NOT here any more (decision 167). An arm that needs it sends
/// a [`WorkerCommand`] and relays the answer; an arm that only renders reads
/// the cached `Ready` snapshot. The capability state the old signature carried
/// — the tool definitions, the permission policy, the module hosts and
/// manifests, the module selection and the context control — went with it
/// (decision 168 R4: capability is authority, and the frontend may not hold
/// authority it cannot enforce). Returns `true` when the loop must exit.
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_tui_command(
    command: &SlashCommand<'_>,
    workspace_root: &Path,
    state: &Rc<RefCell<TuiState>>,
    sink: &mut crate::tui::TuiSink,
    worker: &mut WorkerSource,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
) -> Result<bool, InteractiveError> {
    match command {
        // `/context` and `/tools` read capability state to RENDER it, and the
        // render belongs where the control is applied (decision 168 R4): the
        // worker holds the projection, the context control and the audit, and
        // answers with the very bytes the frontend used to build.
        SlashCommand::Context => {
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::ContextReport,
                Until::Answer,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::Tools => {
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::ToolsReport,
                Until::Answer,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::Modules => {
            // Display only, and the workspace root is frontend state (R1), so
            // this stays a local render.
            let rendered =
                sanitize_for_display(&render_modules(workspace_root));
            let _ = sink.write_all(rendered.as_bytes());
        }
        SlashCommand::Exit => return Ok(true),
        // The three module arms MUTATE the session's module registry and
        // activate hosts, so the session does it (decision 168 R4). The worker
        // calls the very same `render_*` helpers, so neither the report nor the
        // side effect forks.
        SlashCommand::ModulesAdd(folder) => {
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::ModulesAdd(folder.unwrap_or("").to_owned()),
                Until::Answer,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::ModulesEnable(id) => {
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::ModulesEnable(id.unwrap_or("").to_owned()),
                Until::Answer,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::ModulesActivate(id) => {
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::ModulesActivate(id.unwrap_or("").to_owned()),
                Until::Answer,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::UiList => {
            // Display only from the workspace file: local render.
            let rendered =
                sanitize_for_display(&render_ui_list_text(workspace_root));
            let _ = sink.write_all(rendered.as_bytes());
        }
        SlashCommand::UiAdd => {
            // Opens the spec form; submit drains through the worker
            // boundary via the completed form (input.rs drives).
            state.borrow_mut().ui_spec_form =
                Some(crate::tui::UiSpecForm::new_add());
        }
        SlashCommand::UiEdit(id) => {
            let Some(id) = id.filter(|id| !id.is_empty()) else {
                let _ = sink.write_all(
                    sanitize_for_display("usage: /ui-edit <id>\n").as_bytes(),
                );
                return Ok(false);
            };
            match load_sourced_user_specs(workspace_root)
                .unwrap_or_default()
                .into_iter()
                .find(|each| each.spec.id() == id)
            {
                Some(item) => {
                    state.borrow_mut().ui_spec_form =
                        Some(crate::tui::UiSpecForm::new_edit(id, &item.spec));
                }
                None => {
                    let _ = sink.write_all(
                        sanitize_for_display(&format!(
                            "no ui component {id:?}\n"
                        ))
                        .as_bytes(),
                    );
                }
            }
        }
        SlashCommand::UiRemove(id) => {
            // File-local mutation under a fresh observation: no
            // session state involved, so no worker round trip.
            let rendered = sanitize_for_display(&execute_ui_remove(
                workspace_root,
                id.unwrap_or(""),
            ));
            let _ = sink.write_all(rendered.as_bytes());
        }
        SlashCommand::UiPreview(id) => {
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::UiPreview(id.unwrap_or("").to_owned()),
                Until::Answer,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::Provider => {
            // Rendered from the cached snapshot. The raw credential never
            // crosses (decision 168 R2), so the display form is what shows —
            // and it is already redacted where it lives.
            let (provider, credential) = {
                let state = state.borrow();
                (state.provider.clone(), state.credential_display.clone())
            };
            let rendered =
                sanitize_for_display(&render_provider_line_display(
                    provider.as_deref(),
                    credential.unwrap_or_else(|| "absent".to_owned()),
                ));
            let _ = sink.write_all(rendered.as_bytes());
        }
        SlashCommand::ProviderRemove => {
            // Intercepted in-loop (opening the confirmation needs the
            // `TuiState`, which this writer does not hold), so this arm is
            // unreachable — kept only for exhaustiveness.
        }
        SlashCommand::Model(argument) => {
            // Bare `/model` is intercepted in-loop (it opens the switch
            // picker), so the display fallback here is for direct callers.
            // `Some(id)` is the same switch-and-persist as the stdio form,
            // split across the boundary by decision 167 D3.
            match argument {
                None => {
                    let model = state.borrow().model.clone();
                    let rendered = sanitize_for_display(&render_model_line(
                        model.as_deref(),
                        None,
                    ));
                    let _ = sink.write_all(rendered.as_bytes());
                }
                Some(id) => {
                    switch_model_via_worker(
                        workspace_root,
                        sink,
                        state,
                        worker,
                        pane,
                        progress,
                        reasoning,
                        id,
                    )?;
                }
            }
        }
        SlashCommand::Models => {
            // I6: the fetch reads the endpoint and the credential, so it runs
            // where they live (decision 168 R2) and only the ids cross. The
            // gate is shared with stdio: an absent credential is valid for a
            // public Generic endpoint, while a declared-but-unresolved one is
            // a distinct refusal rather than an anonymous request.
            let gate = {
                let state = state.borrow();
                model_listing_gate(
                    state.provider.as_deref(),
                    state.endpoint.as_deref(),
                    state.credential_display.is_some(),
                    state.credential_resolved,
                )
            };
            if gate != ModelListingGate::Ready {
                let sanitized =
                    sanitize_for_display(model_listing_diagnostic(gate));
                let _ = sink.write_all(sanitized.as_bytes());
            } else {
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
                    Some(WorkerEvent::Models(models)) => {
                        if models.is_empty() {
                            let line = "no models returned\n";
                            let _ = sink.write_all(
                                sanitize_for_display(line).as_bytes(),
                            );
                        } else {
                            for id in models {
                                let line = format!(
                                    "{}\n",
                                    safe_alias_for_display(&id)
                                );
                                let sanitized = sanitize_for_display(&line);
                                let _ = sink.write_all(sanitized.as_bytes());
                            }
                        }
                    }
                    Some(WorkerEvent::Failed(_message)) => {
                        let line = "models fetch error (details hidden)\n";
                        let sanitized = sanitize_for_display(line);
                        let _ = sink.write_all(sanitized.as_bytes());
                    }
                    // The relay has already reported a worker that stopped.
                    _ => {}
                }
            }
        }
        SlashCommand::Evolve => {
            let rendered = sanitize_for_display(&render_evolve_lines());
            let _ = sink.write_all(rendered.as_bytes());
        }
        SlashCommand::Reload => {
            // The reload READS the profile, RECOMPOSES and moves the live
            // cells, so it runs with the session (decision 167 D3) and returns
            // the report this frontend shows. The header is re-announced with
            // it, so `/reload` no longer leaves a stale one behind.
            let answer = ask_worker(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::Reload,
                Until::Applied,
            )?;
            render_answer(answer, sink)?;
        }
        SlashCommand::Mouse => {
            // Intercepted in-loop (flipping needs the live `TuiState` plus
            // the `TerminalGuard` re-pair held by the loop), so this
            // sink-only arm is unreachable — kept for exhaustiveness.
        }
        SlashCommand::Prompt(prompt) => {
            // The turn runs where the session is. The relay renders the whole
            // turn -- deltas through the sanitizer, the thinking to its sink,
            // the pane snapshots, the demand tick's effect and the end signal.
            // A tool approval suspends the turn mid-round instead of ending
            // it: the relay arms the modal (with the exchange id in
            // `pending_tool_approval`) and the main loop below routes its
            // keys, delivers the verdict, and resumes the turn.
            let _ = ask_worker_effects(
                worker,
                sink,
                state,
                pane,
                progress,
                reasoning,
                WorkerCommand::Prompt((*prompt).to_owned()),
                Until::TurnFinished,
            )?;
        }
    }
    Ok(false)
}

/// Deliver one tool-approval modal verdict straight into the shared
/// exchange (Part B).
///
/// Provider-removal confirmations resolve locally and never reach
/// here (their `pending_tool_approval` stays `None`). Returns
/// whether a verdict was delivered: `false` means no tool approval
/// was pending (nothing to do) or the exchange no longer holds the
/// id (stale double keypress after a shutdown unblocked the hook —
/// loud, since the worker may still be waiting on the stop flag).
pub(super) fn deliver_tool_approval_verdict(
    state: &Rc<RefCell<TuiState>>,
    exchange: &super::ApprovalExchange,
    sink: &mut crate::tui::TuiSink,
    decision: crate::tui::ApprovalDecision,
) -> bool {
    let Some(id) = state.borrow().pending_tool_approval else {
        return false;
    };
    let verdict = match decision {
        crate::tui::ApprovalDecision::Approve => {
            siralos_core::tool::ApprovalVerdict::Approve
        }
        crate::tui::ApprovalDecision::Deny => {
            siralos_core::tool::ApprovalVerdict::Deny
        }
    };
    if !exchange.deliver(id, verdict) {
        let _ = sink.write_all(
            sanitize_for_display(
                "approval delivery failed (details hidden)\n",
            )
            .as_bytes(),
        );
        return false;
    }
    true
}

/// Resume a verdict-delivered tool-approval turn (Part B).
///
/// Clears the exchange id first (a fresh suspension re-arms it),
/// re-enters the still-running turn, and ends it when it completes.
/// A re-suspension leaves the turn open for the main loop's next
/// verdict; the `working` clock and thinking anchor stay armed
/// across the whole suspension. A relay error propagates (the loop
/// exits and the worker is shut down; the clock stays armed on that
/// dead session, which is benign because nothing renders it again).
pub(super) fn resume_suspended_turn(
    worker: &mut WorkerSource,
    sink: &mut crate::tui::TuiSink,
    state: &Rc<RefCell<TuiState>>,
    pane: &Rc<RefCell<Option<crate::tui::ContextPaneData>>>,
    progress: &mut dyn FnMut() -> bool,
    reasoning: &mut dyn FnMut(&str),
) -> Result<(), InteractiveError> {
    state.borrow_mut().pending_tool_approval = None;
    relay_suspended_turn(worker, sink, state, pane, progress, reasoning)?;
    // A fresh suspension re-arms the exchange id above; only a
    // completed turn ends (ending a re-suspended turn would stop the
    // `working` clock and disarm the thinking anchor mid-turn).
    if state.borrow().pending_tool_approval.is_none() {
        state.borrow_mut().end_turn();
    }
    Ok(())
}

/// Run the TUI shell session (the default frontend when stdout is a TTY;
/// `--stdio` forces the stdio frontend, plain non-TTY uses stdio silently).
///
/// T1 composition: this function reuses the SAME seams the stdio loop calls
/// (`ensure_host`, the command dispatch, `drain_events` with a [`crate::tui::TuiSink`],
/// the sanitizer, the input-queue/command-catalog vocabulary). Because
/// `run_interactive_session` blocks on `BufRead::read_line`, it would starve the
/// `crossterm::event::poll` pump, so this loop calls the SAME shared helpers
/// through its own terminal wiring. T2 consolidated the approval surface; T3
/// consolidated the audit/pane gating; T4 (decision 108) settles the FINAL
/// ledger — shared: `parse_slash_command`,
/// `render_context_segment`/`render_tools_segment` (now called by the worker),
/// `dispatch_stdio_command`/`dispatch_tui_command`, `handle_key`,
/// `flush_record_replay` (now the worker's). Permanent residual: the TUI loop
/// owns the `TerminalGuard`/`Terminal`/`TuiState`/`TuiSink` terminal state (plus
/// Ctrl+C-exit, the PageUp viewport lookup, and the modal verdict lines).
///
/// C2 step 3 (ticket 130): the SESSION IS NOT HERE. The worker composes it
/// (decision 167), this loop holds only commands, events and the cached
/// `Ready`/`Pane` snapshots, and the two talk over the channels in
/// [`crate::session_worker`]. The provider's cadence no longer sets the frame
/// cadence: the relay ticks on its own clock while the worker is silent.
///
/// The terminal state is restored via a drop guard on every exit path
/// (panic-safe), and the worker is stopped and joined before that guard runs
/// (decision 168 R6).
pub fn run_interactive_tui_stdio() -> Result<(), InteractiveError> {
    run_interactive_tui_with_options(InteractiveOptions::default())
}

/// Run the TUI shell with explicit options (workspace root / config path).
pub fn run_interactive_tui_with_options(
    options: InteractiveOptions<'_>,
) -> Result<(), InteractiveError> {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::tui::{TerminalGuard, TuiSink, TuiState, draw_with_pane};

    // --- The worker composes the session BEFORE the alternate screen (R6) ---
    // Startup diagnostics (lock drift, skill/context warnings, profile and
    // credential errors) are `eprintln` from the worker thread. This waits for
    // its first event, so every one of them is on the normal screen before the
    // alternate screen exists — and a composition failure is reported exactly
    // where the synchronous composition used to report it.
    //
    // R1: the frontend keeps the workspace root, because it owns the profile
    // writes (`/provider`, `/provider remove`, `/model`).
    let workspace_root = match options.workspace_root {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(InteractiveError::CurrentDirectory)?,
    };
    let workspace_root = resolve_workspace_root(&workspace_root)?;
    let pane_cache: Rc<RefCell<Option<crate::tui::ContextPaneData>>> =
        Rc::new(RefCell::new(None));
    let tui_state = Rc::new(RefCell::new(TuiState::new()));
    let worker = await_worker_ready(
        WorkerSource::new(spawn_tui_worker(
            &workspace_root,
            options.config_path,
        )),
        &tui_state,
        &pane_cache,
    )?;

    // Guard restores raw mode + alternate screen on every exit path.
    // `mut`: `/mouse` re-pairs the terminal escape through it in-loop.
    let mut _guard = match TerminalGuard::enter() {
        Ok(guard) => guard,
        Err(error) => {
            // No terminal was taken over, but the worker is already running:
            // stop it and WAIT, so its one flush happens on this exit path too.
            let shutdown = WorkerGuard::new(worker).shutdown_result();
            return Err(match shutdown {
                Ok(()) => InteractiveError::Io(error),
                Err(reason) => InteractiveError::Io(io::Error::other(
                    format!("{error}; worker shutdown failed: {reason}"),
                )),
            });
        }
    };
    // C2 step 4: declared AFTER the terminal guard, so it drops FIRST — the
    // worker is stopped and joined (and the recordings flushed exactly once, by
    // the one owner) before the terminal is restored, on EVERY exit path.
    let mut worker = WorkerGuard::new(worker);
    // Keep every fallible terminal/UI operation inside one fallible scope. The
    // worker is stopped after the scope returns, so an early `?` still gets a
    // typed flush/quiesce result instead of relying only on `Drop`'s log.
    let run_result = (|| -> Result<(), InteractiveError> {
        let backend =
            ratatui::backend::CrosstermBackend::new(std::io::stdout());
        // S2 chunk 4: the terminal is SHARED, because the sink must be able to
        // ask for a frame while a streamed turn is arriving -- the loop itself
        // is blocked inside the drain at that moment.
        let terminal =
            Rc::new(RefCell::new(ratatui::Terminal::new(backend).map_err(
                |e| InteractiveError::Io(io::Error::other(e.to_string())),
            )?));
        {
            // H2: banner + greeting at session start (TUI-only, stdio unchanged).
            // The header itself arrived with `Ready` and was applied before the
            // terminal was taken over, so this only prepends the greeting.
            let mut state = tui_state.borrow_mut();
            crate::tui::push_banner_and_greeting(&mut state);
        }
        let mut sink = TuiSink::new(tui_state.clone());

        // One place that paints a frame, callable from the loop and from the
        // sink. It never blocks: a frame already in progress is skipped. The
        // FIRST failure is kept here and reported, so a dead terminal is a
        // diagnostic rather than a frozen UI.
        let draw_error: Rc<RefCell<Option<String>>> =
            Rc::new(RefCell::new(None));
        let draw_now = {
            let terminal = Rc::clone(&terminal);
            let tui_state = Rc::clone(&tui_state);
            let pane_cache = Rc::clone(&pane_cache);
            let draw_error = Rc::clone(&draw_error);
            move || {
                // S3c: every paint releases exactly ONE character of the text the
                // reader is owed (owner ruling: the text renders a character at a
                // time), so the reveal and the frame are the same event -- the
                // cadence at which frames are painted IS the character rate.
                tui_state.borrow_mut().reveal_char();
                if let Ok(mut terminal) = terminal.try_borrow_mut() {
                    // A frame already in progress is skipped, but a REAL failure
                    // (a dead terminal) is kept and reported once, instead of
                    // leaving a frozen UI with no diagnostic.
                    if let Err(error) = terminal.draw(|frame| {
                        // UI panels view (ADR 0039): an explicit mode
                        // with its own draw path. The transcript path
                        // below runs byte-identical when inactive.
                        if tui_state.borrow().panels_view {
                            crate::tui::draw_panels(&tui_state.borrow(), frame)
                        } else {
                            draw_with_pane(
                                &tui_state.borrow(),
                                pane_cache.borrow().as_ref(),
                                frame,
                            )
                        }
                    }) {
                        let mut slot = draw_error.borrow_mut();
                        if slot.is_none() {
                            *slot = Some(error.to_string());
                        }
                    }
                }
            }
        };
        // Each painted frame releases ONE character (the reveal and the frame are
        // the same event), so while the reader is owed text this throttle is OPEN:
        // the text tracks the model at whatever rate frames can be painted, which
        // is what "match the speed the model produces it" means. Once nothing is
        // owed the ordinary redraw interval applies again. A key press forces a
        // frame so expanding is instant.
        let draw_throttled = {
            let draw_now = draw_now.clone();
            let tui_state = Rc::clone(&tui_state);
            let last =
                Rc::new(std::cell::Cell::new(None::<std::time::Instant>));
            move || {
                let now = std::time::Instant::now();
                let interval = crate::tui::paint_interval(
                    tui_state.borrow().reveal_pending(),
                    crate::tui::REDRAW_INTERVAL,
                );
                let due = match last.get() {
                    None => true,
                    Some(previous) => now.duration_since(previous) >= interval,
                };
                if due {
                    last.set(Some(now));
                    draw_now();
                }
            }
        };
        {
            let hook: Rc<dyn Fn()> = Rc::new(draw_throttled.clone());
            sink.set_redraw(hook);
        }

        // S3: the thinking sink. It buffers the streamed reasoning (bounded to
        // the tail), crosses the terminal sanitizer, and repaints.
        let mut reasoning_sink = {
            let tui_state = Rc::clone(&tui_state);
            let draw_now = draw_throttled.clone();
            // The reasoning channel is model output too, so it crosses the SAME
            // terminal sanitizer: a stateful one, because an escape can be split
            // across deltas. Without this, raw provider bytes would reach the
            // frame (AGENTS.md: the sanitizer is the single output boundary).
            let mut sanitizer = crate::sanitize::TerminalSanitizer::new();
            let mut last_epoch = 0u64;
            move |text: &str| {
                let epoch = tui_state.borrow().turn_epoch();
                if epoch != last_epoch {
                    let _ = sanitizer.flush();
                    last_epoch = epoch;
                }
                let safe = sanitizer.push(text);
                tui_state.borrow_mut().push_reasoning(&safe);
                draw_now();
            }
        };
        let progress_error: Rc<RefCell<Option<String>>> =
            Rc::new(RefCell::new(None));
        // S2 chunk 4b: what the TUI does with a keep-alive tick -- repaint,
        // keep what the user typed while the model works, and read the
        // interrupt key. Returns true when the user asked to cancel.
        let interrupt = Rc::new(std::cell::Cell::new(false));
        let exit_requested = Rc::new(std::cell::Cell::new(false));
        let mut progress = {
            let tui_state = Rc::clone(&tui_state);
            let interrupt = Rc::clone(&interrupt);
            let exit_requested = Rc::clone(&exit_requested);
            let draw_now = draw_now.clone();
            let draw_throttled = draw_throttled.clone();
            let progress_error = Rc::clone(&progress_error);
            move || -> bool {
                use crossterm::event::Event;
                let mut handled_key = false;
                loop {
                    match crossterm::event::poll(std::time::Duration::ZERO) {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => {
                            let mut slot = progress_error.borrow_mut();
                            if slot.is_none() {
                                *slot = Some(error.to_string());
                            }
                            break;
                        }
                    }
                    match crossterm::event::read() {
                        Ok(Event::Key(key)) => {
                            if key.kind
                                != crossterm::event::KeyEventKind::Press
                            {
                                continue;
                            }
                            handled_key = true;
                            let is_ctrl_c = key.code
                                == crossterm::event::KeyCode::Char('c')
                                && key.modifiers.contains(
                                    crossterm::event::KeyModifiers::CONTROL,
                                );
                            if crate::tui::apply_turn_key(
                                &mut tui_state.borrow_mut(),
                                key,
                            ) {
                                interrupt.set(true);
                                if is_ctrl_c {
                                    exit_requested.set(true);
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            let mut slot = progress_error.borrow_mut();
                            if slot.is_none() {
                                *slot = Some(error.to_string());
                            }
                            break;
                        }
                    }
                }
                if handled_key {
                    draw_now();
                } else {
                    draw_throttled();
                }
                interrupt.replace(false)
            }
        };

        // Initial draw. The context pane is already cached: the worker pushed it
        // before the header (T3's gate — opted in AND built — is the WORKER's now,
        // and an opted-out session simply receives no pane, byte-identical to T2).
        draw_now();

        // Event loop: P1 zero-timeout drain + immediate draw, outer 50ms idle poll.
        // Keep a bounded FIFO so a burst of Enter presses cannot overwrite an
        // earlier accepted prompt. Each turn consumes one entry; excess entries
        // are rejected visibly rather than silently dropped.
        let mut pending_submits: VecDeque<String> = VecDeque::new();
        loop {
            let mut should_exit_outer = false;
            // Outer bounded idle poll — single wait for idle redraw; inner drain is
            // ZERO. The wait is the idle interval, EXCEPT while the reader is still
            // owed text: a frame releases one character, so painting on a timer
            // would both drain a leftover backlog slowly and cap the text below the
            // rate the model produces it.
            let idle_poll = crate::tui::paint_interval(
                tui_state.borrow().reveal_pending(),
                crate::tui::TUI_IDLE_POLL,
            );
            let has_event = crossterm::event::poll(idle_poll)
                .map_err(InteractiveError::Io)?;
            if has_event {
                // Drain all already-queued events with ZERO timeout (never waits).
                loop {
                    let event = crossterm::event::read()
                        .map_err(InteractiveError::Io)?;
                    match event {
                        crossterm::event::Event::Key(key) => {
                            if key.kind
                                != crossterm::event::KeyEventKind::Press
                            {
                                // Still check for more queued events via ZERO poll below.
                            } else if key.code
                                == crossterm::event::KeyCode::Char('c')
                                && key.modifiers.contains(
                                    crossterm::event::KeyModifiers::CONTROL,
                                )
                            {
                                should_exit_outer = true;
                                break;
                            } else if tui_state
                                .borrow()
                                .pending_approval
                                .is_some()
                            {
                                let removing_provider = tui_state
                                    .borrow()
                                    .confirming_provider_removal;
                                let outcome =
                                    handle_pending_approval_key_outcome(
                                        &tui_state,
                                        key,
                                        &workspace_root,
                                        &mut sink,
                                    );
                                if outcome.is_some() {
                                    let composed = transient_status(
                                        &tui_state.borrow(),
                                        "",
                                    );
                                    tui_state.borrow_mut().status = composed;
                                }
                                if removing_provider
                                    && outcome.is_some_and(|outcome| {
                                        outcome.decision
                                            == crate::tui::ApprovalDecision::Approve
                                            && outcome.removal_committed
                                            && !worker
                                                .source()
                                                .send(WorkerCommand::Reload)
                                    })
                                {
                                    let _ = sink.write_all(
                                        b"profile reload request failed (details hidden)\n",
                                    );
                                }
                                // A tool-approval verdict goes straight
                                // into the exchange (never a command: the
                                // worker is blocked in the hook and serves
                                // no queue). Provider removal leaves
                                // `pending_tool_approval` clear and never
                                // reaches here.
                                if let Some(outcome) = outcome {
                                    deliver_tool_approval_verdict(
                                        &tui_state,
                                        worker.source().approval_exchange(),
                                        &mut sink,
                                        outcome.decision,
                                    );
                                }
                            } else {
                                let viewport = terminal
                                    .borrow()
                                    .size()
                                    .map_err(|e| {
                                        InteractiveError::Io(io::Error::other(
                                            e.to_string(),
                                        ))
                                    })?
                                    .height
                                    .saturating_sub(3);
                                let submitted = crate::tui::handle_key(
                                    &mut tui_state.borrow_mut(),
                                    key,
                                    viewport,
                                );
                                if submitted {
                                    // S1: the state change is the tested helper;
                                    // the pre-dispatch frame below is what makes
                                    // it visible before the turn runs.
                                    let pending =
                                        crate::tui::accept_submitted_input(
                                            &mut tui_state.borrow_mut(),
                                        );
                                    // The bottom bar no longer says ready or
                                    // working: the indicator above the input owns
                                    // that state.
                                    let base = "";
                                    // One submit per drain: dispatch once, keep
                                    // the last line when several arrive together.
                                    if let Some(prompt) = pending {
                                        if pending_submits.len() >= 8 {
                                            let _ = sink.write_all(
                                            sanitize_for_display(
                                                "input queue full; prompt was not accepted\n",
                                            )
                                            .as_bytes(),
                                        );
                                        } else {
                                            pending_submits.push_back(prompt);
                                        }
                                    } else {
                                        tui_state.borrow_mut().end_turn();
                                    }
                                    let composed = transient_status(
                                        &tui_state.borrow(),
                                        base,
                                    );
                                    tui_state.borrow_mut().status = composed;
                                }
                                refresh_panels_if_requested(
                                    worker.source(),
                                    &mut sink,
                                    &tui_state,
                                    &pane_cache,
                                    &mut progress,
                                    &mut reasoning_sink,
                                );
                            }
                        }
                        crossterm::event::Event::Mouse(mouse) => {
                            let viewport = terminal
                                .borrow()
                                .size()
                                .map_err(|e| {
                                    InteractiveError::Io(io::Error::other(
                                        e.to_string(),
                                    ))
                                })?
                                .height
                                .saturating_sub(3);
                            if handle_tui_mouse(&tui_state, mouse, viewport) {
                                // Changed: the loop-bottom draw is the redraw.
                            }
                        }
                        crossterm::event::Event::Resize(_, _) => {}
                        _ => {}
                    }
                    if should_exit_outer {
                        break;
                    }
                    // Only already-queued events; NEVER waits — immediate draw after drain.
                    if !crossterm::event::poll(crate::tui::TUI_DRAIN_POLL)
                        .map_err(InteractiveError::Io)?
                    {
                        break;
                    }
                }
            }
            if should_exit_outer {
                break;
            }
            // Part B: a delivered tool-approval verdict resumes the
            // suspended turn. The modal is closed (verdict above) but
            // the exchange id is still armed; the resume helper clears
            // it, re-enters the still-running turn, and ends it on
            // completion. A Ctrl+C exit above skips this (the worker
            // shutdown unblocks the hook through the stop flag).
            if tui_state.borrow().pending_tool_approval.is_some()
                && tui_state.borrow().pending_approval.is_none()
            {
                resume_suspended_turn(
                    worker.source(),
                    &mut sink,
                    &tui_state,
                    &pane_cache,
                    &mut progress,
                    &mut reasoning_sink,
                )?;
            }
            // C1/C2: handle completed add-flow form (atomically write profile).
            let completed_opt = {
                let mut guard = tui_state.borrow_mut();
                if let Some(form) = guard.provider_add_form.as_mut() {
                    form.completed.take()
                } else {
                    None
                }
            };
            if let Some(data) = completed_opt {
                let write_result = match load_workspace_profile_write_token(
                &workspace_root,
            ) {
                Some(observed) => write_profile_config_at(
                    &workspace_root,
                    &observed,
                    &data.provider,
                    &data.model,
                    data.credential_env.as_deref(),
                    data.endpoint.as_deref(),
                    Some(data.protocol.as_str()),
                    data.model_display_name.as_deref(),
                ),
                None => Err(
                    "profile snapshot is unavailable; reload before retrying"
                        .to_owned(),
                ),
            };
                match write_result {
                    Ok(()) => {
                        let _ = sink.write_all(
                        sanitize_for_display(
                            "provider saved to siralos.toml - restart the session to apply\n",
                        )
                        .as_bytes(),
                    );
                        tui_state.borrow_mut().provider_add_form = None;
                    }
                    Err(err) => {
                        let msg = format!("provider config failed: {err}\n");
                        let _ = sink
                            .write_all(sanitize_for_display(&msg).as_bytes());
                        tui_state.borrow_mut().provider_add_form = None;
                    }
                }
                let composed = transient_status(&tui_state.borrow(), "");
                tui_state.borrow_mut().status = composed;
            }
            // UI spec form drain (ADR 0039 P4): a completed add/edit
            // form writes `.siralos/ui.toml` under a fresh
            // observation. File-local mutation under a one-shot
            // token: no session state involved, so no worker round
            // trip (unlike profile writes, there is no live state
            // to apply).
            let ui_completed = {
                let mut guard = tui_state.borrow_mut();
                if let Some(form) = guard.ui_spec_form.as_mut() {
                    form.completed
                        .take()
                        .map(|data| (form.editing.clone(), data))
                } else {
                    None
                }
            };
            if let Some((editing, data)) = ui_completed {
                let outcome =
                    drain_ui_spec_form(&workspace_root, editing, &data);
                let _ =
                    sink.write_all(sanitize_for_display(&outcome).as_bytes());
                tui_state.borrow_mut().ui_spec_form = None;
                let composed = transient_status(&tui_state.borrow(), "");
                tui_state.borrow_mut().status = composed;
            }
            // S2: model fetch integration — after ApiKey advance, fetch once (blocking, freeze documented).
            let needs_fetch = {
                let guard = tui_state.borrow();
                guard
                    .provider_add_form
                    .as_ref()
                    .is_some_and(|f| f.fetching_models)
            };
            if needs_fetch {
                // Show fetching status while blocking. The add-flow's fetch is
                // FRONTEND-side on purpose: it uses the values the user is typing
                // into the form, not the session's credential (decision 168 R2 is
                // about the composed credential, which stays in the worker).
                {
                    let fetching = transient_status(
                        &tui_state.borrow(),
                        "fetching models...",
                    );
                    tui_state.borrow_mut().status = fetching;
                }
                // Gather the form's provider, protocol, endpoint, and
                // credential. The adapter routes named providers through their
                // fixed effective route; only generic uses this endpoint.
                let (provider_opt, url_opt, protocol_opt, cred_opt) = {
                    let guard = tui_state.borrow();
                    if let Some(form) = guard.provider_add_form.as_ref() {
                        (
                            form.provider.clone(),
                            form.endpoint.clone(),
                            form.protocol.clone(),
                            form.credential_env.clone(),
                        )
                    } else {
                        (None, None, None, None)
                    }
                };
                let named_provider =
                    provider_opt.as_deref().is_some_and(|provider| {
                        matches!(
                            provider.to_ascii_lowercase().as_str(),
                            "openai" | "anthropic"
                        )
                    });
                let protocol =
                    if provider_opt.as_deref().is_some_and(|provider| {
                        provider.eq_ignore_ascii_case("openai")
                    }) {
                        siralos_core::composition::Protocol::OpenAiCompletions
                    } else if provider_opt.as_deref().is_some_and(|provider| {
                        provider.eq_ignore_ascii_case("anthropic")
                    }) {
                        siralos_core::composition::Protocol::AnthropicMessages
                    } else {
                        protocol_opt
                            .as_deref()
                            .and_then(
                                siralos_core::composition::Protocol::parse,
                            )
                            .unwrap_or_default()
                    };
                let url_opt = if named_provider { None } else { url_opt };
                // Do not transmit a pasted credential during the pre-approval add
                // flow. The profile is not written/digest-approved yet, so an
                // endpoint probe has no consent binding; credentialed users enter
                // the model id or fetch it after `/reload` from the worker-owned
                // session. Public endpoints may still be probed without auth.
                let credential = None;
                let fetch_result = if cred_opt.is_some() {
                    Err("credentialed model listing requires an approved profile; enter the model manually".to_owned())
                } else {
                    siralos_adapters::provider::HostProvider::fetch_models_for_provider(
                    provider_opt.as_deref(),
                    url_opt.as_deref(),
                    credential,
                    protocol,
                )
                };
                {
                    let mut guard = tui_state.borrow_mut();
                    if let Some(form) = guard.provider_add_form.as_mut() {
                        form.apply_fetch_result(fetch_result);
                    }
                }
                // Restore ready status after the fetch.
                {
                    let ready = transient_status(&tui_state.borrow(), "ready");
                    tui_state.borrow_mut().status = ready;
                }
            }
            // S1 (owner QoL 2026-09-12): paint BEFORE the turn runs. The turn is
            // synchronous, so without this frame the input box still shows the
            // submitted text and `working` is never seen until the response
            // arrives -- the "press Enter" and "looks frozen" reports.
            if !pending_submits.is_empty() {
                draw_now();
            }
            if !pending_submits.is_empty()
                && tui_state.borrow().reveal_pending()
            {
                // Do not start the next turn while the previous turn still owns
                // unrevealed stream/reasoning bytes. This keeps the per-turn
                // buffer boundary and thinking anchor truthful.
                continue;
            }
            if let Some(input_line) = pending_submits.pop_front() {
                tui_state.borrow_mut().begin_turn(std::time::Instant::now());
                // I3 & I6/I7: parse once, handle unknown honesty before dispatch
                // through the single shared helper (decision 114 Q3 — both loops
                // call one definition).
                let trimmed = input_line.trim().to_owned();
                let mut command_exit = false;
                let command = parse_slash_command(&trimmed);
                let is_unknown = is_unknown_slash_command(&trimmed);
                if is_unknown {
                    let catalog_names = slash_command_catalog()
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ");
                    let msg = format!(
                        "unknown command - available: {catalog_names}\n"
                    );
                    let sanitized = sanitize_for_display(&msg);
                    let _ = sink.write_all(sanitized.as_bytes());
                } else if let SlashCommand::Provider = command {
                    // C1: /provider with no configured provider OR the "add" entry opens the sequential add-flow form.
                    // The values come from the cached `Ready` snapshot (R3).
                    let (provider, model, endpoint) = {
                        let state = tui_state.borrow();
                        (
                            state.provider.clone(),
                            state.model.clone(),
                            state.endpoint.clone(),
                        )
                    };
                    let entries = crate::tui::provider_entries_from_session(
                        provider.as_deref(),
                        model.as_deref(),
                        endpoint.as_deref(),
                    );
                    let observed =
                        load_workspace_profile_write_token(&workspace_root);
                    let mut state = tui_state.borrow_mut();
                    state.pending_profile_write_token = observed;
                    if entries.is_empty() {
                        crate::tui::open_provider_add_form(&mut state);
                    } else {
                        crate::tui::open_provider_picker(&mut state, entries);
                    }
                } else if let SlashCommand::ProviderRemove = command {
                    // Removal entry point (TUI): absent profile is the truthful
                    // no-op; otherwise arm the y/N confirmation modal (the
                    // decision resolves through the single outcome in the
                    // modal branch below).
                    let (provider, model, endpoint) = {
                        let state = tui_state.borrow();
                        (
                            state.provider.clone(),
                            state.model.clone(),
                            state.endpoint.clone(),
                        )
                    };
                    let entries = crate::tui::provider_entries_from_session(
                        provider.as_deref(),
                        model.as_deref(),
                        endpoint.as_deref(),
                    );
                    if entries.is_empty() {
                        let msg = sanitize_for_display(
                            "no provider configured - nothing to remove\n",
                        );
                        let _ = sink.write_all(msg.as_bytes());
                    } else if let Some(observed) =
                        load_workspace_profile_write_token(&workspace_root)
                    {
                        let mut state = tui_state.borrow_mut();
                        state.pending_profile_write_token = Some(observed);
                        crate::tui::open_provider_remove_confirm(&mut state);
                    } else {
                        let _ = sink.write_all(
                            sanitize_for_display(
                                "provider removal failed (details hidden)\n",
                            )
                            .as_bytes(),
                        );
                    }
                } else if let SlashCommand::Mouse = command {
                    // `/mouse` in-loop interception (beside `ProviderRemove`
                    // above): flip the live `TuiState` and re-pair the terminal
                    // escape in the same arm so state and terminal stay in
                    // lockstep; the sink-only `dispatch_tui_command` arm below
                    // stays unreachable. A failed escape rolls the flip back.
                    let before = tui_state.borrow().mouse_capture;
                    let message = crate::tui::toggle_mouse_capture(
                        &mut tui_state.borrow_mut(),
                    );
                    let enabled = tui_state.borrow().mouse_capture;
                    if let Err(err) = _guard.set_mouse_capture(enabled) {
                        tui_state.borrow_mut().mouse_capture = before;
                        let msg = sanitize_for_display(&format!(
                            "mouse capture unchanged - terminal escape failed: {err}\n"
                        ));
                        let _ = sink.write_all(msg.as_bytes());
                    } else {
                        let msg =
                            sanitize_for_display(&format!("{message}\n"));
                        let _ = sink.write_all(msg.as_bytes());
                    }
                } else if let SlashCommand::Model(None) = command {
                    open_model_picker_via_worker(
                        &mut sink,
                        &tui_state,
                        worker.source(),
                        &pane_cache,
                        &mut progress,
                        &mut reasoning_sink,
                    )?;
                } else {
                    let should_exit = dispatch_tui_command(
                        &command,
                        &workspace_root,
                        &tui_state,
                        &mut sink,
                        worker.source(),
                        &pane_cache,
                        &mut progress,
                        &mut reasoning_sink,
                    )?;
                    // The turn is over: the indicator above the input stops, and
                    // the thinking block stays where this turn anchored it.
                    // A tool-approval suspension is not over (the resume
                    // block ends it): ending it here would stop the
                    // `working` clock and disarm the thinking anchor
                    // mid-turn.
                    command_exit = should_exit;
                }
                if tui_state.borrow().pending_tool_approval.is_none() {
                    tui_state.borrow_mut().end_turn();
                }
                if command_exit {
                    break;
                }
                // Model-switch picker selection: the picker's Enter arms
                // `pending_model_switch`; resolve it through the same
                // switch-and-persist as the explicit-argument form. The header and
                // the displayed model follow from the worker's `Ready` -- the
                // frontend no longer guesses them (decision 168 R3).
                let pending_model =
                    tui_state.borrow_mut().pending_model_switch.take();
                if let Some(selected) = pending_model {
                    switch_model_via_worker(
                        &workspace_root,
                        &mut sink,
                        &tui_state,
                        worker.source(),
                        &pane_cache,
                        &mut progress,
                        &mut reasoning_sink,
                        &selected,
                    )?;
                }
            }
            if exit_requested.get() {
                break;
            }
            // C3: every frame drains the channel first. Anything the worker has
            // already produced -- a pane snapshot, a header the composition moved
            // under, an event nobody is waiting for -- is applied on THIS frame,
            // without waiting for it.
            drain_pending_worker(
                worker.source(),
                &mut sink,
                &tui_state,
                &pane_cache,
            )?;
            if let Some(message) = progress_error.borrow_mut().take() {
                return Err(InteractiveError::Io(io::Error::other(format!(
                    "terminal input failed: {message}"
                ))));
            }
            // A draw failure is reported ONCE (a dead terminal must not spin in
            // silence) and then cleared.
            if let Some(message) = draw_error.borrow_mut().take() {
                return Err(InteractiveError::Io(io::Error::other(format!(
                    "terminal draw failed: {message}"
                ))));
            }
            // One draw at loop bottom — every drained batch or idle tick (P1:
            // immediate after drain). The pane is whatever the worker last pushed
            // (decision 167 D1), so there is nothing to rebuild here.
            draw_now();
        }

        // C2 step 4: stop the worker and WAIT after EVERY fallible UI operation
        // has returned. The recordings' single flush happens inside that join,
        // while `_guard` is still alive and the terminal is still in the alternate
        // screen. Combine a primary UI error with a typed worker failure rather
        // than losing either cause.
        Ok(())
    })();
    let shutdown_result = worker.shutdown_result();
    match (run_result, shutdown_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(reason)) => Err(InteractiveError::Worker(reason)),
        (Err(primary), Err(reason)) => Err(InteractiveError::Worker(format!(
            "{primary}; worker shutdown failed: {reason}"
        ))),
    }
}
