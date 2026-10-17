//! The blocking stdio frontend loop and its slash-command dispatch.

use super::{
    BTreeMap, BufRead, ContextPolicy, HostCredential, InteractiveError,
    InteractiveOptions, ModelListingGate, ModuleHost, ModuleManifest, Path,
    PermissionPolicy, Rc, RefCell, ReloadAuthorityEvidence,
    SessionComposition, SessionProvider, SiralosApplication, SlashCommand,
    StdioApprovalPrompter, WorkspaceProfileLoad, Write, apply_model_switch,
    apply_provider_remove_confirmation_at, apply_reloaded_config,
    approve_workspace_profile, compose_session, drain_events,
    drive_context_demand, execute_ui_remove, flush_record_replay, io,
    is_unknown_slash_command, load_user_configuration,
    load_workspace_profile_snapshot, model_listing_diagnostic,
    model_listing_gate, non_live_state_digest, parse_slash_command,
    read_approval_via_input_queue, reload_authority_is_revalidated,
    reload_report_from_load, render_activate_module, render_add_module,
    render_context_segment, render_enable_module, render_evolve_lines,
    render_model_line, render_modules, render_provider_line,
    render_startup_notices, render_tools_segment, render_ui_list_text,
    render_ui_preview_text, safe_alias_for_display, sanitize_for_display,
    slash_command_catalog,
};
/// Run the default interactive session over process stdin/stdout.
pub fn run_interactive_stdio() -> Result<(), InteractiveError> {
    // Owned buffered handles (not held locks): the approval hook
    // shares these same objects, so reads and writes stay ordered on
    // the one queue in every configuration.
    run_interactive_session(io::BufReader::new(io::stdin()), io::stdout())
}

/// Run a synchronous interactive session with the default composition.
pub fn run_interactive_session<R, W>(
    reader: R,
    writer: W,
) -> Result<(), InteractiveError>
where
    R: BufRead + 'static,
    W: Write + 'static,
{
    run_interactive_session_with_options(
        reader,
        writer,
        InteractiveOptions::default(),
    )
}

/// Maximum bytes accepted for one stdio input line.
const MAX_STDIO_INPUT_BYTES: usize = 64 * 1024;

struct BoundedInputLine {
    text: String,
    overlong: bool,
}

fn read_bounded_input_line<R: BufRead>(
    reader: &mut R,
) -> Result<Option<BoundedInputLine>, InteractiveError> {
    let mut bytes = Vec::new();
    let mut overlong = false;
    let mut saw_input = false;
    loop {
        let available = reader.fill_buf().map_err(InteractiveError::Io)?;
        if available.is_empty() {
            break;
        }
        saw_input = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if !overlong {
            let remaining = MAX_STDIO_INPUT_BYTES.saturating_sub(bytes.len());
            if take <= remaining {
                bytes.extend_from_slice(&available[..take]);
            } else {
                bytes.extend_from_slice(&available[..remaining]);
                overlong = true;
            }
        }
        reader.consume(take);
        if newline.is_some() {
            break;
        }
    }
    if !saw_input {
        return Ok(None);
    }
    if overlong {
        return Ok(Some(BoundedInputLine {
            text: String::new(),
            overlong: true,
        }));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        InteractiveError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "input line is not valid UTF-8",
        ))
    })?;
    Ok(Some(BoundedInputLine { text, overlong }))
}

///
/// T4 (decision 108): the session-composition block is the single shared
/// [`compose_session`] helper both the stdio loop and the TUI loop call —
/// one definition, two call sites. The residual below it is honest and
/// permanent: `compose_session` stops where the frontends diverge (the
/// stdio loop owns `reader`/`writer` generics; the TUI loop owns the
/// `TerminalGuard`/`Terminal`/`TuiState`/`TuiSink` terminal state), so the
/// return bundle carries everything both loops need and each loop wires
/// only its own frontend.
pub fn run_interactive_session_with_options<R, W>(
    reader: R,
    writer: W,
    options: InteractiveOptions<'_>,
) -> Result<(), InteractiveError>
where
    R: BufRead + 'static,
    W: Write + 'static,
{
    let mut session = compose_session(options)?;
    // The approval hook runs inside the turn drain while the loop is
    // blocked in it, so the loop's reader and writer are shared
    // (never doubly borrowed: every use is one brief borrow, and the
    // hook never runs while the loop holds one).
    let reader: Rc<RefCell<R>> = Rc::new(RefCell::new(reader));
    let writer: Rc<RefCell<W>> = Rc::new(RefCell::new(writer));
    session
        .application
        .set_approval_resolver(StdioApprovalPrompter::resolver(
            Rc::clone(&reader),
            Rc::clone(&writer),
        ))
        .map_err(|error| {
            InteractiveError::Io(io::Error::other(error.to_string()))
        })?;
    let SessionComposition {
        workspace_root,
        tool_definitions,
        policy,
        mut application,
        live_provider,
        mut hosts,
        mut manifests,
        profile_modules,
        context_control,
        context_system_enabled,
        mut context_session_holder,
        mut context_history_len,
        record_recorder,
        replay_store_path,
        applied_provider,
        mut applied_model,
        mut applied_model_display_name,
        mut applied_endpoint,
        credential_present: _,
        mut applied_credential,
        mut applied_credential_raw,
        profile_approval_path,
        mut applied_protocol_str,
        mut applied_non_live_digest,
        mut authority_revoked,
        startup_notices,
    } = session;

    // Host-owned startup notices cross the terminal sanitizer to the
    // owned writer before the first prompt (composition owns no
    // writer, so it returns them instead of printing them).
    writer
        .borrow_mut()
        .write_all(render_startup_notices(&startup_notices).as_bytes())
        .map_err(InteractiveError::Io)?;

    // --- Frontend residual (stdio): prompt loop over reader/writer. ---
    // All session state above comes from the shared helper; only the
    // terminal I/O below is per-frontend.
    // Keep the loop in a fallible closure so a reader/writer/dispatch error
    // cannot bypass the single replay-store flush below.
    let loop_result = (|| -> Result<(), InteractiveError> {
        loop {
            writer
                .borrow_mut()
                .write_all(b"> ")
                .map_err(InteractiveError::Io)?;
            writer.borrow_mut().flush().map_err(InteractiveError::Io)?;
            let Some(bounded) =
                read_bounded_input_line(&mut *reader.borrow_mut())?
            else {
                break;
            };
            if bounded.overlong {
                writer.borrow_mut()
                .write_all(
                    b"input line exceeded the 64 KiB bound; line discarded\n",
                )
                .map_err(InteractiveError::Io)?;
                continue;
            }
            let input = bounded.text.trim_end_matches(['\r', '\n']);
            if input.trim().is_empty() {
                continue;
            }
            // T4: one shared parse, one thin stdio writer (the TUI loop calls
            // the same parser with its sink writer). Q3 (decision 114): the
            // stdio loop gains the same unknown-command honesty gate the TUI
            // has, through the single shared helper — unknown slash commands
            // render the explicit honesty line instead of falling through to
            // the prompt path.
            let trimmed = input.trim();
            if is_unknown_slash_command(trimmed) {
                let catalog_names = slash_command_catalog()
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ");
                let msg =
                    format!("unknown command - available: {catalog_names}\n");
                let sanitized = sanitize_for_display(&msg);
                writer
                    .borrow_mut()
                    .write_all(sanitized.as_bytes())
                    .map_err(InteractiveError::Io)?;
                continue;
            }
            let command = parse_slash_command(trimmed);
            if dispatch_stdio_command(
                &command,
                &workspace_root,
                &tool_definitions,
                &policy,
                &mut application,
                &writer,
                &reader,
                &mut hosts,
                &mut manifests,
                profile_modules.as_deref(),
                context_control.as_ref(),
                context_system_enabled,
                &mut context_session_holder,
                &mut context_history_len,
                applied_provider.as_deref(),
                live_provider,
                &mut applied_model,
                &mut applied_model_display_name,
                &mut applied_credential_raw,
                &mut applied_endpoint,
                &mut applied_credential,
                &profile_approval_path,
                &mut applied_protocol_str,
                &mut authority_revoked,
                &mut applied_non_live_digest,
            )? {
                break;
            }
        }
        Ok(())
    })();
    // Decision 78 B2: the shared record-replay flush both loops call. It is
    // attempted even when the frontend closure failed.
    let flush_result =
        flush_record_replay(record_recorder, &replay_store_path);
    match (loop_result, flush_result) {
        (Err(primary), Err(flush)) => {
            Err(InteractiveError::ReplayFlush(format!("{primary}; {flush}")))
        }
        (Err(primary), Ok(_)) => Err(primary),
        (Ok(()), Err(flush)) => {
            Err(InteractiveError::ReplayFlush(flush.to_string()))
        }
        (Ok(()), Ok(_)) => Ok(()),
    }
}

/// Plan one undo restore, prompt for confirmation on the input
/// queue, and execute on approval: the stdio shape of the
/// confirmation gate (mirrors the provider-removal prompt).
fn run_stdio_undo_for_id<W, R>(
    writer: &Rc<RefCell<W>>,
    reader: &Rc<RefCell<R>>,
    workspace_root: &Path,
    id: &str,
) -> Result<(), InteractiveError>
where
    W: Write,
    R: BufRead,
{
    let store = match crate::interactive::undo::open_undo_store(workspace_root)
    {
        Ok(store) => store,
        Err(error) => {
            let rendered =
                sanitize_for_display(&format!("{}\n", error.message));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
            return Ok(());
        }
    };
    let plan = match crate::interactive::undo::plan_undo_restore(&store, id) {
        Ok(plan) => plan,
        Err(error) => {
            let rendered =
                sanitize_for_display(&format!("{}\n", error.message));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
            return Ok(());
        }
    };
    let prompt = sanitize_for_display(
        &crate::interactive::undo::render_undo_prompt(&plan),
    );
    writer
        .borrow_mut()
        .write_all(prompt.as_bytes())
        .map_err(InteractiveError::Io)?;
    writer.borrow_mut().flush().map_err(InteractiveError::Io)?;
    let decision = read_approval_via_input_queue(&mut *reader.borrow_mut())?;
    let confirmation = if decision == crate::tui::ApprovalDecision::Approve {
        crate::interactive::undo::UndoConfirmation::Confirmed
    } else {
        crate::interactive::undo::UndoConfirmation::Unconfirmed
    };
    let text = crate::interactive::undo::execute_undo_restore(
        &store,
        &plan.id,
        confirmation,
    );
    let rendered = sanitize_for_display(&text);
    writer
        .borrow_mut()
        .write_all(rendered.as_bytes())
        .map_err(InteractiveError::Io)?;
    Ok(())
}

/// Dispatch one parsed [`SlashCommand`] to the stdio writer — one of the
/// two thin per-frontend writers over the shared parse + render helpers.
/// Returns `true` when the loop must exit (`/exit`).
///
/// T4 consolidation: the stdio loop calls this; the TUI loop calls
/// [`dispatch_tui_command`]. Both take the same parsed command and call
/// the same render helpers — no parallel match arms.
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_stdio_command<P, W, R>(
    command: &SlashCommand<'_>,
    workspace_root: &Path,
    tool_definitions: &[siralos_core::tool::registry::RegisteredToolInfo],
    policy: &PermissionPolicy,
    application: &mut SiralosApplication<'_, P>,
    writer: &Rc<RefCell<W>>,
    reader: &Rc<RefCell<R>>,
    hosts: &mut BTreeMap<String, ModuleHost>,
    manifests: &mut BTreeMap<String, ModuleManifest>,
    profile_modules: Option<&[String]>,
    context_control: Option<&ContextPolicy>,
    context_system_enabled: bool,
    context_session_holder: &mut Option<
        siralos_adapters::context_session::ContextSystemSession,
    >,
    context_history_len: &mut usize,
    provider: Option<&str>,
    live_provider: &SessionProvider,
    applied_model: &mut Option<String>,
    applied_model_display_name: &mut Option<String>,
    applied_credential_raw: &mut Option<String>,
    applied_endpoint: &mut Option<String>,
    applied_credential: &mut Option<HostCredential>,
    profile_approval_path: &Path,
    applied_protocol_str: &mut String,
    authority_revoked: &mut bool,
    applied_non_live_digest: &mut Option<String>,
) -> Result<bool, InteractiveError>
where
    P: siralos_core::provider::ModelProvider,
    W: Write,
    R: BufRead,
{
    match command {
        SlashCommand::Context => {
            let sanitized = render_context_segment(
                application,
                context_control,
                context_system_enabled,
                context_session_holder,
            );
            writer
                .borrow_mut()
                .write_all(sanitized.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Tools => {
            let rendered =
                render_tools_segment(tool_definitions, policy, application);
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Modules => {
            let rendered =
                sanitize_for_display(&render_modules(workspace_root));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Exit => return Ok(true),
        SlashCommand::ModulesAdd(folder) => {
            let rendered = sanitize_for_display(&render_add_module(
                workspace_root,
                folder.unwrap_or(""),
                hosts,
                manifests,
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::ModulesEnable(id) => {
            let rendered = sanitize_for_display(&render_enable_module(
                workspace_root,
                hosts,
                manifests,
                id.unwrap_or(""),
                profile_modules,
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::ModulesActivate(id) => {
            let rendered = sanitize_for_display(&render_activate_module(
                workspace_root,
                hosts,
                manifests,
                id.unwrap_or(""),
                profile_modules,
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::UiList => {
            let rendered =
                sanitize_for_display(&render_ui_list_text(workspace_root));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::UiAdd => {
            let rendered = sanitize_for_display(
                "ui-add opens a form in the TUI; run the TUI and press F2 to manage panels\n",
            );
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::UiEdit(_) => {
            let rendered = sanitize_for_display(
                "ui-edit opens a form in the TUI; run the TUI and press F2 to manage panels\n",
            );
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::UiRemove(id) => {
            let rendered = sanitize_for_display(&execute_ui_remove(
                workspace_root,
                id.unwrap_or(""),
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Undo(argument) => match argument {
            None => {
                let text = match crate::interactive::undo::open_undo_store(
                    workspace_root,
                ) {
                    Ok(store) => {
                        crate::interactive::undo::render_undo_list(&store)
                    }
                    Err(error) => format!("{}\n", error.message),
                };
                let rendered = sanitize_for_display(&text);
                writer
                    .borrow_mut()
                    .write_all(rendered.as_bytes())
                    .map_err(InteractiveError::Io)?;
            }
            Some(id) => {
                run_stdio_undo_for_id(writer, reader, workspace_root, id)?;
            }
        },
        SlashCommand::UndoLast => {
            let store = match crate::interactive::undo::open_undo_store(
                workspace_root,
            ) {
                Ok(store) => store,
                Err(error) => {
                    let rendered =
                        sanitize_for_display(&format!("{}\n", error.message));
                    writer
                        .borrow_mut()
                        .write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                    return Ok(false);
                }
            };
            let id = match crate::interactive::undo::resolve_last_restorable(
                &store,
            ) {
                Ok(id) => id,
                Err(error) => {
                    let rendered =
                        sanitize_for_display(&format!("{}\n", error.message));
                    writer
                        .borrow_mut()
                        .write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                    return Ok(false);
                }
            };
            run_stdio_undo_for_id(writer, reader, workspace_root, &id)?;
        }
        SlashCommand::UiPreview(id) => {
            let state_dir = siralos_adapters::paths::state_dir().ok();
            let modules = crate::interactive::ui::load_feed_module_records(
                workspace_root,
            );
            let inputs = crate::interactive::ui::UiFeedInputs {
                root: workspace_root,
                tools: tool_definitions,
                policy,
                modules: &modules,
                last_projection: application.last_projection(),
                checkpoint_state_dir: state_dir.as_deref(),
            };
            let rendered = sanitize_for_display(&render_ui_preview_text(
                &inputs,
                id.unwrap_or(""),
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Provider => {
            let rendered = sanitize_for_display(&render_provider_line(
                provider,
                applied_credential_raw.as_deref(),
                applied_credential.as_ref(),
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::ProviderRemove => {
            // Removal entry point (stdio): absent profile is the truthful
            // no-op without prompting; otherwise retain the exact revision
            // observed before the input-queue approval and bind removal to it.
            let profile_snapshot =
                load_workspace_profile_snapshot(workspace_root);
            let observed = profile_snapshot.write_token();
            match profile_snapshot.load() {
                WorkspaceProfileLoad::Absent => {
                    let rendered = sanitize_for_display(
                        "no provider configured - nothing to remove\n",
                    );
                    writer
                        .borrow_mut()
                        .write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                }
                _ => {
                    let Some(observed) = observed else {
                        let rendered = sanitize_for_display(
                            "provider removal failed (details hidden)\n",
                        );
                        writer
                            .borrow_mut()
                            .write_all(rendered.as_bytes())
                            .map_err(InteractiveError::Io)?;
                        return Ok(false);
                    };
                    let prompt = sanitize_for_display(
                        "remove the configured provider from siralos.toml? (y/N)\n",
                    );
                    writer
                        .borrow_mut()
                        .write_all(prompt.as_bytes())
                        .map_err(InteractiveError::Io)?;
                    writer
                        .borrow_mut()
                        .flush()
                        .map_err(InteractiveError::Io)?;
                    let decision = read_approval_via_input_queue(
                        &mut *reader.borrow_mut(),
                    )?;
                    let removal_report = apply_provider_remove_confirmation_at(
                        workspace_root,
                        &observed,
                        decision,
                    );
                    if removal_report.starts_with("provider removed") {
                        *authority_revoked = true;
                        *applied_credential = None;
                        *applied_credential_raw = None;
                        *applied_endpoint = None;
                        *applied_model = None;
                        *applied_model_display_name = None;
                        let _ = live_provider.set_live_credential(None);
                        let _ = live_provider.set_live_endpoint(None);
                    }
                    let rendered = sanitize_for_display(&removal_report);
                    writer
                        .borrow_mut()
                        .write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                }
            }
        }
        SlashCommand::Model(argument) => {
            match argument {
                None => {
                    // Bare `/model` in stdio: keep the display behaviour
                    // (U7) and say how to switch — a picker is not
                    // possible here, so never silently do nothing.
                    let mut out = render_model_line(
                        applied_model.as_deref(),
                        applied_credential.as_ref(),
                    );
                    out.push_str(
                        "pass /model <id> to switch, or use the TUI picker\n",
                    );
                    let rendered = sanitize_for_display(&out);
                    writer
                        .borrow_mut()
                        .write_all(rendered.as_bytes())
                        .map_err(InteractiveError::Io)?;
                }
                Some(id) => {
                    if *authority_revoked {
                        writer.borrow_mut()
                            .write_all(
                                b"session authority was revoked; restart required\n",
                            )
                            .map_err(InteractiveError::Io)?;
                        return Ok(false);
                    }
                    // Explicit switch: validate + persist, then update
                    // the live provider cell and the display holders.
                    match apply_model_switch(
                        workspace_root,
                        live_provider,
                        provider,
                        applied_model,
                        applied_model_display_name,
                        id,
                    ) {
                        Ok(message) => {
                            let rendered = sanitize_for_display(&message);
                            writer
                                .borrow_mut()
                                .write_all(rendered.as_bytes())
                                .map_err(InteractiveError::Io)?;
                        }
                        Err(reason) => {
                            let rendered = sanitize_for_display(&reason);
                            writer
                                .borrow_mut()
                                .write_all(rendered.as_bytes())
                                .map_err(InteractiveError::Io)?;
                        }
                    }
                }
            }
        }
        SlashCommand::Models => {
            // I6 blocking fetch — route through the effective provider so
            // named providers never use a workspace endpoint/credential. The
            // gate is shared with the TUI: absent credentials are valid for a
            // public Generic endpoint, while a declared-but-unresolved
            // credential must not silently fall back to an unauthenticated
            // request.
            let gate = model_listing_gate(
                provider,
                live_provider.effective_endpoint().as_deref(),
                applied_credential_raw.is_some(),
                applied_credential.is_some(),
            );
            if gate != ModelListingGate::Ready {
                let line = model_listing_diagnostic(gate);
                writer
                    .borrow_mut()
                    .write_all(sanitize_for_display(line).as_bytes())
                    .map_err(InteractiveError::Io)?;
            } else {
                match live_provider.fetch_models() {
                    Ok(models) => {
                        if models.is_empty() {
                            let line = "no models returned\n";
                            writer
                                .borrow_mut()
                                .write_all(
                                    sanitize_for_display(line).as_bytes(),
                                )
                                .map_err(InteractiveError::Io)?;
                        } else {
                            for id in models {
                                let line = format!(
                                    "{}\n",
                                    safe_alias_for_display(&id)
                                );
                                let sanitized = sanitize_for_display(&line);
                                writer
                                    .borrow_mut()
                                    .write_all(sanitized.as_bytes())
                                    .map_err(InteractiveError::Io)?;
                            }
                        }
                    }
                    Err(_err) => {
                        let line = "models fetch error (details hidden)\n";
                        let sanitized = sanitize_for_display(line);
                        writer
                            .borrow_mut()
                            .write_all(sanitized.as_bytes())
                            .map_err(InteractiveError::Io)?;
                    }
                }
            }
        }
        SlashCommand::Reload => {
            // SAFE HALF: re-read + recompose + REPORT. Pure — no live
            // mutation (neither the display holders nor the provider
            // cell are touched; a later chunk does the swap). Stdio
            // must work without a TTY (plain writer, no picker). The
            // live snapshot is what startup composed: applied provider
            // + LIVE model (honours `/model` switches) + raw
            // credential + endpoint + the protocol the session's
            // provider was built with.
            let live_model = live_provider.live_model();
            // Re-read the trusted user configuration on every reload. A
            // changed dangerous profile is accepted only after its new digest
            // is explicitly present in that trusted file; the startup digest
            // is never silently reused for edited bytes.
            if *authority_revoked {
                writer
                    .borrow_mut()
                    .write_all(
                        b"session authority was revoked; restart required\n",
                    )
                    .map_err(InteractiveError::Io)?;
                return Ok(false);
            }
            let (
                mut report,
                recomposed_config,
                fresh_non_live,
                reload_authorized,
                authority_evidence,
            ) =
                match load_user_configuration(Some(profile_approval_path)) {
                    Ok(fresh_config) => {
                        let fresh_snapshot =
                            load_workspace_profile_snapshot(workspace_root);
                        let fresh_approved = approve_workspace_profile(
                            &fresh_snapshot,
                            fresh_config.config.profile_approval.as_deref(),
                        );
                        let authorized = reload_authority_is_revalidated(
                            workspace_root,
                            profile_approval_path,
                            &fresh_snapshot,
                            &fresh_approved,
                            &fresh_config,
                        );
                        let (report, config) = reload_report_from_load(
                            &fresh_approved,
                            provider,
                            live_model.as_deref().or(applied_model.as_deref()),
                            applied_credential_raw.as_deref(),
                            applied_endpoint.as_deref(),
                            applied_protocol_str.as_str(),
                        );
                        (
                            report,
                            if authorized { config } else { None },
                            non_live_state_digest(&fresh_approved),
                            authorized,
                            Some(ReloadAuthorityEvidence {
                                snapshot: fresh_snapshot,
                                approved: fresh_approved,
                                config: fresh_config,
                            }),
                        )
                    }
                    Err(_error) => (
                        "reload not applied: trusted user configuration could not be reloaded (details hidden)\n"
                            .to_owned(),
                        None,
                        None,
                        false,
                        None,
                    ),
                };
            let reload_authorized = reload_authorized
                && authority_evidence.as_ref().is_some_and(|evidence| {
                    reload_authority_is_revalidated(
                        workspace_root,
                        profile_approval_path,
                        &evidence.snapshot,
                        &evidence.approved,
                        &evidence.config,
                    )
                });
            if !reload_authorized {
                report.push_str(
                    "reload not applied: profile authority could not be revalidated\n",
                );
            }
            let route_applied = reload_authorized
                && apply_reloaded_config(
                    live_provider,
                    provider,
                    live_model.as_deref(),
                    applied_model,
                    applied_model_display_name,
                    applied_endpoint,
                    applied_protocol_str,
                    applied_credential,
                    applied_credential_raw,
                    recomposed_config,
                    &mut report,
                );
            let non_live_changed = reload_authorized
                && (fresh_non_live.is_none()
                    && applied_non_live_digest.is_some()
                    || fresh_non_live.as_ref().is_some_and(|digest| {
                        Some(digest) != applied_non_live_digest.as_ref()
                    }));
            let provider_would_change = reload_authorized
                && fresh_non_live.is_some()
                && report.contains("provider changed");
            if !reload_authorized
                || !route_applied
                || non_live_changed
                || provider_would_change
            {
                *authority_revoked = true;
                *applied_credential = None;
                *applied_credential_raw = None;
                *applied_endpoint = None;
                *applied_model = None;
                *applied_model_display_name = None;
                let _ = live_provider.set_live_credential(None);
                let _ = live_provider.set_live_endpoint(None);
                report.push_str(
                    "profile reload invalidated live authority; further turns are refused until restart\n",
                );
            } else if let Some(digest) = fresh_non_live {
                *applied_non_live_digest = Some(digest);
            }
            let rendered = sanitize_for_display(&report);
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Evolve => {
            let rendered = sanitize_for_display(&render_evolve_lines());
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Mouse => {
            // Stdio has no TTY mouse: report truthfully instead of
            // pretending to toggle. The TUI arm (in-loop, needs the
            // `TuiState` + `TerminalGuard`) is unreachable here.
            let rendered = sanitize_for_display(&format!(
                "{}\n",
                crate::tui::MOUSE_STDIO_MESSAGE
            ));
            writer
                .borrow_mut()
                .write_all(rendered.as_bytes())
                .map_err(InteractiveError::Io)?;
        }
        SlashCommand::Prompt(prompt) => {
            if *authority_revoked {
                writer
                    .borrow_mut()
                    .write_all(
                        b"session authority was revoked; restart required\n",
                    )
                    .map_err(InteractiveError::Io)?;
                return Ok(false);
            }
            application.send_prompt((*prompt).to_owned()).map_err(
                |error| {
                    InteractiveError::Io(io::Error::other(error.to_string()))
                },
            )?;
            drain_events(application, writer, &mut || false, &mut |_| {})?;
            if let Some(session) = context_session_holder {
                drive_context_demand(
                    application,
                    session,
                    context_history_len,
                );
            }
        }
    }
    Ok(false)
}
