//! Provider snapshot reload: fresh-load recomposition, approval-bound
//! authority revalidation, and the reload report the frontends print.

#[cfg(test)]
use super::load_workspace_profile_snapshot;
use super::{
    ComposedUserConfig, DeclaredProfile, EffectiveRunPolicy, HostCredential,
    Path, PermissionPolicy, PermissionRule, PolicyRule, SessionProvider,
    WorkspaceProfileLoad, WorkspaceProfileSnapshot, compose_effective_policy,
    declare_profile, display_endpoint_redacted, display_field_redacted,
    load_user_configuration, read_profile_bytes_bounded,
    redacted_credential_token, safe_profile_diagnostic,
};

/// One recomposed provider snapshot — the routing configuration startup
/// threads through the event loop (provider/model/credential/endpoint/
/// protocol). Pure data: no live handles, no mutation.
#[derive(Clone, PartialEq, Eq)]
struct ProviderSnapshot {
    /// Applied provider id (`None` = session default on pure Host policy).
    provider: Option<String>,
    /// Whether a valid profile record supplied this snapshot. An explicitly
    /// empty record is distinct from an absent/invalid profile.
    applied: bool,
    /// Applied model id.
    model: Option<String>,
    /// Applied model display name (`None` = prefer the raw id).
    model_display_name: Option<String>,
    /// Applied credential raw string (compared redacted-ly, never resolved).
    credential_raw: Option<String>,
    /// Applied endpoint override.
    endpoint: Option<String>,
    /// Applied protocol string.
    protocol: String,
    /// Composition refusal/validation diagnostic, if a record did not apply.
    diagnostic: Option<String>,
}

impl std::fmt::Debug for ProviderSnapshot {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderSnapshot")
            .field(
                "provider",
                &self.provider.as_deref().map(|value| {
                    display_field_redacted(
                        Some(value),
                        self.credential_raw.as_deref(),
                    )
                }),
            )
            .field(
                "model",
                &self.model.as_deref().map(|value| {
                    display_field_redacted(
                        Some(value),
                        self.credential_raw.as_deref(),
                    )
                }),
            )
            .field(
                "model_display_name",
                &self.model_display_name.as_deref().map(|value| {
                    display_field_redacted(
                        Some(value),
                        self.credential_raw.as_deref(),
                    )
                }),
            )
            .field(
                "credential_raw",
                &self.credential_raw.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "endpoint",
                &self.endpoint.as_ref().map(|value| {
                    if self.credential_raw.is_some() {
                        "[credential-bearing endpoint]".to_owned()
                    } else {
                        siralos_adapters::provider::safe_endpoint_for_output(
                            value,
                        )
                    }
                }),
            )
            .field("protocol", &self.protocol)
            .field(
                "diagnostic",
                &self.diagnostic.as_ref().map(|_| "[SANITIZED]"),
            )
            .finish()
    }
}

/// The parts of a recomposed snapshot `/reload` can apply to a live session:
/// the model plus the display name that describes it, the endpoint base, and
/// the protocol that selects the POST path segment.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ReloadedConfig {
    /// Recomposed provider id, when the profile names one.
    pub(super) provider: Option<String>,
    /// Recomposed model id, when the profile names one.
    pub(super) model: Option<String>,
    /// Recomposed display name (`None` = prefer the raw id).
    pub(super) display_name: Option<String>,
    /// Recomposed endpoint base (`None` = the provider-neutral placeholder).
    pub(super) endpoint: Option<String>,
    /// Recomposed protocol.
    pub(super) protocol: siralos_core::composition::Protocol,
    /// The credential form the profile declares (`env:NAME`, `key:VALUE`,
    /// a bare legacy env name, or `None` when none is declared). Resolved
    /// at apply time so a credential added mid-session converges.
    pub(super) credential_raw: Option<String>,
}

impl std::fmt::Debug for ReloadedConfig {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ReloadedConfig")
            .field(
                "provider",
                &self.provider.as_deref().map(|value| {
                    display_field_redacted(
                        Some(value),
                        self.credential_raw.as_deref(),
                    )
                }),
            )
            .field(
                "model",
                &self.model.as_deref().map(|value| {
                    display_field_redacted(
                        Some(value),
                        self.credential_raw.as_deref(),
                    )
                }),
            )
            .field(
                "display_name",
                &self.display_name.as_deref().map(|value| {
                    display_field_redacted(
                        Some(value),
                        self.credential_raw.as_deref(),
                    )
                }),
            )
            .field(
                "endpoint",
                &self.endpoint.as_ref().map(|value| {
                    if self.credential_raw.is_some() {
                        "[credential-bearing endpoint]".to_owned()
                    } else {
                        siralos_adapters::provider::safe_endpoint_for_output(
                            value,
                        )
                    }
                }),
            )
            .field("protocol", &self.protocol)
            .field(
                "credential_raw",
                &self.credential_raw.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Project a recomposed snapshot onto the parts `/reload` applies.
///
/// The EMPTY snapshot is what `recompose_provider_snapshot` returns when no
/// profile applied (absent, invalid or refused), so an empty projection means
/// "nothing to apply" -- never "clear every field".
fn reloaded_config(fresh: &ProviderSnapshot) -> Option<ReloadedConfig> {
    if fresh.diagnostic.is_some() {
        return None;
    }
    let empty = fresh.provider.is_none()
        && fresh.model.is_none()
        && fresh.model_display_name.is_none()
        && fresh.credential_raw.is_none()
        && fresh.endpoint.is_none()
        && fresh.protocol
            == siralos_core::composition::Protocol::default().as_str();
    if empty && !fresh.applied {
        return None;
    }
    Some(ReloadedConfig {
        provider: fresh.provider.clone(),
        model: fresh.model.clone(),
        display_name: fresh.model_display_name.clone(),
        endpoint: fresh.endpoint.clone(),
        protocol: siralos_core::composition::Protocol::parse(&fresh.protocol)
            .unwrap_or_default(),
        credential_raw: fresh.credential_raw.clone(),
    })
}

/// Apply a recomposed configuration to the live session: the provider cells the
/// NEXT request reads (model, endpoint base, protocol) plus the display holders
/// the status line shows. The profile file is the truth here, so this never
/// writes it and always adopts the file's display name (clearing it when the
/// file has none). Each transition is reported; an unchanged value is a no-op,
/// which is what keeps the pure report path byte-identical.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_reloaded_config(
    live_provider: &SessionProvider,
    current_provider: Option<&str>,
    current_live_model: Option<&str>,
    applied_model: &mut Option<String>,
    applied_model_display_name: &mut Option<String>,
    applied_endpoint: &mut Option<String>,
    applied_protocol_str: &mut String,
    applied_credential: &mut Option<HostCredential>,
    applied_credential_raw: &mut Option<String>,
    recomposed: Option<ReloadedConfig>,
    report: &mut String,
) -> bool {
    let Some(recomposed) = recomposed else {
        return true;
    };
    // Resolve the complete credential before changing ANY live route cell. A
    // profile can change the endpoint and credential together; if the new
    // environment reference is unresolved, retaining the old credential while
    // the new endpoint becomes live would send that secret to a new
    // destination. Refuse the whole live transition in that case.
    let resolved_credential = match recomposed.credential_raw.as_deref() {
        None => Some(None),
        Some(raw) => match HostCredential::from_credential_str(raw) {
            Ok(credential) => Some(Some(credential)),
            Err(_reason) => {
                report.push_str(
                    "reload not applied: credential could not be resolved (details hidden)\n",
                );
                return false;
            }
        },
    };
    let incoming_credential = recomposed.credential_raw.as_deref();
    let redact_both = |value: Option<&str>| {
        let first =
            display_field_redacted(value, applied_credential_raw.as_deref());
        display_field_redacted(Some(&first), incoming_credential)
    };
    if recomposed.provider.as_deref() != current_provider {
        report.push_str(
            "restart required: provider changed; the current adapter was not replaced\n",
        );
        return false;
    }
    let model_changed = recomposed.model.as_deref()
        != current_live_model.or(applied_model.as_deref());
    let endpoint_changed =
        applied_endpoint.as_deref() != recomposed.endpoint.as_deref();
    let protocol_changed =
        applied_protocol_str != recomposed.protocol.as_str();
    let credential_changed =
        match (&resolved_credential, applied_credential.as_ref()) {
            (Some(Some(resolved)), Some(current)) => {
                applied_credential_raw.as_deref()
                    != recomposed.credential_raw.as_deref()
                    || !current.same_credential(resolved)
            }
            (Some(Some(_)), None) => true,
            (Some(None), None) => {
                applied_credential_raw.as_deref()
                    != recomposed.credential_raw.as_deref()
            }
            (Some(None), Some(_)) => true,
            (None, _) => true,
        };
    if recomposed.model.is_none() && applied_model.is_some() {
        report.push_str(
            "restart required: model removal is not a live transition\n",
        );
        return false;
    }
    if (model_changed && !live_provider.supports_live_model())
        || (credential_changed && !live_provider.supports_live_credential())
    {
        report.push_str(
            "restart required: one or more live route changes are unsupported; no partial state was applied\n",
        );
        return false;
    }
    // Route PREFLIGHT. A provider with no live route is refused here, before
    // the model cell or any display holder is touched: applying the model
    // first and refusing the endpoint afterwards would leave a half-applied
    // composition, which the caller then revokes while the session's own
    // label already moved. The messages match the branches below exactly, so
    // the refusal is the same either way it is reached.
    if endpoint_changed
        && recomposed.endpoint.as_deref().is_some_and(|value| {
            !siralos_core::composition::is_valid_http_endpoint(value)
                || value.contains(' ')
                || value.contains('\0')
        })
    {
        // A hand-edited endpoint can be syntactically present and still be
        // refused by the adapter. Deciding that here keeps the promise the
        // report makes: NOTHING was applied when this branch runs.
        report.push_str(
            "restart required: the declared endpoint is not a valid HTTP(S) route; no partial state was applied\n",
        );
        return false;
    }
    if endpoint_changed
        && !live_provider.supports_live_endpoint()
        && !live_provider.fixed_typed_route()
    {
        report.push_str(
            "restart required: endpoint changed; this provider has no live route\n",
        );
        return false;
    }
    if protocol_changed
        && !live_provider.supports_live_protocol()
        && !live_provider.fixed_typed_route()
    {
        report.push_str(
            "restart required: protocol changed; this provider has no live route\n",
        );
        return false;
    }
    // Model: the provider cell behind `stream()`, plus the display name the
    // profile file declares for it.
    if let Some(model) = recomposed.model {
        let current =
            current_live_model.or(applied_model.as_deref()).map(str::to_owned);
        if current.as_deref() != Some(model.as_str()) {
            if !live_provider.set_live_model(&model) {
                report.push_str(
                    "restart required: model switch was not applied\n",
                );
                return false;
            }
            *applied_model = Some(model.clone());
            *applied_model_display_name = recomposed.display_name.clone();
            report.push_str(&format!(
                "applied: model {} -> {} (live, no restart)\n",
                redact_both(current.as_deref()),
                redact_both(Some(model.as_str()))
            ));
        } else if applied_model_display_name.as_deref()
            != recomposed.display_name.as_deref()
        {
            *applied_model_display_name = recomposed.display_name.clone();
            report.push_str(
                "applied: model display name changed (live, no restart)\n",
            );
        }
    }
    // Endpoint base: the value the NEXT request resolves its URL from. The
    // endpoint VALUE is never echoed -- the same rule the report follows.
    if endpoint_changed {
        if live_provider.supports_live_endpoint() {
            if live_provider.set_live_endpoint(recomposed.endpoint.clone()) {
                *applied_endpoint = recomposed.endpoint.clone();
                report.push_str(
                    "applied: endpoint changed (live, no restart)\n",
                );
            } else {
                report.push_str(
                    "restart required: endpoint change was not applied\n",
                );
                return false;
            }
        } else if live_provider.fixed_typed_route() {
            // Typed adapters deliberately keep their fixed wire route. A
            // profile-level cosmetic endpoint is not a live transition, and
            // must not make an otherwise healthy named session look revoked.
            *applied_endpoint = recomposed.endpoint.clone();
            report.push_str(
                "ignored: endpoint is fixed for this provider (not applied live)\n",
            );
        } else {
            report.push_str(
                "restart required: endpoint changed; this provider has no live route\n",
            );
            return false;
        }
    }
    // Protocol: selects the POST path segment appended to that base.
    if protocol_changed {
        let before = applied_protocol_str.clone();
        if live_provider.supports_live_protocol() {
            if live_provider.set_live_protocol(recomposed.protocol) {
                *applied_protocol_str =
                    recomposed.protocol.as_str().to_owned();
                report.push_str(&format!(
                    "applied: protocol {before} -> {} (live, no restart)\n",
                    applied_protocol_str
                ));
            } else {
                report.push_str(
                    "restart required: protocol change was not applied\n",
                );
                return false;
            }
        } else if live_provider.fixed_typed_route() {
            *applied_protocol_str = recomposed.protocol.as_str().to_owned();
            report.push_str(
                "ignored: protocol is fixed for this provider (not applied live)\n",
            );
        } else {
            report.push_str(
                "restart required: protocol changed; this provider has no live route\n",
            );
            return false;
        }
    }
    // Credential: resolved fresh from the declared form, because a
    // credential that appears AFTER composition is exactly what a
    // mid-session `/provider` add produces -- and silently sending the
    // request without it is what made that add look like a 401 from the
    // provider. The value is never echoed; a resolution failure is
    // reported instead of swallowed.
    let declared = recomposed.credential_raw.clone();
    let credential_needs_retry = credential_changed
        || matches!(
            &resolved_credential,
            Some(Some(_)) if applied_credential.is_none()
        );
    if credential_needs_retry {
        match resolved_credential {
            Some(None) => {
                // A profile that declares none clears the live one: a stale
                // secret must never keep flowing to a provider that stopped
                // declaring it.
                if live_provider.set_live_credential(None) {
                    *applied_credential = None;
                    *applied_credential_raw = None;
                    report.push_str(
                        "applied: credential cleared (live, no restart)\n",
                    );
                } else {
                    report.push_str(
                        "restart required: credential removal; this provider keeps its composed credential\n",
                    );
                    return false;
                }
            }
            Some(Some(resolved)) => {
                if live_provider.set_live_credential(Some(resolved.clone())) {
                    let changed = credential_changed;
                    *applied_credential = Some(resolved);
                    *applied_credential_raw = declared;
                    if changed {
                        report.push_str(
                            "applied: credential changed (live, no restart)\n",
                        );
                    }
                } else {
                    report.push_str(
                        "not applied: credential (this provider keeps the credential it was composed with; restart to converge)\n",
                    );
                    return false;
                }
            }
            None => {
                unreachable!("credential validation returns before this point")
            }
        }
    }
    true
}

/// The session's Host rules — read-only workspace inspection,
/// Allow; workspace mutation, Ask (ADR 0038: every mutation needs a
/// one-time host approval for the exact action, and the approval gate
/// holds even under an allow rule); command execution, Ask (the
/// `bash` tool reports typed unavailability where no live probe
/// proves enforcement, and profiles can only narrow toward deny).
///
/// The ONE rule set `compose_session` composes the workspace profile
/// against (R7.4 fail-closed posture; Stage 5.2 narrowing-only). `/reload`
/// recomposes against these same rules through
/// [`declare_and_compose_profile`] — the same composition path, never a
/// second one.
pub(super) fn session_host_rules() -> Vec<PolicyRule> {
    vec![
        PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "workspace.read",
            )
            .expect("workspace.read is a valid capability id"),
            rule: PermissionRule::Allow,
        },
        PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "workspace.write",
            )
            .expect("workspace.write is a valid capability id"),
            rule: PermissionRule::Ask,
        },
        PolicyRule {
            capability: siralos_core::tool::CapabilityId::parse(
                "process.execute",
            )
            .expect("process.execute is a valid capability id"),
            rule: PermissionRule::Ask,
        },
    ]
}

/// Declare + compose a loaded profile — the SAME declare/compose pair
/// `compose_session` runs at startup (`load_workspace_profile` →
/// `declare_profile` → `compose_effective_policy`). Both the startup path
/// and `/reload` call this one function; there is no second composition.
pub(super) fn declare_and_compose_profile(
    loaded_profile: &WorkspaceProfileLoad,
    host_rules: &[PolicyRule],
) -> EffectiveRunPolicy {
    let declared = match loaded_profile {
        WorkspaceProfileLoad::Record(record) => declare_profile(
            Some(record),
            &PermissionPolicy::from_rules(host_rules.to_vec()),
        ),
        WorkspaceProfileLoad::Absent => DeclaredProfile::Absent,
        WorkspaceProfileLoad::Invalid { diagnostic } => {
            DeclaredProfile::Invalid { diagnostic: diagnostic.clone() }
        }
    };
    compose_effective_policy(host_rules, &declared)
}

fn profile_requires_explicit_approval(
    record: &siralos_core::composition::ProfileRecord,
) -> bool {
    record.credential.is_some()
        || record.endpoint.is_some()
        || record.record_replay
        || record.replay
        || record.context_system_enabled
}

/// Apply the trusted user-config approval to an untrusted workspace snapshot.
/// A digest mismatch refuses the whole profile rather than selectively
/// applying a partially trusted authority set.
pub(super) fn approve_workspace_profile(
    snapshot: &WorkspaceProfileSnapshot,
    approval: Option<&str>,
) -> WorkspaceProfileLoad {
    let load = snapshot.load();
    let WorkspaceProfileLoad::Record(record) = load else {
        return load.clone();
    };
    if !profile_requires_explicit_approval(record)
        || approval == Some(snapshot.raw_sha256())
    {
        return load.clone();
    }
    WorkspaceProfileLoad::Invalid {
        diagnostic: format!(
            "workspace profile requires explicit approval for digest {}: \
             compute the SHA-256 of siralos.toml yourself and put it in \
             ~/.siralos/config.json as profileApproval (the prefix is for \
             comparison, not for copying)",
            &snapshot.raw_sha256()[..16]
        ),
    }
}

/// Revalidate both authorities before a live reload is allowed to apply.
/// A profile is accepted only when the exact snapshot still has a bindable
/// identity and the trusted user configuration is still the configuration
/// whose approval was checked. Any failure is a refusal, never a partial
/// route update.
pub(super) fn reload_authority_is_revalidated(
    workspace_root: &Path,
    config_path: &Path,
    snapshot: &WorkspaceProfileSnapshot,
    approved: &WorkspaceProfileLoad,
    expected_config: &ComposedUserConfig,
) -> bool {
    if matches!(approved, WorkspaceProfileLoad::Invalid { .. }) {
        return false;
    }
    let Some(profile_token) = snapshot.write_token() else {
        return false;
    };
    let profile_path = workspace_root
        .join(siralos_adapters::module::manifest::SIRALOS_TOML_FILE_NAME);
    let current_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &profile_path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(_) => return false,
    };
    if !profile_token.matches_path(&profile_path, current_bytes.as_deref()) {
        return false;
    }
    let current_config = match load_user_configuration(Some(config_path)) {
        Ok(config) => config,
        Err(_) => return false,
    };
    if current_config.config != expected_config.config
        || current_config.review_provider_id
            != expected_config.review_provider_id
    {
        return false;
    }
    // Close the profile/config cross-read window: the trusted config was
    // checked after the first profile observation, so re-observe the profile
    // before accepting the live route.
    let final_bytes: Option<Vec<u8>> = match read_profile_bytes_bounded(
        &profile_path,
        siralos_adapters::module::manifest::MAX_SIRALOS_TOML_BYTES,
    ) {
        Ok(bytes) => Some(bytes),
        Err(reason) if reason == "profile target is absent" => None,
        Err(_) => return false,
    };
    profile_token.matches_path(&profile_path, final_bytes.as_deref())
}

pub(super) struct ReloadAuthorityEvidence {
    pub(super) snapshot: WorkspaceProfileSnapshot,
    pub(super) approved: WorkspaceProfileLoad,
    pub(super) config: ComposedUserConfig,
}

pub(super) fn effective_profile_protocol(
    provider: Option<&str>,
    declared: siralos_core::composition::Protocol,
) -> siralos_core::composition::Protocol {
    if provider == Some("anthropic") {
        siralos_core::composition::Protocol::AnthropicMessages
    } else {
        declared
    }
}

/// startup uses: [`load_workspace_profile`] then
/// [`declare_and_compose_profile`] over [`session_host_rules`], then the
/// applied-record projection `compose_session` performs. Pure: reads the
/// workspace file, holds no live handles, mutates nothing.
fn recompose_provider_snapshot_from_load(
    loaded_profile: &WorkspaceProfileLoad,
) -> ProviderSnapshot {
    let host_rules = session_host_rules();
    let effective = declare_and_compose_profile(loaded_profile, &host_rules);
    match loaded_profile {
        WorkspaceProfileLoad::Record(record)
            if effective.applied_profile.is_some() =>
        {
            ProviderSnapshot {
                applied: true,
                provider: record.provider.clone(),
                model: record.model.clone(),
                model_display_name: record.model_display_name.clone(),
                credential_raw: record.credential.clone(),
                endpoint: record.endpoint.clone(),
                protocol: effective_profile_protocol(
                    record.provider.as_deref(),
                    record.protocol,
                )
                .as_str()
                .to_owned(),
                diagnostic: effective.diagnostic,
            }
        }
        _ => ProviderSnapshot {
            applied: false,
            provider: None,
            model: None,
            model_display_name: None,
            credential_raw: None,
            endpoint: None,
            protocol: siralos_core::composition::Protocol::default()
                .as_str()
                .to_owned(),
            diagnostic: effective.diagnostic,
        },
    }
}

/// Pure `/reload` report: recompose the session's provider configuration
/// from the workspace `siralos.toml` WITHOUT restarting, and describe what
/// WOULD change relative to the live session snapshot — no live-state
/// mutation at all. Three cases:
///
/// - profile EDITED: one `; `-joined line naming each changed field, e.g.
///   `provider unchanged; model example/model-a -> example/model-b;
///   endpoint changed`.
/// - profile INVALID: `reload not applied: <diagnostic verbatim>` — the
///   exact `load_workspace_profile` diagnostic, nothing recomposed.
/// - profile ABSENT: startup falls back to the deterministic fake on pure
///   Host policy, so the report says what that fallback WOULD do.
#[cfg(test)]
pub(super) fn reload_report(
    workspace_root: &Path,
    current_provider: Option<&str>,
    current_model: Option<&str>,
    current_credential_raw: Option<&str>,
    current_endpoint: Option<&str>,
    current_protocol: &str,
) -> (String, Option<ReloadedConfig>) {
    let snapshot = load_workspace_profile_snapshot(workspace_root);
    reload_report_with_approval(
        &snapshot,
        current_provider,
        current_model,
        current_credential_raw,
        current_endpoint,
        current_protocol,
        Some(snapshot.raw_sha256()),
    )
}

pub(super) fn non_live_state_digest(
    loaded_profile: &WorkspaceProfileLoad,
) -> Option<String> {
    match loaded_profile {
        WorkspaceProfileLoad::Invalid { .. } => None,
        WorkspaceProfileLoad::Absent => Some("absent".to_owned()),
        WorkspaceProfileLoad::Record(record) => {
            let payload = format!(
                "overlay={:?};modules={:?};context={:?};skills={:?};record_replay={};replay={};context_system_enabled={}",
                record.overlay,
                record.modules,
                record.context,
                record.skills,
                record.record_replay,
                record.replay,
                record.context_system_enabled,
            );
            Some(siralos_core::identity::sha256_hex(payload.as_bytes()))
        }
    }
}

/// Test-only helper behind `reload_report`: its only caller is the `#[cfg(test)]`
/// report shim, so it is not compiled into the product binary.
#[cfg(test)]
fn reload_report_with_approval(
    snapshot: &WorkspaceProfileSnapshot,
    current_provider: Option<&str>,
    current_model: Option<&str>,
    current_credential_raw: Option<&str>,
    current_endpoint: Option<&str>,
    current_protocol: &str,
    approval: Option<&str>,
) -> (String, Option<ReloadedConfig>) {
    let approved = approve_workspace_profile(snapshot, approval);
    reload_report_from_load(
        &approved,
        current_provider,
        current_model,
        current_credential_raw,
        current_endpoint,
        current_protocol,
    )
}

pub(super) fn reload_report_from_load(
    loaded_profile: &WorkspaceProfileLoad,
    current_provider: Option<&str>,
    current_model: Option<&str>,
    current_credential_raw: Option<&str>,
    current_endpoint: Option<&str>,
    current_protocol: &str,
) -> (String, Option<ReloadedConfig>) {
    match loaded_profile {
        WorkspaceProfileLoad::Invalid { diagnostic } => (
            format!(
                "reload not applied: {}\n",
                safe_profile_diagnostic(diagnostic)
            ),
            None,
        ),
        WorkspaceProfileLoad::Absent => {
            let fresh = recompose_provider_snapshot_from_load(loaded_profile);
            let want_provider = fresh.provider.as_deref();
            let want_model = fresh.model.as_deref();
            let want_endpoint = fresh.endpoint.as_deref();
            let provider_unchanged =
                want_provider == current_provider.filter(|s| !s.is_empty());
            let model_unchanged =
                want_model == current_model.filter(|s| !s.is_empty());
            let endpoint_unchanged =
                want_endpoint == current_endpoint.filter(|s| !s.is_empty());
            if provider_unchanged && model_unchanged && endpoint_unchanged {
                ("reload: no profile configured — startup would use the deterministic fake on pure Host policy; live session already there, nothing would change\n"
                    .to_owned(), reloaded_config(&fresh))
            } else {
                ("reload: no profile configured — startup would use the deterministic fake on pure Host policy; live session differs, restart to converge\n"
                    .to_owned(), reloaded_config(&fresh))
            }
        }
        WorkspaceProfileLoad::Record(_) => {
            let fresh = recompose_provider_snapshot_from_load(loaded_profile);
            let mut parts: Vec<String> = Vec::new();
            // Non-live profile state is compared against the applied session
            // snapshot by `Session::reload`; this pure report function must not
            // infer a transition merely because the new record contains keys.
            if let Some(diagnostic) = fresh.diagnostic.as_deref() {
                return (
                    format!(
                        "reload not applied: {}\n",
                        safe_profile_diagnostic(diagnostic)
                    ),
                    None,
                );
            }
            let incoming_credential = fresh.credential_raw.as_deref();
            let redact_both = |value: Option<&str>| {
                let first =
                    display_field_redacted(value, current_credential_raw);
                display_field_redacted(Some(&first), incoming_credential)
            };
            let redact_endpoint_both = |value: Option<&str>| {
                let first =
                    display_endpoint_redacted(value, current_credential_raw);
                display_endpoint_redacted(Some(&first), incoming_credential)
            };
            let want_provider = fresh.provider.as_deref();
            let current_provider = current_provider.filter(|s| !s.is_empty());
            if want_provider == current_provider {
                parts.push("provider unchanged".to_owned());
            } else {
                parts.push(format!(
                    "provider {} -> {} (restart to converge)",
                    redact_both(current_provider),
                    redact_both(want_provider)
                ));
            }
            let want_model = fresh.model.as_deref();
            let current_model = current_model.filter(|s| !s.is_empty());
            if want_model == current_model {
                parts.push("model unchanged".to_owned());
            } else {
                parts.push(format!(
                    "model {} -> {}",
                    redact_both(current_model),
                    redact_both(want_model)
                ));
            }
            let credential_same =
                fresh.credential_raw.as_deref() == current_credential_raw;
            if credential_same {
                parts.push("credential unchanged".to_owned());
            } else {
                let want_cred =
                    redacted_credential_token(fresh.credential_raw.as_deref());
                let current_cred =
                    redacted_credential_token(current_credential_raw);
                parts
                    .push(format!("credential {current_cred} -> {want_cred}"));
            }
            let want_endpoint = fresh.endpoint.as_deref();
            let current_endpoint = current_endpoint.filter(|s| !s.is_empty());
            if want_endpoint == current_endpoint {
                parts.push("endpoint unchanged".to_owned());
            } else if want_endpoint.is_none() || current_endpoint.is_none() {
                parts.push(format!(
                    "endpoint {} -> {}",
                    redact_endpoint_both(current_endpoint),
                    redact_endpoint_both(want_endpoint)
                ));
            } else {
                parts.push("endpoint changed".to_owned());
            }
            if fresh.protocol == current_protocol {
                parts.push("protocol unchanged".to_owned());
            } else {
                parts.push(format!(
                    "protocol {current_protocol} -> {}",
                    fresh.protocol
                ));
            }
            if parts.iter().all(|p| p.ends_with("unchanged")) {
                (
                    format!("reload: {}\n", parts.join("; ")),
                    reloaded_config(&fresh),
                )
            } else {
                (
                    format!("reload would change: {}\n", parts.join("; ")),
                    reloaded_config(&fresh),
                )
            }
        }
    }
}
