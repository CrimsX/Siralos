//! Session composition: the shared [`SessionComposition`] both
//! frontends drive, its worker-session implementation, and the
//! context audit the `/context` command reports.

use super::{
    ApplicationProjectionConfig, BTreeMap, BashTool, ConfigurationError,
    ContextCapacity, ContextPolicy, ConversationItem,
    DEFAULT_REVIEW_PROVIDER_ID, EditTool, FlushError, FlushOutcome,
    HostCredential, HostProvider, InteractiveError, InteractiveOptions,
    ModuleHost, ModuleManifest, PermissionPolicy, ProjectionService, Rc,
    ReadTool, RecordedReplayProvider, ReplayStoreLoadError,
    RetainingReplayRecorder, SIRALOS_SYSTEM_INSTRUCTIONS, SegmentInput,
    SessionProvider, SiralosApplication, Stability, ToolLoopEvent,
    ToolRegistry, UiFeedInputs, WorkspaceListTool, WorkspaceProfileLoad,
    WorkspaceReadTool, WorkspaceSearchTool, WriteTool, apply_reloaded_config,
    approve_workspace_profile, compose_skills_segment,
    declare_and_compose_profile, effective_profile_protocol,
    load_replay_store, load_sourced_user_specs, load_user_configuration,
    load_workspace_profile_snapshot, non_live_state_digest,
    redacted_credential_display, reload_authority_is_revalidated,
    reload_report_from_load, render_activate_module, render_add_module,
    render_context_segment, render_enable_module, render_tools_segment,
    render_ui_preview_text, resolve_spec_content, resolve_workspace_root,
    safe_alias_for_display, safe_profile_diagnostic, safe_report_text,
    session_host_rules, verify_session_lock, write_replay_store,
};

/// Flush the retaining record-replay recorder at session exit — the SINGLE
/// function both loops call (T4 consolidation; decision 78 B2).
pub(super) fn flush_record_replay(
    record_recorder: Option<Rc<RetainingReplayRecorder>>,
    replay_store_path: &std::path::Path,
) -> Result<FlushOutcome, FlushError> {
    let Some(recorder) = record_recorder else {
        return Ok(FlushOutcome::NoRecorder);
    };
    let snapshot = recorder.replayable_records_snapshot();
    let persistable =
        siralos_adapters::provider::replay::replayable_recording_snapshot(
            &snapshot,
        );
    match write_replay_store(replay_store_path, &persistable) {
        Ok(count) => {
            eprintln!("siralos: replay store persisted: {count}");
            Ok(FlushOutcome::Persisted { recordings: count })
        }
        Err(_err) => {
            eprintln!("siralos: replay store not persisted (details hidden)");
            Err(FlushError::Persistence(
                "replay store write failed (details hidden)".to_owned(),
            ))
        }
    }
}

/// One composed session: every host-owned handle both frontends need.
///
/// T4 (decision 108) permanent residual: `compose_session` stops where the
/// frontends diverge — the stdio loop owns `reader`/`writer` generics and
/// the TUI loop owns the `TerminalGuard`/`Terminal`/`TuiState`/`TuiSink`
/// terminal state. Nothing else is per-frontend: the provider, registry,
/// policy, application, hosts, manifests, profile/context wiring, and the
/// replay-store flush inputs are all composed once here.
///
/// Sharing note (`unsafe_code = "forbid"`, so no `Rc::from_raw` trick):
/// `ToolRegistry` holds `Box<dyn Tool>` and is not `Clone`, and
/// `SiralosApplication::new` borrows the registry — so the application
/// cannot own it and the bundle cannot clone it. The bundle therefore
/// carries the composed registry's immutable OBSERVABLE state — the
/// registration-ordered definitions snapshot (`definitions()` returns
/// freshly cloned owned values) — and the `/tools` render uses the shared
/// [`render_tools_segment`] helper both loops call. The live
/// application keeps borrowing the leaked `&'static` registry (the harness
/// already uses this leak pattern in
/// `harness_cli_session::create_application`); the definitions snapshot is
/// byte-equal to what `registry.definitions()` would return because the
/// registry is immutable after construction.
pub(crate) struct SessionComposition<'a> {
    /// Canonical workspace root.
    pub(super) workspace_root: std::path::PathBuf,
    /// Registration-ordered tool definitions snapshot (same content as
    /// `registry.definitions()` — immutable, so byte-equal forever).
    pub(super) tool_definitions:
        Vec<siralos_core::tool::registry::RegisteredToolInfo>,
    /// Effective permission policy.
    pub(super) policy: PermissionPolicy,
    /// Host application over the session provider.
    pub(super) application: SiralosApplication<'a, SessionProvider>,
    /// The live session provider behind the application borrow. A `/model`
    /// switch updates its interior-mutable model cell in place, so the NEXT
    /// provider request uses the new id without re-composing
    /// provider/endpoint/credential.
    pub(super) live_provider: &'a SessionProvider,
    /// Installed module hosts by module id.
    pub(super) hosts: BTreeMap<String, ModuleHost>,
    /// Loaded module manifests by module id.
    pub(super) manifests: BTreeMap<String, ModuleManifest>,
    /// Applied profile's module selection, if any.
    pub(super) profile_modules: Option<Vec<String>>,
    /// Applied profile's context control, if any.
    pub(super) context_control: Option<ContextPolicy>,
    /// Whether the context subsystem is enabled for this session.
    pub(super) context_system_enabled: bool,
    /// Live context session holder, if the subsystem built.
    pub(super) context_session_holder:
        Option<siralos_adapters::context_session::ContextSystemSession>,
    /// Conversation items already observed by the demand loop.
    pub(super) context_history_len: usize,
    /// Retaining replay recorder for the record-replay flush, if any.
    pub(super) record_recorder: Option<Rc<RetainingReplayRecorder>>,
    /// Replay-store path for load + flush.
    pub(super) replay_store_path: std::path::PathBuf,
    /// Applied provider name from the composed profile (U5/U7).
    pub(super) applied_provider: Option<String>,
    /// Applied model name from the composed profile (U5/U7).
    pub(super) applied_model: Option<String>,
    /// Applied model display name — shown in header/status instead of raw model (S5).
    pub(super) applied_model_display_name: Option<String>,
    /// Applied endpoint from the composed profile (H6 host for picker).
    pub(super) applied_endpoint: Option<String>,
    /// Whether the credential for the applied provider RESOLVED (U7). Crosses
    /// to the frontend as `SessionStatus::credential_resolved` -- the fact, not
    /// the value.
    pub(super) credential_present: bool,
    /// Retained credential for /models fetch (I6) — the live HostCredential
    /// (if any) resolved from the profile's `credential = "env:..."` or `key:...`.
    pub(super) applied_credential:
        Option<siralos_adapters::provider::HostCredential>,
    /// Raw credential string for redacted display (key:*** / env:NAME).
    pub(super) applied_credential_raw: Option<String>,
    /// The trusted user-config path to re-read before `/reload` accepts a
    /// changed profile. The approval is intentionally not cached across a
    /// profile edit: updating this file is the reapproval operation.
    pub(super) profile_approval_path: std::path::PathBuf,
    /// Protocol string the session's provider was built with (snapshot of
    /// `applied_protocol.as_str()` at composition; `/reload` diffs this).
    pub(super) applied_protocol_str: String,
    /// Digest of applied non-route profile state (overlay/modules/context/
    /// skills/replay flags/context-system). Reload compares this before
    /// deciding whether live authority must be revoked.
    pub(super) applied_non_live_digest: Option<String>,
    /// Set when a reload invalidates/absent the trusted profile. Further turns
    /// are refused until a fresh composition is created.
    pub(super) authority_revoked: bool,
    /// Host-owned startup notices for the owning frontend to render once
    /// through the terminal sanitizer (static strings; never model
    /// output, never paths). `pub(crate)` because the headless entry
    /// renders them too; the batch evaluation path has no terminal and
    /// ignores them.
    pub(crate) startup_notices: Vec<String>,
}

/// C2 (ticket 130): the composition IS the worker's session.
///
/// The adapter is thin on purpose -- `SessionComposition` already owns the
/// application, the live provider behind it, the context session and the
/// recorder, which are exactly the things the worker loop needs. Implementing
/// the trait here (rather than on `SiralosApplication`) is what lets `pane()`
/// read the context metrics and `flush()` reach the recordings.
// C2 step 3b: the drain's source seam, implemented by pure delegation. The
// stdio and TUI loops keep calling \`drain_events\` with the composed session,
// so this changes no behaviour -- it is what makes the source swappable.
impl<'a, P> crate::session_worker::EventSource for SiralosApplication<'a, P>
where
    P: siralos_core::provider::ModelProvider,
{
    fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        SiralosApplication::poll_event(self)
    }

    fn cancel(&mut self) {
        SiralosApplication::cancel(self);
    }
}

impl crate::session_worker::EventSource for SessionComposition<'_> {
    fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        self.application.poll_event()
    }

    fn cancel(&mut self) {
        self.application.cancel();
    }
}

impl crate::session_worker::WorkerSession for SessionComposition<'_> {
    fn send_prompt(&mut self, prompt: &str) -> Result<(), String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        self.application
            .send_prompt(prompt.to_owned())
            .map_err(|error| error.to_string())
    }

    fn turn_settled(&mut self) {
        // C2: the demand loop reads the session's OWN history, so it runs with
        // the session -- after the turn's events, exactly where the frontends
        // call it today (`send_prompt` -> drain -> demand).
        if let Some(session) = self.context_session_holder.as_mut() {
            drive_context_demand(
                &mut self.application,
                session,
                &mut self.context_history_len,
            );
        }
    }
    fn poll_event(&mut self) -> Option<siralos_core::tool::ToolLoopEvent> {
        self.application.poll_event()
    }

    fn is_responding(&self) -> bool {
        self.application.is_responding()
    }

    fn pane(&self) -> Option<crate::tui::ContextPaneData> {
        crate::tui::build_context_pane(
            self.context_system_enabled,
            self.context_session_holder.as_ref().map(|s| &s.metrics),
            self.application.history(),
        )
    }

    fn context_report(&self) -> String {
        // The SAME renderer the stdio dispatcher writes: the projection claim,
        // the applied profile's context control, and the audit segment. All
        // three inputs live here (decision 168 R4: a report belongs where the
        // control is applied). The frontend used to call this renderer itself,
        // so the bytes it renders do not move.
        render_context_segment(
            &self.application,
            self.context_control.as_ref(),
            self.context_system_enabled,
            &self.context_session_holder,
        )
    }

    fn tools_report(&self) -> String {
        // Same as stdio: the registration-ordered definitions plus the current
        // projection, both of which live here (the registry is borrowed by the
        // application and is not `Clone`).
        render_tools_segment(
            &self.tool_definitions,
            &self.policy,
            &self.application,
        )
    }

    fn set_model(&mut self, model: &str) -> Result<(), String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        if !siralos_core::composition::is_model_id(model) {
            return Err("model id is invalid".to_owned());
        }
        // D3: the FRONTEND persists the profile first; the worker only applies
        // it live, so persist-before-live stays true without shared state.
        if !self.live_provider.set_live_model(model) {
            return Err("model switch is unavailable for this provider mode; restart required".to_owned());
        }
        // A display name belongs to the model it was declared for: keeping the
        // old one would label the new model with the old model's name.
        if self.applied_model.as_deref() != Some(model) {
            self.applied_model_display_name = None;
        }
        self.applied_model = Some(model.to_owned());
        Ok(())
    }

    fn fetch_models(
        &mut self,
        cancellation: &crate::session_worker::CancelFlag,
    ) -> Result<Vec<String>, String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        // Route the command through the effective provider instance. Named
        // providers must not inherit a workspace endpoint or credential;
        // Generic uses its own live cells. The cancel flag is polled by the
        // bounded model-list probe so TUI interruption does not wait for HTTP.
        self.live_provider.fetch_models_cancellable(cancellation.flag())
    }

    fn modules_add(&mut self, folder: &str) -> Result<String, String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        // The registry and the hosts live here, so the mutation does too
        // (decision 168 R4). The render helpers are the SAME functions the
        // dispatcher called, so the reports and side effects do not fork.
        Ok(render_add_module(
            &self.workspace_root,
            folder,
            &mut self.hosts,
            &mut self.manifests,
        ))
    }

    fn modules_enable(&mut self, id: &str) -> Result<String, String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        Ok(render_enable_module(
            &self.workspace_root,
            &mut self.hosts,
            &mut self.manifests,
            id,
            self.profile_modules.as_deref(),
        ))
    }

    fn modules_activate(&mut self, id: &str) -> Result<String, String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        Ok(render_activate_module(
            &self.workspace_root,
            &mut self.hosts,
            &mut self.manifests,
            id,
            self.profile_modules.as_deref(),
        ))
    }

    fn ui_preview(&self, id: &str) -> String {
        // Same render path as the stdio dispatcher: the session
        // holds the registry snapshot, policy, and application, so
        // feed content resolves here (decision 168 R4).
        let state_dir = siralos_adapters::paths::state_dir().ok();
        render_ui_preview_text(
            &self.workspace_root,
            &self.tool_definitions,
            &self.policy,
            self.application.last_projection(),
            state_dir.as_deref(),
            id,
        )
    }

    fn panels_report(&self) -> crate::tui::UiPanels {
        use siralos_core::ui::{UiRegistry, content_for, project_components};
        // User specs only; module-declared specs join in P5.
        // Unreadable documents render as the empty host here; the
        // typed refusal surfaces through `/ui-list`.
        let sourced =
            load_sourced_user_specs(&self.workspace_root).unwrap_or_default();
        let mut registry = UiRegistry::new();
        for item in &sourced {
            if !item.spec.enabled() {
                continue;
            }
            if registry.insert(item.spec.clone()).is_err() {
                continue;
            }
        }
        let state_dir = siralos_adapters::paths::state_dir().ok();
        let mut contents = std::collections::BTreeMap::new();
        for spec in registry.specs() {
            let inputs = UiFeedInputs {
                root: &self.workspace_root,
                tools: &self.tool_definitions,
                policy: &self.policy,
                last_projection: self.application.last_projection(),
                checkpoint_state_dir: state_dir.as_deref(),
            };
            if let Ok(lines) = resolve_spec_content(&inputs, spec) {
                contents.insert(spec.id().to_owned(), lines);
            }
        }
        let projection = project_components(&registry, |spec| {
            content_for(spec.id(), &contents)
        });
        crate::tui::UiPanels { projection, selected: 0 }.clamped()
    }

    fn status(&self) -> crate::session_worker::SessionStatus {
        if self.authority_revoked {
            return crate::session_worker::SessionStatus {
                status: "authority revoked; restart required".to_owned(),
                provider: None,
                model: None,
                endpoint: None,
                protocol: "revoked".to_owned(),
                credential_display: None,
                credential_resolved: false,
                live_model_switchable: false,
                context_suffix: String::new(),
                startup_notices: Vec::new(),
            };
        }
        // The same recipe the TUI entry used before the session moved here: the
        // display name wins when the profile declares one, and the context
        // metrics feed the usage segment.
        let redact_value = |value: &str| {
            let bounded = safe_report_text(value, 256);
            self.applied_credential
                .as_ref()
                .map(|credential| credential.redact_text(&bounded))
                .unwrap_or(bounded)
        };
        let model = self
            .applied_model_display_name
            .clone()
            .filter(|name| !name.is_empty())
            .or_else(|| self.applied_model.clone())
            .map(|value| safe_alias_for_display(&redact_value(&value)));
        let provider = self
            .applied_provider
            .as_deref()
            .map(|value| safe_alias_for_display(&redact_value(value)));
        let endpoint = self.live_provider.effective_endpoint().as_deref().map(
            |endpoint| {
                let projected =
                    siralos_adapters::provider::safe_endpoint_for_credential(
                        endpoint,
                        self.applied_credential.as_ref(),
                    );
                let lower = projected.to_ascii_lowercase();
                if lower.contains("sk-")
                    || lower.contains("key:")
                    || lower.contains("token")
                    || lower.contains("secret")
                {
                    "[ENDPOINT REDACTED]".to_owned()
                } else {
                    projected
                }
            },
        );
        let protocol =
            self.live_provider.effective_protocol().as_str().to_owned();
        crate::session_worker::SessionStatus {
            status: crate::tui::compose_status_line_with_context(
                "",
                provider.as_deref(),
                model.as_deref(),
                self.context_session_holder.as_ref().map(|s| &s.metrics),
            ),
            provider,
            model,
            endpoint,
            protocol,
            credential_display: self
                .applied_credential_raw
                .as_deref()
                .map(|raw| redacted_credential_display(Some(raw))),
            // The RESOLUTION, not the value: the frontend's `/models` arm
            // decides on exactly this today.
            credential_resolved: self.credential_present,
            // The frontend persists BEFORE the worker applies, so it needs to
            // know here whether a switch is possible at all.
            live_model_switchable: self.live_provider.supports_live_model(),
            // The suffix alone, so a transient status keeps the readout.
            context_suffix: crate::tui::append_context_usage(
                String::new(),
                self.context_session_holder.as_ref().map(|s| &s.metrics),
            ),
            // Host-owned composition notices; the owning frontend
            // renders them once through the terminal sanitizer.
            // Re-sent on every status (reload re-announces a still-true
            // notice); only the startup path renders them.
            startup_notices: self.startup_notices.clone(),
        }
    }

    fn reload(&mut self) -> Result<String, String> {
        if self.authority_revoked {
            return Err(
                "session authority was revoked by profile reload; restart required"
                    .to_owned(),
            );
        }
        // C2: the reload path (re-read, recompose, apply) now runs HERE, with
        // the session it mutates -- the same `reload_report` +
        // `apply_reloaded_config` pair both frontends call, so there is still
        // exactly one definition of what a reload does. The report is RETURNED
        // rather than printed: the frontend shows what happened, and the loop
        // cannot announce a reload that did not.
        let live_provider = self.live_provider;
        let live_model = live_provider.live_model();
        let fresh_config = match load_user_configuration(Some(
            &self.profile_approval_path,
        )) {
            Ok(config) => config,
            Err(_error) => {
                self.invalidate_live_authority();
                return Err(
                    "reload not applied: trusted user configuration could not be reloaded (details hidden)"
                        .to_owned(),
                );
            }
        };
        let fresh_snapshot =
            load_workspace_profile_snapshot(&self.workspace_root);
        let fresh_approved = approve_workspace_profile(
            &fresh_snapshot,
            fresh_config.config.profile_approval.as_deref(),
        );
        let reload_authorized = reload_authority_is_revalidated(
            &self.workspace_root,
            &self.profile_approval_path,
            &fresh_snapshot,
            &fresh_approved,
            &fresh_config,
        );
        let fresh_non_live = non_live_state_digest(&fresh_approved);
        let (mut report, recomposed) = reload_report_from_load(
            &fresh_approved,
            self.applied_provider.as_deref(),
            live_model.as_deref().or(self.applied_model.as_deref()),
            self.applied_credential_raw.as_deref(),
            self.applied_endpoint.as_deref(),
            self.applied_protocol_str.as_str(),
        );
        // Recompose/report work above is pure, but authority can drift while
        // it runs. Revalidate the exact snapshot and trusted config again at
        // the last gate before any live-cell mutation.
        let reload_authorized = reload_authorized
            && reload_authority_is_revalidated(
                &self.workspace_root,
                &self.profile_approval_path,
                &fresh_snapshot,
                &fresh_approved,
                &fresh_config,
            );
        if !reload_authorized {
            report.push_str(
                "reload not applied: profile authority could not be revalidated\n",
            );
        }
        let non_live_changed = reload_authorized
            && ((fresh_non_live.is_none()
                && self.applied_non_live_digest.is_some())
                || fresh_non_live.as_ref().is_some_and(|digest| {
                    Some(digest) != self.applied_non_live_digest.as_ref()
                }));
        let provider_would_change = reload_authorized
            && recomposed.as_ref().and_then(|value| value.provider.as_deref())
                != self.applied_provider.as_deref();
        let route_applied = reload_authorized
            && apply_reloaded_config(
                live_provider,
                self.applied_provider.as_deref(),
                live_model.as_deref(),
                &mut self.applied_model,
                &mut self.applied_model_display_name,
                &mut self.applied_endpoint,
                &mut self.applied_protocol_str,
                &mut self.applied_credential,
                &mut self.applied_credential_raw,
                recomposed,
                &mut report,
            );
        let revoke_authority = !reload_authorized
            || !route_applied
            || provider_would_change
            || non_live_changed;
        if non_live_changed {
            report.push_str(
                "restart required: non-live profile state changed\n",
            );
        }
        if reload_authorized {
            if let Some(digest) = fresh_non_live {
                self.applied_non_live_digest = Some(digest);
            }
        }
        if revoke_authority {
            self.invalidate_live_authority();
            report.push_str(
                "profile reload invalidated live authority; further turns are refused until restart\n",
            );
        }
        self.credential_present = self.applied_credential.is_some();
        Ok(report)
    }

    fn cancel(&mut self) {
        self.application.cancel();
    }

    fn enable_progress_ticks(&mut self) {
        self.application.enable_provider_progress_ticks();
    }

    fn flush(&mut self) {
        // Legacy compatibility wrapper. The worker uses `flush_result` so a
        // persistence failure cannot be reported as a successful stop.
        let _ = self.flush_result();
    }

    fn flush_result(&mut self) -> Result<FlushOutcome, FlushError> {
        // Exactly once, by the single owner (decision 78). `take` is what
        // makes that mechanical: a second flush finds nothing to flush.
        let recorder = self.record_recorder.take();
        flush_record_replay(recorder, &self.replay_store_path)
    }
}

impl SessionComposition<'_> {
    fn invalidate_live_authority(&mut self) {
        self.authority_revoked = true;
        self.credential_present = false;
        self.applied_credential = None;
        self.applied_credential_raw = None;
        self.applied_endpoint = None;
        self.applied_model = None;
        self.applied_model_display_name = None;
        let _ = self.live_provider.set_live_credential(None);
        let _ = self.live_provider.set_live_endpoint(None);
    }

    /// Flush the optional retaining recorder exactly once. Frontends that own
    /// a worker call its flush; synchronous callers use this at every return
    /// path so early failures cannot drop record-replay evidence.
    pub(crate) fn flush_replay(&mut self) -> Result<FlushOutcome, FlushError> {
        let recorder = self.record_recorder.take();
        flush_record_replay(recorder, &self.replay_store_path)
    }

    /// Install the synchronous approval resolver (worker thread setup
    /// only: stdio installs its own prompter around its owned reader
    /// and writer). Fails when a response is already active, exactly
    /// like projection setup.
    pub(crate) fn set_approval_resolver(
        &mut self,
        resolver: siralos_core::tool::SharedApprovalResolver,
    ) -> Result<(), siralos_core::tool::PromptStartError> {
        self.application.set_approval_resolver(resolver)
    }

    /// Ticket 135: provider-reported usage totalled over this session's
    /// recordings.
    ///
    /// `None` means the session recorded nothing at all -- there is no
    /// recorder, so this was not a `record-replay` run. `Some` with `None`
    /// fields means the recorder exists and the provider reported no usage:
    /// absent stays absent, never a fabricated zero (decision 102's rule).
    /// Read this BEFORE `flush()`, which hands the recorder away.
    pub(crate) fn usage_totals(
        &self,
    ) -> Option<siralos_core::evaluation::UsageTotals> {
        let recorder = self.record_recorder.as_ref()?;
        let recordings = recorder.records_snapshot();
        let mut totals = siralos_core::evaluation::UsageTotals::default();
        for recording in &recordings {
            accumulate_usage(
                &mut totals.input_tokens,
                recording.identity.input_tokens,
            );
            accumulate_usage(
                &mut totals.output_tokens,
                recording.identity.output_tokens,
            );
            accumulate_usage(
                &mut totals.cached_tokens,
                recording.identity.cached_tokens,
            );
        }
        Some(totals)
    }
}

/// Add one reported value to a running total, leaving it absent when nothing
/// reported one.
fn accumulate_usage(total: &mut Option<u64>, value: Option<u64>) {
    if let Some(value) = value {
        *total = Some(total.unwrap_or(0).saturating_add(value));
    }
}

/// Compose one session — the SINGLE definition both loops call (T4).
///
/// This is the verbatim T1 composition block both loops duplicated:
/// configuration gate, workspace root, workspace tools, host rules,
/// profile declare/compose, replay wiring, provider choice, module/context
/// selection, context-system build, registry, lock verification, skills
/// segment, projection config, application, hosts/manifests. The ONLY
/// per-frontend residual is the terminal I/O each loop owns after this
/// returns (stdio: `reader`/`writer`; TUI: guard/terminal/state/sink).
pub(crate) fn compose_session(
    options: InteractiveOptions<'_>,
) -> Result<SessionComposition<'static>, InteractiveError> {
    // --- The verbatim T1 composition both loops duplicated (one copy now).
    let composed = load_user_configuration(options.config_path)?;
    if composed.review_provider_id != DEFAULT_REVIEW_PROVIDER_ID {
        return Err(InteractiveError::Configuration(
            ConfigurationError::UnknownReviewProvider {
                provider_id: composed.review_provider_id,
            },
        ));
    }
    let workspace_root = match options.workspace_root {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(InteractiveError::CurrentDirectory)?,
    };
    let workspace_root = resolve_workspace_root(&workspace_root)?;
    let mut tools: Vec<Box<dyn siralos_core::tool::Tool>> = vec![
        Box::new(WorkspaceListTool::new(&workspace_root)?),
        Box::new(WorkspaceReadTool::new(&workspace_root)?),
        Box::new(WorkspaceSearchTool::new(&workspace_root)?),
        Box::new(ReadTool::new(&workspace_root)?),
    ];
    // ADR 0038: the default mutation tools join the registry with the
    // checkpoint store bound to this workspace. A store that cannot be
    // opened degrades truthfully to the read-only surface: the notice
    // joins the bundle for the owning frontend to render through the
    // terminal sanitizer (never a silent partial registration, never a
    // direct stderr write from composition).
    let mut startup_notices = Vec::new();
    match WriteTool::default_store(&workspace_root) {
        Ok(store) => {
            tools.push(Box::new(WriteTool::new(
                &workspace_root,
                store.clone(),
            )?));
            tools.push(Box::new(EditTool::new(&workspace_root, store)?));
        }
        Err(_error) => {
            startup_notices.push(
                "siralos: checkpoint store unavailable: mutation tools disabled (details hidden)"
                    .to_owned(),
            );
        }
    }
    // ADR 0038: the `bash` default tool joins the registry unconditionally
    // (it needs no store). It reports typed `sandbox_unavailable`
    // wherever no live probe proves enforcement — registration makes
    // the refusal reachable, never execution.
    tools.push(Box::new(BashTool::new(&workspace_root)?));
    // R7.4 profiles select the built-in fail-closed posture; they never
    // grant a Tool. Read-only workspace inspection is allowed and its
    // decision is still checked per call; workspace mutation defaults
    // to ask (per-action approval through the host ledger, ADR 0038)
    // and profiles can only narrow it toward deny.
    // Stage 5.2 (decision 48): the workspace profile narrows these Host
    // rules - composition can never produce a rule broader than the
    // Host's own, and a refused or invalid profile is simply not applied
    // with a truthful diagnostic (C3). `/reload` recomposes through the
    // same [`declare_and_compose_profile`] below — one composition path.
    let host_rules = session_host_rules();
    let profile_snapshot = load_workspace_profile_snapshot(&workspace_root);
    let loaded_profile = approve_workspace_profile(
        &profile_snapshot,
        composed.config.profile_approval.as_deref(),
    );
    let effective = declare_and_compose_profile(&loaded_profile, &host_rules);
    if let Some(diagnostic) = &effective.diagnostic {
        // Host-side startup diagnostic (never model output): the declared
        // profile was not applied; the session proceeds on pure Host
        // policy.
        eprintln!(
            "siralos: profile not applied: {}",
            safe_profile_diagnostic(diagnostic)
        );
    }
    // Stage 8 B2: additive [profile] record-replay / replay wiring
    let replay_store_path =
        workspace_root.join(".siralos").join("replay-store.json");
    let (want_record_replay, want_replay) = match &loaded_profile {
        WorkspaceProfileLoad::Record(record)
            if effective.applied_profile.is_some() =>
        {
            (record.record_replay, record.replay)
        }
        _ => (false, false),
    };
    let (provider_name_owned, model_opt, credential_opt, endpoint_opt) =
        match &loaded_profile {
            WorkspaceProfileLoad::Record(record)
                if effective.applied_profile.is_some() =>
            {
                let cred = match record.credential.as_deref() {
                    Some(c) => Some(
                        HostCredential::from_credential_str(c).map_err(|e| {
                            InteractiveError::Provider(format!(
                                "declared credential could not be resolved: {e}"
                            ))
                        })?,
                    ),
                    None => None,
                };
                (
                    record
                        .provider
                        .as_deref()
                        .unwrap_or("deterministic-fake")
                        .to_owned(),
                    record.model.clone(),
                    cred,
                    record.endpoint.clone(),
                )
            }
            _ => ("deterministic-fake".to_owned(), None, None, None),
        };
    // One effective snapshot owns one resolution. The provider constructor
    // and status projection below reuse this value rather than resolving an
    // env reference a second time.
    let resolved_profile_credential = credential_opt.clone();
    let applied_protocol: siralos_core::composition::Protocol =
        effective_profile_protocol(
            (provider_name_owned == "anthropic").then_some("anthropic"),
            match &loaded_profile {
                WorkspaceProfileLoad::Record(record)
                    if effective.applied_profile.is_some() =>
                {
                    record.protocol
                }
                _ => siralos_core::composition::Protocol::default(),
            },
        );
    // I5/U7 + H6: applied provider/model/credential/endpoint for status + display + picker + /models (I6).
    // S5: model display name prefers over raw model id for header/status.
    let (
        applied_provider,
        applied_model,
        applied_model_display_name,
        applied_endpoint,
        credential_present,
        applied_credential,
        applied_credential_raw,
    ) = match &loaded_profile {
        WorkspaceProfileLoad::Record(record)
            if effective.applied_profile.is_some() =>
        {
            let resolved_credential = resolved_profile_credential.clone();
            let cred_present = resolved_credential.is_some();
            let cred = resolved_credential;
            (
                record.provider.clone(),
                record.model.clone(),
                record.model_display_name.clone(),
                record.endpoint.clone(),
                cred_present,
                cred,
                record.credential.clone(),
            )
        }
        _ => (None, None, None, None, false, None, None),
    };
    let mut live_host_provider: Option<HostProvider> = None;
    let mut replay_provider_holder: Option<RecordedReplayProvider> = None;
    let mut record_recorder: Option<Rc<RetainingReplayRecorder>> = None;
    if want_replay {
        let pid = provider_name_owned.clone();
        let model =
            model_opt.clone().unwrap_or_else(|| "generic-model".to_owned());
        match load_replay_store(&replay_store_path) {
            Ok(store) => {
                let digest = siralos_core::determinism::replay_store::compute_replay_store_digest(&store.recordings);
                eprintln!(
                    "siralos: replay store loaded: digest {digest} count {}",
                    store.recordings.len()
                );
                let store_recordings = store.recordings;
                let had_store_recordings = !store_recordings.is_empty();
                let (route_recordings, legacy_recordings): (Vec<_>, Vec<_>) =
                    store_recordings
                        .into_iter()
                        .filter(|recording| {
                            recording.identity.provider_id == pid
                        })
                        .partition(|recording| {
                            recording.request_sha256.is_some()
                        });
                if route_recordings.is_empty()
                    && legacy_recordings.is_empty()
                    && had_store_recordings
                {
                    return Err(InteractiveError::Provider(
                        "replay store has no recordings for the active provider"
                            .to_owned(),
                    ));
                }
                replay_provider_holder = Some(
                    if route_recordings.is_empty() {
                        // An all-legacy cache has no route binding to verify; use
                        // the explicit legacy constructor rather than weakening
                        // the strict route-bound contract.
                        RecordedReplayProvider::new(
                            pid,
                            model,
                            legacy_recordings,
                        )
                    } else {
                        // Mixed caches use the route-bound subset. Legacy entries
                        // remain available through the legacy constructor but are
                        // never replayed under an unrelated active route.
                        // The named OpenAI/Anthropic adapters ignore the
                        // profile endpoint and always bind their fixed wire
                        // route. Replay must use that same effective route;
                        // otherwise a valid live recording is rejected merely
                        // because the profile selected a cosmetic endpoint or
                        // protocol. Generic providers, in contrast, use the
                        // configured endpoint and canonical applied protocol.
                        let (replay_endpoint, replay_protocol) =
                            match provider_name_owned.as_str() {
                                "openai" => (
                                    "https://api.openai.com/v1".to_owned(),
                                    "openai-completions".to_owned(),
                                ),
                                "anthropic" => (
                                    "https://api.anthropic.com/v1".to_owned(),
                                    "anthropic-messages".to_owned(),
                                ),
                                "deterministic-fake" => (
                                    "https://deterministic-fake.invalid/v1"
                                        .to_owned(),
                                    applied_protocol.as_str().to_owned(),
                                ),
                                _ => {
                                    let endpoint = endpoint_opt.clone().ok_or_else(
                                        || {
                                            InteractiveError::Provider(
                                                "replay route requires an endpoint"
                                                    .to_owned(),
                                            )
                                        },
                                    )?;
                                    (
                                        endpoint,
                                        applied_protocol.as_str().to_owned(),
                                    )
                                }
                            };
                        RecordedReplayProvider::new_with_route(
                            pid,
                            model,
                            replay_endpoint,
                            replay_protocol,
                            route_recordings,
                        )
                    },
                );
            }
            Err(ReplayStoreLoadError::NotFound) => {
                return Err(InteractiveError::Provider(
                    "replay store requested but not found".to_owned(),
                ));
            }
            Err(err) => {
                let msg = match &err {
                    ReplayStoreLoadError::UntrustedDigest => {
                        "replay store untrusted: digest mismatch".to_owned()
                    }
                    ReplayStoreLoadError::Malformed(r) => {
                        format!("replay store malformed: {r}")
                    }
                    ReplayStoreLoadError::Bounds(e) => {
                        format!("replay store bounds: {e}")
                    }
                    ReplayStoreLoadError::Io(m) => {
                        format!("replay store I/O: {m}")
                    }
                    ReplayStoreLoadError::NotFound => unreachable!(),
                };
                return Err(InteractiveError::Provider(msg));
            }
        }
    } else if want_record_replay {
        let raw = match HostProvider::from_provider_str_with_protocol(
            &provider_name_owned,
            model_opt.clone(),
            credential_opt,
            endpoint_opt.clone(),
            applied_protocol,
        ) {
            Ok(p) => p,
            Err(err) => {
                return Err(InteractiveError::Provider(format!(
                    "provider composition refused: {err}"
                )));
            }
        };
        let recorder = Rc::new(RetainingReplayRecorder::new());
        let clock: Rc<dyn siralos_core::determinism::Clock> =
            Rc::new(siralos_core::determinism::SystemClock);
        let with = raw.with_replay_support(clock, recorder.clone());
        record_recorder = Some(recorder);
        live_host_provider = Some(with);
    } else {
        let raw = match HostProvider::from_provider_str_with_protocol(
            &provider_name_owned,
            model_opt,
            credential_opt,
            endpoint_opt,
            applied_protocol,
        ) {
            Ok(p) => p,
            Err(err) => {
                return Err(InteractiveError::Provider(format!(
                    "provider composition refused: {err}"
                )));
            }
        };
        live_host_provider = Some(raw);
    }
    // Stage 5.7 (decision 53): the applied profile's module selection
    // narrows /modules-activate. Only an actually-applied profile
    // contributes a selection; invalid or refused profiles contribute
    // none (5.2 semantics), and the Host can never be broadened.
    let profile_modules: Option<Vec<String>> =
        if effective.applied_profile.is_some() {
            match &loaded_profile {
                WorkspaceProfileLoad::Record(record) => record.modules.clone(),
                _ => None,
            }
        } else {
            None
        };
    // Stage 5.8 (decision 54): the applied profile's context control
    // narrows what the session claims about content. Only an
    // actually-applied profile contributes a control; invalid or refused
    // profiles contribute none (5.2 semantics), and without one the
    // session is transparent (Live).
    let context_control: Option<ContextPolicy> =
        if effective.applied_profile.is_some() {
            match &loaded_profile {
                WorkspaceProfileLoad::Record(record) => record.context.clone(),
                _ => None,
            }
        } else {
            None
        };
    // Activation B3b (decision 99): the session wires the read-only context
    // subsystem behind the additive `[profile.context_system]` opt-in. Only
    // an actually-applied profile contributes the opt-in (absent key or
    // enabled=false is byte-transparent), and a widening interpretation is
    // impossible by construction: the key only registers read-only context
    // tools over an immutable snapshot and updates an in-memory working set.
    // A build failure is a host-side diagnostic and the subsystem stays off
    // (never fatal, never partial — no tools are registered if the build
    // failed). Nothing persists; no rendering changes (B4 owns the audit
    // surface).
    let context_system_enabled: bool = if effective.applied_profile.is_some() {
        match &loaded_profile {
            WorkspaceProfileLoad::Record(record) => {
                record.context_system_enabled
            }
            _ => false,
        }
    } else {
        false
    };
    let context_build =
        siralos_adapters::context_session::build_context_system(
            &workspace_root,
            context_system_enabled,
        );
    if let Some(diagnostic) = &context_build.diagnostic {
        eprintln!("siralos: {diagnostic}");
    }
    let context_session_holder: Option<
        siralos_adapters::context_session::ContextSystemSession,
    > = context_build.session.clone();
    // The number of conversation items already observed by the demand loop.
    // The demand edge only derives NEW, host-observed context-tool results
    // since the last tick, so repeated prompts never double-tick a node.
    let context_history_len: usize = 0;
    if let Some(session) = &context_session_holder {
        tools.extend(session.register_tools());
    }
    let registry = ToolRegistry::new(tools)?;
    // Stage 5.9 (decision 55): verify the on-disk `siralos.lock` against
    // the recomputed current lock. The lock never gates authority: the
    // session always proceeds on live Host state and reports drift or
    // untrusted content truthfully as a host-side startup diagnostic.
    let lock_decision = verify_session_lock(&workspace_root, &effective);
    if let Some(reason) = &lock_decision.reason {
        eprintln!("siralos: lock not trusted: {reason}");
    }
    // Stage 5.10 (decision 56): the applied profile's opt-in skill
    // selection resolves against the workspace skill catalog. Guidance only —
    // the consumption can never add capability, Tool, or permission —
    // and absent selection/catalog stays byte-transparent (R7.5).
    let skills_segment = compose_skills_segment(
        &workspace_root,
        &loaded_profile,
        &effective,
        resolved_profile_credential.as_ref(),
    );
    let mut segments = vec![SegmentInput {
        id: "siralos-core-instructions".to_owned(),
        stability: Stability::Stable,
        title: "Siralos instructions".to_owned(),
        content: SIRALOS_SYSTEM_INSTRUCTIONS.to_owned(),
    }];
    if let Some(segment) = skills_segment {
        segments.push(segment);
    }
    let policy = PermissionPolicy::from_rules(effective.rules.clone());
    let projection_config = ApplicationProjectionConfig {
        capacity: Some(ContextCapacity::default()),
        segments,
        ..ApplicationProjectionConfig::default()
    };
    // Choose the provider for this session based on B2 flags. The provider
    // and registry are borrowed by the application; both are leaked to
    // `'static` (the harness already uses this pattern in
    // `harness_cli_session::create_application`). Registries are immutable
    // after construction, so the definitions snapshot below is byte-equal
    // to `registry.definitions()` forever.
    let session_provider: &'static SessionProvider =
        Box::leak(Box::new(if let Some(rp) = replay_provider_holder {
            SessionProvider::Replay(rp)
        } else if let Some(hp) = live_host_provider {
            SessionProvider::Host(hp)
        } else {
            unreachable!("session provider must be present");
        }));
    let registry_static: &'static ToolRegistry = Box::leak(Box::new(registry));
    let tool_definitions: Vec<
        siralos_core::tool::registry::RegisteredToolInfo,
    > = registry_static.definitions();
    let application = SiralosApplication::new(
        session_provider,
        registry_static,
        policy.clone(),
        None,
        // Owner bug 2026-09-12: the frozen reference default is 8 rounds,
        // which a real multi-step task exhausts (read, search, read again).
        // The Session Budget takes the hard cap the same frozen rules
        // allow; the reference default stays untouched for parity.
        Some(f64::from(siralos_core::tool::budget::MAX_TOOL_ROUNDS)),
    )
    .with_projection(ProjectionService::new(), projection_config);
    Ok(SessionComposition {
        workspace_root,
        tool_definitions,
        policy,
        application,
        live_provider: session_provider,
        hosts: BTreeMap::new(),
        manifests: BTreeMap::new(),
        profile_modules,
        context_control,
        context_system_enabled,
        context_session_holder,
        context_history_len,
        record_recorder,
        replay_store_path,
        applied_provider,
        applied_model,
        applied_model_display_name,
        applied_endpoint,
        credential_present,
        applied_credential,
        applied_credential_raw,
        profile_approval_path: composed.path.clone(),
        applied_protocol_str: applied_protocol.as_str().to_owned(),
        applied_non_live_digest: non_live_state_digest(&loaded_profile),
        authority_revoked: false,
        startup_notices,
    })
}

/// T3 shared audit/pane gate — the SAME function both frontends call.
///
/// The decision 100 `/context` audit segment renders only when the applied
/// profile has `[profile.context_system].enabled = true` AND the subsystem
/// built successfully; the T3 context pane uses the identical condition.
/// OFF (`!enabled` or no built session) yields `None`: byte-transparent, no
/// placeholder. This consolidates the TUI `/context` arm's former
/// holder-only check with the stdio arm's flag+holder check (T3
/// consolidation where the pane work touches).
pub fn context_audit_session(
    enabled: bool,
    holder: &Option<siralos_adapters::context_session::ContextSystemSession>,
) -> Option<&siralos_adapters::context_session::ContextSystemSession> {
    if enabled { holder.as_ref() } else { None }
}

/// Activation B3b (decision 99): drive the demand loop from host-observed
/// context-tool results after a completed prompt.
///
/// The only observations that reach the scheduler are paired
/// `AssistantToolCall` -> `ToolResult` entries from the authoritative
/// Host-owned conversation history for the three read-only context tools
/// (`context.search` / `context.inspect` / `context.expand`). The model can
/// never inject events or scores: the demand helpers (`compose_tick_input`
/// and friends) accept only host-observed `ToolObservation`s, and this is
/// the only surface feeding the session working set. New history items since
/// the last tick are processed exactly once; a prompt with no context-tool
/// results yields an empty observation set whose tick coalesces naturally
/// (the decision 89 guard), so no-op rounds change nothing.
pub(super) fn drive_context_demand<P>(
    application: &mut SiralosApplication<'_, P>,
    session: &mut siralos_adapters::context_session::ContextSystemSession,
    context_history_len: &mut usize,
) where
    P: siralos_core::provider::ModelProvider,
{
    let history = application.history();
    let start = (*context_history_len).min(history.len());
    let new_items: &[ConversationItem] = &history[start..];
    let mut observations: Vec<
        siralos_adapters::tool::context_events::ToolObservation,
    > = Vec::new();
    // Pair assistant tool calls with their results within the new window.
    let mut pending_inputs = std::collections::BTreeMap::new();
    for item in new_items {
        match item {
            ConversationItem::AssistantToolCall {
                call_id,
                tool_name,
                input,
            } => {
                if input.value().is_some()
                    && matches!(
                        tool_name.as_str(),
                        "context.search"
                            | "context.inspect"
                            | "context.expand"
                    )
                {
                    pending_inputs.insert(
                        call_id.clone(),
                        input.value().expect("value present").clone(),
                    );
                }
            }
            ConversationItem::ToolResult { call_id, tool_name, result } => {
                if !matches!(
                    tool_name.as_str(),
                    "context.search" | "context.inspect" | "context.expand"
                ) {
                    continue;
                }
                if let Some(input) = pending_inputs.remove(call_id) {
                    observations.push(
                        siralos_adapters::tool::context_events::ToolObservation::new(
                            tool_name.clone(),
                            input,
                            result.clone(),
                        ),
                    );
                }
            }
            _ => {}
        }
    }
    *context_history_len = history.len();
    let _ = session.drive_tick(&observations);
}
