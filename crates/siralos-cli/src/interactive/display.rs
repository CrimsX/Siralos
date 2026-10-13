//! Transcript and diagnostic formatting: provider/model lines,
//! redaction helpers, segment rendering, and transient status text.

use super::{
    ContextPolicy, EffectiveRunPolicy, HostCredential, InteractiveError, Path,
    PermissionPolicy, SegmentInput, SiralosApplication, SkillCatalogLoad,
    SkillCatalogState, Stability, TerminalSanitizer, TuiState, WorkerEvent,
    WorkspaceProfileLoad, compose_skill_consumption, context_audit_session,
    decide_context_control, format_context_audit, format_context_status,
    format_tool_projection, format_tools, load_workspace_skills,
    sanitize_for_display,
};

/// Render host-owned startup notices for the owned writer.
///
/// Every notice crosses the terminal sanitizer even though production
/// notices are static host strings: the single output boundary is
/// structural, not content-dependent. Each notice renders on its own
/// line.
pub(crate) fn render_startup_notices(notices: &[String]) -> String {
    let mut out = String::new();
    for notice in notices {
        out.push_str(&sanitize_for_display(notice));
        out.push('\n');
    }
    out
}

/// Host-generated provider line from the composed profile (U7) — redacted (G1).
pub(super) fn render_provider_line(
    provider: Option<&str>,
    credential_raw: Option<&str>,
    resolved_credential: Option<&siralos_adapters::provider::HostCredential>,
) -> String {
    let provider = provider.map(|value| {
        siralos_adapters::provider::redact_host_display(
            value,
            resolved_credential,
        )
    });
    render_provider_line_display(
        provider.as_deref(),
        redacted_credential_display(credential_raw),
    )
}

/// The same line from the ALREADY-REDACTED display form (C2 step 3).
///
/// The TUI renders it from the worker's status snapshot, where the raw
/// credential never arrives (decision 168 R2), so the line has to be
/// composable from the redacted form alone — one definition of the wording,
/// two sources of the value.
pub(super) fn render_provider_line_display(
    provider: Option<&str>,
    credential: String,
) -> String {
    let name = provider
        .map(safe_alias_for_display)
        .unwrap_or_else(|| "no provider configured".to_owned());
    format!("provider: {name}\ncredential: {credential}\n")
}

/// Redacted credential display for status surfaces (key:*** / env:NAME / absent).
pub(super) fn redacted_credential_display(raw: Option<&str>) -> String {
    match raw {
        None => "absent".to_owned(),
        Some(s) if s.starts_with("key:") => "key:***".to_owned(),
        Some(s) if s.starts_with("env:") => safe_alias_for_display(s),
        Some(s) => safe_alias_for_display(&format!("env:{s}")),
    }
}

fn safe_report_identifier(value: &str) -> String {
    let projected: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || character == '_'
                || character == '-'
            {
                character
            } else {
                '_'
            }
        })
        .collect();
    let bounded = siralos_core::language::truncate_utf8_bytes(&projected, 96);
    if bounded.is_empty() { "[empty]".to_owned() } else { bounded }
}

pub(super) fn safe_report_text(value: &str, maximum: usize) -> String {
    let sanitized = sanitize_for_display(value);
    let single_line: String = sanitized
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => ' ',
            other if other.is_control() => ' ',
            other => other,
        })
        .collect();
    siralos_core::language::truncate_utf8_bytes(&single_line, maximum)
}

/// Project a profile diagnostic without reflecting a route or a literal
/// credential. Parser diagnostics are normally already generic; this second
/// projection protects the report boundary from a future adapter diagnostic
/// that accidentally includes source text.
pub(super) fn safe_profile_diagnostic(value: &str) -> String {
    let projected = safe_report_text(value, 512);
    let lower = projected.to_ascii_lowercase();
    let env_name_diagnostic = lower.starts_with("credential (env var ")
        && lower.ends_with(" is not set)");
    // A credential that failed to RESOLVE is its own failure class, and the
    // single most common startup problem: calling it an invalid profile sends
    // the user to fix a document that is perfectly valid. The name of the
    // missing variable is source text and stays hidden.
    if lower.contains("credential could not be resolved") {
        return "declared credential could not be resolved (details hidden)"
            .to_owned();
    }
    if lower.contains("://")
        || lower.contains("key:")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("password")
        || lower.contains("bearer ")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("endpoint=")
        || lower.contains("endpoint:")
        || (lower.contains("credential") && !env_name_diagnostic)
    {
        "profile document is invalid (details hidden)".to_owned()
    } else {
        projected
    }
}

/// Host-generated model line from the composed profile (U7).
pub(super) fn render_model_line(
    model: Option<&str>,
    resolved_credential: Option<&siralos_adapters::provider::HostCredential>,
) -> String {
    let name = model
        .map(|value| {
            siralos_adapters::provider::redact_host_display(
                value,
                resolved_credential,
            )
        })
        .map(|value| safe_alias_for_display(&value))
        .unwrap_or_else(|| "no model configured".to_owned());
    format!("model: {name}\n")
}

/// Project an untrusted provider/model label into a bounded, report-safe
/// alias without exposing credential-shaped values or terminal controls.
pub(crate) fn safe_alias_for_display(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains("key:")
        || lower.contains("sk-")
        || lower.contains("akia")
        || lower.contains("secret")
        || lower.contains("token")
        || value.chars().any(char::is_control)
    {
        return "[REDACTED]".to_owned();
    }
    siralos_core::language::truncate_utf8_bytes(value, 256).to_owned()
}

/// Why a model-listing request is not safe to send. The distinction between a
/// missing credential and a declared-but-unresolved credential is part of the
/// user-facing contract: the former may be a public endpoint, while the latter
/// is a failed environment lookup and must not silently fall back to an
/// unauthenticated request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelListingGate {
    Ready,
    NoProvider,
    NoEndpoint,
    MissingCredential,
    UnresolvedCredential,
    UnsupportedProvider,
}

pub(super) fn model_listing_gate(
    provider: Option<&str>,
    endpoint: Option<&str>,
    credential_declared: bool,
    credential_resolved: bool,
) -> ModelListingGate {
    let name = provider.unwrap_or_default();
    if name.is_empty() {
        return ModelListingGate::NoProvider;
    }
    if name == "deterministic-fake" {
        return ModelListingGate::UnsupportedProvider;
    }
    // Report a failed declared lookup before endpoint absence: the former is
    // actionable and prevents an accidental unauthenticated fallback even
    // when the route is not usable for a second reason.
    if credential_declared && !credential_resolved {
        return ModelListingGate::UnresolvedCredential;
    }
    if endpoint.is_none() {
        return ModelListingGate::NoEndpoint;
    }
    if matches!(name, "openai" | "anthropic") && !credential_declared {
        return ModelListingGate::MissingCredential;
    }
    ModelListingGate::Ready
}

pub(super) fn model_listing_diagnostic(
    gate: ModelListingGate,
) -> &'static str {
    match gate {
        ModelListingGate::Ready => "",
        ModelListingGate::NoProvider => {
            "no provider configured — set [profile] provider/endpoint in siralos.toml\n"
        }
        ModelListingGate::NoEndpoint => {
            "model listing unavailable: the effective provider has no remote endpoint\n"
        }
        ModelListingGate::MissingCredential => {
            "provider credential is not configured — set [profile] credential = \"env:...\" in siralos.toml\n"
        }
        ModelListingGate::UnresolvedCredential => {
            "provider credential is unresolved — set the referenced environment variable and reload\n"
        }
        ModelListingGate::UnsupportedProvider => {
            "model listing unavailable: deterministic-fake has no remote model catalog\n"
        }
    }
}

/// Redacted credential comparison token: `key:` literals collapse to
/// `key:***` so secret bytes never reach the report; `env:` names and
/// absence compare verbatim.
pub(super) fn redacted_credential_token(raw: Option<&str>) -> String {
    match raw {
        None => "absent".to_owned(),
        Some(s) if s.starts_with("key:") => "key:***".to_owned(),
        Some(s) if s.starts_with("env:") => s.to_owned(),
        Some(s) => format!("env:{s}"),
    }
}

pub(super) fn display_field_redacted(
    value: Option<&str>,
    credential_raw: Option<&str>,
) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return "absent".to_owned();
    };
    if let Some(raw) = credential_raw {
        if let Ok(credential) = HostCredential::from_credential_str(raw) {
            return siralos_adapters::provider::redact_host_display(
                value,
                Some(&credential),
            );
        }
    }
    safe_alias_for_display(value)
}

fn display_endpoint(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .map(siralos_adapters::provider::safe_endpoint_for_output)
        .unwrap_or_else(|| "absent".to_owned())
}

pub(super) fn display_endpoint_redacted(
    value: Option<&str>,
    credential_raw: Option<&str>,
) -> String {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return "absent".to_owned();
    };
    if let Some(raw) = credential_raw {
        if let Ok(credential) = HostCredential::from_credential_str(raw) {
            return siralos_adapters::provider::safe_endpoint_for_credential(
                value,
                Some(&credential),
            );
        }
    }
    display_endpoint(Some(value))
}

/// Render the `/context` claim with the shared audit gate — the SINGLE
/// function both loops call (T4 consolidation).
///
/// Returns the sanitized combined segment (base claim + trailing audit when
/// the gate passes), ready to write to either sink.
pub(super) fn render_context_segment<P>(
    application: &SiralosApplication<'_, P>,
    context_control: Option<&ContextPolicy>,
    context_system_enabled: bool,
    context_session_holder: &Option<
        siralos_adapters::context_session::ContextSystemSession,
    >,
) -> String
where
    P: siralos_core::provider::ModelProvider,
{
    let base = render_context_claim(
        &format_context_status(application.last_projection()),
        context_control,
    );
    // T3: the shared audit gate (same condition the pane uses).
    let audit = match context_audit_session(
        context_system_enabled,
        context_session_holder,
    ) {
        Some(session) => format_context_audit(Some(session)),
        None => String::new(),
    };
    let combined =
        if audit.is_empty() { base } else { format!("{base}{audit}") };
    // R4: the audit segment flows through the existing terminal
    // sanitizer path like every other rendered line (single output
    // boundary — no raw bypass). Host vocab passes unchanged, so
    // OFF remains byte-transparent.
    sanitize_for_display(&combined)
}

/// Render the `/tools` segment — the SINGLE function both loops call
/// (T4 consolidation). Returns the raw (already-safe) bytes.
///
/// Takes the composed registration-ordered definitions snapshot (byte-equal
/// to `registry.definitions()` — see [`SessionComposition`]) instead of
/// the registry itself, because the registry is borrowed by the live
/// application for the whole session and cannot be moved or cloned.
pub(super) fn render_tools_segment<P>(
    tool_definitions: &[siralos_core::tool::registry::RegisteredToolInfo],
    policy: &PermissionPolicy,
    application: &SiralosApplication<'_, P>,
) -> String
where
    P: siralos_core::provider::ModelProvider,
{
    let mut out = format_tools(tool_definitions, policy);
    out.push_str(&format_tool_projection(application.last_projection()));
    // Tool/module descriptions are external content. Keep the single output
    // boundary even for the stdio `/tools` path.
    sanitize_for_display(&out)
}

/// Render the answer one request command received (C2 step 3).
///
/// The wording is the shared bridge's, so a refusal cannot look like success
/// and the sanitizer stays the single output boundary. A worker that died
/// before answering has already been reported by the relay.
pub(super) fn render_answer(
    answer: Option<WorkerEvent>,
    sink: &mut crate::tui::TuiSink,
) -> Result<(), InteractiveError> {
    let Some(event) = answer else {
        return Ok(());
    };
    let mut sanitizer = TerminalSanitizer::new();
    let mut scratch_pane = None;
    crate::session_worker::apply_worker_event(
        event,
        &mut sanitizer,
        sink,
        &mut |_| {},
        &mut scratch_pane,
    )
    .map_err(InteractiveError::Io)
}

/// Stage 5.8 (decision 54): evaluate the applied profile's context
/// control against the rendered `/context` claim. Without a control the
/// render is byte-for-byte transparent (R7.5 rubric). `Pinned`-stale
/// keeps the claim usable but appends a truthful label; `Frozen`-stale
/// refuses the claim use with a typed refusal before anything renders.
fn render_context_claim(raw: &str, control: Option<&ContextPolicy>) -> String {
    let Some(control) = control else {
        return raw.to_owned();
    };
    let observed = siralos_core::identity::sha256_hex(raw.as_bytes());
    let decision = decide_context_control(Some(control), &observed);
    match &decision.reason {
        None => format!(
            "{raw}Context control: context claim {} (bound {})\n",
            decision.outcome.disposition(),
            &observed[..8],
        ),
        Some(reason) if decision.outcome.usable() => {
            format!("{raw}Context control: context claim stale ({reason})\n")
        }
        Some(reason) => format!("Context projection refused: {reason}\n"),
    }
}
/// The opening delimiter prefix for a projected untrusted skill guidance block.
/// The suffix carries caller-controlled name and digest fields, so the prefix
/// itself must be escaped in skill content before the host adds the real block.
const SKILL_GUIDANCE_START_DELIMITER: &str = "<<<UNTRUSTED_WORKSPACE_GUIDANCE";
const SKILL_GUIDANCE_END_DELIMITER: &str =
    "<<<END_UNTRUSTED_WORKSPACE_GUIDANCE>>>";

/// Escape delimiter syntax in untrusted skill content before it is wrapped in
/// the host-owned guidance block. Only the host-generated wrappers remain
/// parseable; skill text cannot create or close another guidance block.
fn escape_skill_guidance_delimiters(content: &str) -> String {
    content
        .replace(SKILL_GUIDANCE_START_DELIMITER, "[escaped start marker]")
        .replace(SKILL_GUIDANCE_END_DELIMITER, "[escaped end marker]")
}

/// Stage 5.10 (decision 56): resolve the applied profile's opt-in skill
/// selection against the workspace skill catalog. Guidance only — the
/// consumption can never add capability, Tool, or permission. Returns
/// the bounded, deterministic workspace-skills guidance segment when at
/// least one skill binds; absent selection/catalog or unknown
/// selections are reported truthfully and leave the session
/// byte-transparent (R7.5 preserved). `active_credential` is redacted at
/// the final projection boundary when one is resolved for the session.
pub(super) fn compose_skills_segment(
    workspace_root: &Path,
    loaded_profile: &WorkspaceProfileLoad,
    effective: &EffectiveRunPolicy,
    active_credential: Option<&HostCredential>,
) -> Option<SegmentInput> {
    let session_skills: Option<Vec<String>> =
        if effective.applied_profile.is_some() {
            match loaded_profile {
                WorkspaceProfileLoad::Record(record) => record.skills.clone(),
                _ => None,
            }
        } else {
            None
        };
    let loaded_catalog = match load_workspace_skills(workspace_root) {
        Ok(SkillCatalogLoad::Catalog(catalog)) => Some(catalog),
        Ok(SkillCatalogLoad::Absent) => None,
        Err(failure) => {
            eprintln!(
                "siralos: skill catalog not trusted: {}",
                failure.message
            );
            None
        }
    };
    let skill_catalog_state = match &loaded_catalog {
        Some(catalog) => SkillCatalogState::Loaded(catalog),
        None => SkillCatalogState::Absent,
    };
    let skill_consumption = compose_skill_consumption(
        session_skills.as_deref(),
        skill_catalog_state,
    );
    if !skill_consumption.resolution.unknown.is_empty() {
        let safe_unknown = skill_consumption
            .resolution
            .unknown
            .iter()
            .map(|name| safe_report_identifier(name))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "siralos: skills not in the workspace catalog: {safe_unknown}"
        );
    }
    // Bound guidance applies for both `bound` and `unknown` outcomes
    // (the bound subset applies; unknown names are reported truthfully
    // above). Only `none` leaves the session byte-transparent.
    if skill_consumption.resolution.bound.is_empty() {
        return None;
    }
    // Bounded, deterministic guidance segment: sorted by name (the
    // catalog and resolution are sorted), capped at the skill-content
    // bound per skill by the loader.
    let mut guidance = String::new();
    for reference in &skill_consumption.resolution.bound {
        if let Some(skill) = loaded_catalog
            .as_ref()
            .and_then(|catalog| catalog.get(&reference.name))
        {
            let name = safe_report_identifier(skill.name());
            let content = escape_skill_guidance_delimiters(skill.content());
            guidance.push_str(&format!(
                "{SKILL_GUIDANCE_START_DELIMITER} name={name} digest={}>>>\n{content}\n{SKILL_GUIDANCE_END_DELIMITER}\n",
                skill.digest()
            ));
        }
    }
    if guidance.is_empty() {
        return None;
    }
    // Redact at the final projection boundary so resolved credential bytes
    // cannot survive in either untrusted content or host-added metadata.
    let guidance = match active_credential {
        Some(credential) => credential.redact_text(&guidance),
        None => guidance,
    };
    Some(SegmentInput {
        id: "workspace-skills".to_owned(),
        stability: Stability::Stable,
        title: "Workspace skills".to_owned(),
        content: guidance,
    })
}
/// Re-render the status row with a transient base, keeping the worker's
/// context-usage suffix (C2 step 3).
///
/// The frontend holds no metrics any more, so the suffix is cached from the
/// last `Ready`; without it a transient status ("fetching models...") would
/// silently drop the context readout an opted-in session shows.
pub(super) fn transient_status(state: &TuiState, base: &str) -> String {
    format!(
        "{}{}",
        crate::tui::compose_status_line(
            base,
            state.provider.as_deref(),
            state.model.as_deref(),
        ),
        state.context_suffix,
    )
}
