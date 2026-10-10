//! Godot domain subject record builders.
use crate::harness::*;

pub(in crate::harness) fn godot_record(
    subject: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    match subject {
        SUBJECT_GODOT_SCENE_RESOLVE => godot_scene_resolve_record(input),
        SUBJECT_GODOT_DISCOVERY => godot_discovery_record(input),
        SUBJECT_GODOT_KNOWLEDGE => godot_knowledge_record(input),
        SUBJECT_GODOT_DIAGNOSTICS => godot_diagnostics_record(input),
        SUBJECT_GODOT_LSP => godot_lsp_record(input),
        SUBJECT_GODOT_REVIEW_CONTEXT => godot_review_context_record(input),
        SUBJECT_GODOT_MUTATION_PREPARE => godot_mutation_prepare_record(input),
        SUBJECT_GODOT_DEVELOP_PLAN => godot_develop_plan_record(input),
        SUBJECT_CI_ARTIFACT_DIGEST => {
            content_identity_artifact_digest_record(input)
        }
        SUBJECT_CI_CONTRACT_DIGEST => {
            content_identity_contract_digest_record(input)
        }
        SUBJECT_CI_MANIFESTS => content_identity_manifests_record(input),
        SUBJECT_CI_DELTA => content_identity_delta_record(input),
        SUBJECT_DET_REPLAY => determinism_replay_record(input),
        _ => unreachable!(
            "godot subject was validated while loading the corpus"
        ),
    }
}

/// Transcription of the oracle probe's selection rule
/// (`godot-scene-resolve-oracle.mjs`): tres wins as a resource; a tscn
/// payload containing `[gd_resource` parses as a resource; anything else
/// parses as a scene; missing keys are null, unknown keys are ignored,
/// and the default path is empty.
fn godot_scene_resolve_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::godot::scene::{
        GodotParseStatus, parse_godot_resource, parse_godot_scene,
    };
    let tres = input.get("tres").and_then(Value::as_str);
    let tscn = input.get("tscn").and_then(Value::as_str);
    let path = input.get("path").and_then(Value::as_str).unwrap_or("");
    let content;
    let is_resource;
    if let Some(tres_text) = tres {
        content = tres_text;
        is_resource = true;
    } else if tscn.is_some_and(|text| text.contains("[gd_resource")) {
        content = tscn.unwrap_or_default();
        is_resource = true;
    } else {
        content = tscn.unwrap_or("");
        is_resource = false;
    }
    let status_str;
    let diagnostics_len;
    let truncated;
    if is_resource {
        let doc = parse_godot_resource(content, path, None);
        status_str = match doc.status {
            GodotParseStatus::Complete => "complete",
            GodotParseStatus::Partial => "partial",
            GodotParseStatus::Invalid => "invalid",
        };
        diagnostics_len = doc.diagnostics.len();
        truncated = doc.truncated;
    } else {
        let doc = parse_godot_scene(content, path, None);
        status_str = match doc.status {
            GodotParseStatus::Complete => "complete",
            GodotParseStatus::Partial => "partial",
            GodotParseStatus::Invalid => "invalid",
        };
        diagnostics_len = doc.diagnostics.len();
        truncated = doc.truncated;
    }
    Ok(json!({
        "status": status_str,
        "diagnostics": diagnostics_len,
        "truncated": truncated,
    }))
}

fn godot_knowledge_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::adapters::godot::knowledge::GodotKnowledgeService;
    use siralos_godot::godot::{
        GodotApiSearchKind, GodotApiSearchQuery, GodotKnowledgeLookupOutcome,
        GodotKnowledgeQueryResult, GodotKnowledgeRefreshResult,
        KnowledgeState,
    };
    let platform =
        input.get("platform").and_then(Value::as_str).unwrap_or("win32");
    let mut service = GodotKnowledgeService::new(platform);
    match input.get("op").and_then(Value::as_str) {
        Some("status") => {
            let status = service.status();
            Ok(json!({
                "state": match status.state {
                    KnowledgeState::Ready => "ready",
                    KnowledgeState::Unavailable => "unavailable",
                    KnowledgeState::Unsupported => "unsupported",
                },
                "reason": status.reason,
                "platform": status.platform,
                "cacheEnabled": status.cache_enabled,
                "schemaVersion": status.schema_version,
                "profile": json!(null),
                "manualChannel": json!(null),
            }))
        }
        Some("refresh") => {
            let cancelled = input
                .get("cancelled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match service.refresh(cancelled) {
                GodotKnowledgeRefreshResult::NotReady { status, message } => {
                    Ok(json!({
                        "status": refresh_status_str(status),
                        "message": message,
                    }))
                }
                GodotKnowledgeRefreshResult::Ready { .. } => Err(
                    HarnessError::corpus(
                        "knowledge refresh cannot become ready without generation"
                            .to_owned(),
                    ),
                ),
            }
        }
        Some("search") => {
            let query = GodotApiSearchQuery {
                query: input
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                kinds: input.get("kinds").and_then(|value| {
                    value.as_array().map(|entries| {
                        entries
                            .iter()
                            .filter_map(godot_api_search_kind)
                            .collect::<Vec<GodotApiSearchKind>>()
                    })
                }),
                limit: input
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|limit| limit as usize),
            };
            match service.search(&query, false) {
                GodotKnowledgeQueryResult::NotReady { status, message } => Ok(
                    json!({
                        "status": query_status_str(status),
                        "message": message,
                    }),
                ),
                GodotKnowledgeQueryResult::Ready { .. } => Err(
                    HarnessError::corpus(
                        "knowledge search cannot become ready without a loaded base"
                            .to_owned(),
                    ),
                ),
            }
        }
        Some("lookup") => {
            let symbol = input
                .get("symbol")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match service.lookup(symbol, false) {
                GodotKnowledgeLookupOutcome::NotReady { status, message } => {
                    Ok(json!({
                        "status": lookup_status_str(status),
                        "message": message,
                    }))
                }
                GodotKnowledgeLookupOutcome::Ready { .. } => Err(
                    HarnessError::corpus(
                        "knowledge lookup cannot become ready without a loaded base"
                            .to_owned(),
                    ),
                ),
            }
        }
        _ => unreachable!("godot knowledge op was validated"),
    }
}

fn godot_api_search_kind(
    value: &Value,
) -> Option<siralos_godot::godot::GodotApiSearchKind> {
    use siralos_godot::godot::GodotApiSearchKind;
    match value.as_str() {
        Some("class") => Some(GodotApiSearchKind::Class),
        Some("method") => Some(GodotApiSearchKind::Method),
        Some("property") => Some(GodotApiSearchKind::Property),
        Some("signal") => Some(GodotApiSearchKind::Signal),
        Some("constant") => Some(GodotApiSearchKind::Constant),
        Some("enum") => Some(GodotApiSearchKind::Enum),
        Some("utility") => Some(GodotApiSearchKind::Utility),
        Some("operator") => Some(GodotApiSearchKind::Operator),
        _ => None,
    }
}

fn refresh_status_str(
    status: siralos_godot::godot::KnowledgeRefreshStatus,
) -> &'static str {
    use siralos_godot::godot::KnowledgeRefreshStatus;
    match status {
        KnowledgeRefreshStatus::Unavailable => "unavailable",
        KnowledgeRefreshStatus::Unsupported => "unsupported",
        KnowledgeRefreshStatus::Failed => "failed",
        KnowledgeRefreshStatus::Cancelled => "cancelled",
    }
}

fn query_status_str(
    status: siralos_godot::godot::KnowledgeQueryStatus,
) -> &'static str {
    use siralos_godot::godot::KnowledgeQueryStatus;
    match status {
        KnowledgeQueryStatus::Unavailable => "unavailable",
        KnowledgeQueryStatus::InvalidInput => "invalid_input",
        KnowledgeQueryStatus::Cancelled => "cancelled",
    }
}

fn lookup_status_str(
    status: siralos_godot::godot::KnowledgeLookupStatus,
) -> &'static str {
    use siralos_godot::godot::KnowledgeLookupStatus;
    match status {
        KnowledgeLookupStatus::NotFound => "not_found",
        KnowledgeLookupStatus::Unavailable => "unavailable",
        KnowledgeLookupStatus::InvalidInput => "invalid_input",
        KnowledgeLookupStatus::Cancelled => "cancelled",
    }
}

fn check_run_status_str(
    status: siralos_godot::godot::GodotProjectCheckRunStatus,
) -> &'static str {
    use siralos_godot::godot::GodotProjectCheckRunStatus;
    match status {
        GodotProjectCheckRunStatus::Denied => "denied",
        GodotProjectCheckRunStatus::Conflict => "conflict",
        GodotProjectCheckRunStatus::Cancelled => "cancelled",
        GodotProjectCheckRunStatus::TimedOut => "timed-out",
        GodotProjectCheckRunStatus::Unsupported => "unsupported",
        GodotProjectCheckRunStatus::Unavailable => "unavailable",
        GodotProjectCheckRunStatus::SandboxFailed => "sandbox-failed",
        GodotProjectCheckRunStatus::Failed => "failed",
    }
}

fn diagnostics_state_str(
    state: siralos_godot::godot::GodotDiagnosticsState,
) -> &'static str {
    use siralos_godot::godot::GodotDiagnosticsState;
    match state {
        GodotDiagnosticsState::Untrusted => "untrusted",
        GodotDiagnosticsState::CheckInvalidated => "check-invalidated",
    }
}

fn godot_diagnostics_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::adapters::godot::diagnostics::GodotDiagnosticsService;
    use siralos_godot::godot::{
        GodotCheckPreparationResult, GodotDiagnosticsExecutionContext,
        GodotDiagnosticsRequest, GodotProjectCheckResult,
        PreparedGDScriptCheck,
    };
    let platform =
        input.get("platform").and_then(Value::as_str).unwrap_or("win32");
    let mut service = GodotDiagnosticsService::new(platform);
    match input.get("op").and_then(Value::as_str) {
        Some("support") => {
            let support = service.support();
            Ok(json!({
                "state": "unavailable",
                "reason": support.reason,
                "platform": support.platform,
            }))
        }
        Some("prepare") => {
            let paths = input.get("paths").and_then(|value| {
                value.as_array().map(|entries| {
                    entries
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<String>>()
                })
            });
            let request = GodotDiagnosticsRequest { paths };
            let cancelled = input
                .get("cancelled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match service.prepare(&request, cancelled) {
                Ok(GodotCheckPreparationResult::NotReady {
                    status,
                    message,
                }) => Ok(json!({
                    "status": status.as_str(),
                    "message": message,
                })),
                Ok(GodotCheckPreparationResult::Ready { .. }) => Err(
                    HarnessError::corpus(
                        "diagnostics prepare cannot become ready without an engine"
                            .to_owned(),
                    ),
                ),
                Err(cancelled) => Ok(json!({
                    "status": "cancelled",
                    "message": cancelled.message,
                })),
            }
        }
        Some("execute") => {
            let check = PreparedGDScriptCheck::create(1);
            let approved_digest = input
                .get("approvedDigest")
                .and_then(Value::as_str)
                .unwrap_or("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .to_owned();
            let context = GodotDiagnosticsExecutionContext {
                approved_digest,
                cancelled: false,
            };
            let outcome = service.execute(&check, &context);
            let GodotProjectCheckResult::NotChecked { status, message } =
                outcome
            else {
                return Err(HarnessError::corpus(
                    "diagnostics execute cannot produce checked results without an engine"
                        .to_owned(),
                ));
            };
            Ok(json!({
                "status": check_run_status_str(status),
                "message": message,
            }))
        }
        Some("status") => {
            let status = service.status();
            Ok(json!({"state": diagnostics_state_str(status.state)}))
        }
        _ => unreachable!("godot diagnostics op was validated"),
    }
}

fn godot_lsp_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::adapters::godot::lsp::GodotLspService;
    use siralos_godot::godot::{
        GdScriptNetworkIsolation, GdScriptSessionState,
        GodotCheckPreparationResult,
    };
    let platform =
        input.get("platform").and_then(Value::as_str).unwrap_or("win32");
    let mut service = GodotLspService::new(platform);
    match input.get("op").and_then(Value::as_str) {
        Some("support") => {
            let support = service.support();
            Ok(json!({
                "state": "unavailable",
                "reason": support.reason,
                "platform": support.platform,
            }))
        }
        Some("prepare") => {
            let cancelled = input
                .get("cancelled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match service.prepare(cancelled) {
                Ok(GodotCheckPreparationResult::NotReady { status, message }) => {
                    Ok(json!({
                        "status": status.as_str(),
                        "message": message,
                    }))
                }
                Ok(GodotCheckPreparationResult::Ready { .. }) => Err(
                    HarnessError::corpus(
                        "language-session prepare cannot become ready without an engine"
                            .to_owned(),
                    ),
                ),
                Err(cancelled) => Ok(json!({
                    "status": "cancelled",
                    "message": cancelled.message,
                })),
            }
        }
        Some("status") => {
            let status = service.status();
            Ok(json!({
                "state": match status.state {
                    GdScriptSessionState::Starting => "starting",
                    GdScriptSessionState::Ready => "ready",
                    GdScriptSessionState::Stale => "stale",
                    GdScriptSessionState::Closed => "closed",
                    GdScriptSessionState::Unavailable => "unavailable",
                },
                "openDocumentCount": status.open_document_count,
                "diagnosticCount": status.diagnostic_count,
                "networkIsolation": match status.network_isolation {
                    GdScriptNetworkIsolation::LoopbackOnly => "loopback-only",
                    GdScriptNetworkIsolation::Unverified => "unverified",
                    GdScriptNetworkIsolation::Unavailable => "unavailable",
                },
            }))
        }
        _ => unreachable!("godot lsp op was validated"),
    }
}

fn godot_discovery_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::adapters::godot::profile::engine_profiler::{
        GodotOverrideSource, GodotProfilerInputs, discover, selected_profile,
    };
    use siralos_godot::config::{
        UserGodotConfig, UserGodotEditionHint, UserGodotInstallationConfig,
    };
    use siralos_godot::godot::{
        GodotInstallationSource, GodotSelectionPreference,
    };

    fn source_str(source: GodotInstallationSource) -> &'static str {
        match source {
            GodotInstallationSource::UserConfig => "user-config",
            GodotInstallationSource::Path => "path",
            GodotInstallationSource::CliPath => "cli-path",
            GodotInstallationSource::CliInstallation => "cli-installation",
            GodotInstallationSource::EnvironmentPath => "environment-path",
            GodotInstallationSource::EnvironmentInstallation => {
                "environment-installation"
            }
            GodotInstallationSource::ActiveConfig => "active-config",
        }
    }

    fn overview_record(
        overview: &siralos_godot::godot::GodotInstallationOverview,
    ) -> Value {
        json!({
            "id": overview.installation_id,
            "sourceLabel": overview.source_label,
            "source": source_str(overview.source),
            "invalid": overview.invalid,
            "isDuplicate": overview.is_duplicate,
            "selected": overview.selected,
        })
    }

    fn parse_preference(
        value: Option<&Value>,
    ) -> Result<GodotSelectionPreference, HarnessError> {
        match value {
            None | Some(Value::Null) => Ok(GodotSelectionPreference::Auto),
            Some(Value::String(text)) => match text.as_str() {
                "auto" => Ok(GodotSelectionPreference::Auto),
                "none" => Ok(GodotSelectionPreference::None),
                "config-active" => Ok(GodotSelectionPreference::ConfigActive),
                other => Err(HarnessError::corpus(format!(
                    "scenario input rejected: preference {other} is not auto, none, or config-active"
                ))),
            },
            Some(object) => {
                if let Some(path) = object.get("path").and_then(Value::as_str)
                {
                    return Ok(GodotSelectionPreference::Path(
                        path.to_owned(),
                    ));
                }
                if let Some(id) =
                    object.get("installationId").and_then(Value::as_str)
                {
                    return Ok(GodotSelectionPreference::InstallationId(
                        id.to_owned(),
                    ));
                }
                Err(HarnessError::corpus(
                    "scenario input rejected: preference object must declare path or installationId".to_owned(),
                ))
            }
        }
    }

    let platform =
        input.get("platform").and_then(Value::as_str).unwrap_or("win32");
    let workspace_root = input
        .get("workspaceRoot")
        .and_then(Value::as_str)
        .unwrap_or("/siralos-differential")
        .to_owned();
    let host_path =
        input.get("hostPath").and_then(Value::as_str).map(str::to_owned);
    let host_path_ext =
        input.get("hostPathExt").and_then(Value::as_str).map(str::to_owned);

    let mut installations = BTreeMap::new();
    for entry in input
        .pointer("/config/installations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
        let path =
            entry.get("path").and_then(Value::as_str).unwrap_or_default();
        let edition_hint =
            match entry.get("editionHint").and_then(Value::as_str) {
                Some("dotnet") => UserGodotEditionHint::Dotnet,
                Some("unknown") => UserGodotEditionHint::Unknown,
                _ => UserGodotEditionHint::Standard,
            };
        installations.insert(
            id.to_owned(),
            UserGodotInstallationConfig {
                path: path.to_owned(),
                edition_hint,
            },
        );
    }
    let config = UserGodotConfig {
        active_installation: input
            .pointer("/config/activeInstallation")
            .and_then(Value::as_str)
            .map(str::to_owned),
        installations,
        discover_on_path: input
            .pointer("/config/discoverOnPath")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let inputs = GodotProfilerInputs {
        config,
        preference: parse_preference(input.get("preference"))?,
        override_source: match input
            .get("overrideSource")
            .and_then(Value::as_str)
        {
            Some("cli") => Some(GodotOverrideSource::Cli),
            _ => None,
        },
        workspace_root,
        host_path,
        host_path_ext,
        platform: platform.to_owned(),
    };
    match input.get("op").and_then(Value::as_str) {
        Some("discover") => {
            let result = discover(&inputs);
            match result {
                Ok(result) => Ok(json!({
                    "ok": true,
                    "selected": result.selected.as_ref().map(overview_record),
                    "candidates": result
                        .candidates
                        .iter()
                        .map(overview_record)
                        .collect::<Vec<Value>>(),
                    "configuration": {
                        "activeInstallation":
                            result.configuration.active_installation,
                        "configuredCount": result.configuration.configured_count,
                        "discoverOnPath": result.configuration.discover_on_path,
                        "overrides": result.configuration.overrides,
                    },
                    "rationale": result.rationale,
                    "diagnostics": result
                        .diagnostics
                        .iter()
                        .map(|diagnostic| {
                            json!({
                                "severity": diagnostic.severity.as_str(),
                                "message": diagnostic.message,
                            })
                        })
                        .collect::<Vec<Value>>(),
                })),
                Err(error) => Ok(json!({
                    "ok": false,
                    "error": error.message,
                })),
            }
        }
        Some("select") => match selected_profile(&inputs) {
            Ok(None) => Ok(json!({"ok": true, "selected": false})),
            Ok(Some(_)) => Ok(json!({"ok": true, "selected": true})),
            Err(error) => Ok(json!({
                "ok": false,
                "error": error.message,
            })),
        },
        _ => unreachable!("godot discovery op was validated"),
    }
}

struct ReviewContextSource {
    edges: Vec<siralos_godot::godot::impact::ImpactEdge>,
    signals: std::collections::HashMap<
        String,
        Vec<siralos_godot::godot::impact::ImpactSignalConnection>,
    >,
    autoloads: std::collections::HashMap<String, String>,
    candidate_tests: std::collections::HashMap<String, Vec<String>>,
    revisions: std::collections::HashMap<String, Option<String>>,
    main_scene: Option<String>,
}

impl siralos_godot::godot::impact::ImpactRelationshipSource
    for ReviewContextSource
{
    fn outgoing(
        &self,
        path: &str,
    ) -> Vec<siralos_godot::godot::impact::ImpactEdge> {
        self.edges
            .iter()
            .filter(|edge| edge.from_path == path)
            .cloned()
            .collect()
    }

    fn incoming(
        &self,
        path: &str,
    ) -> Vec<siralos_godot::godot::impact::ImpactEdge> {
        self.edges
            .iter()
            .filter(|edge| edge.to_path == path)
            .cloned()
            .collect()
    }

    fn signal_connections(
        &self,
        path: &str,
    ) -> Vec<siralos_godot::godot::impact::ImpactSignalConnection> {
        self.signals.get(path).cloned().unwrap_or_default()
    }

    fn autoload_name(&self, path: &str) -> Option<String> {
        self.autoloads.get(path).cloned()
    }

    fn main_scene(&self) -> Option<String> {
        self.main_scene.clone()
    }

    fn current_revision(&self, path: &str) -> Option<String> {
        self.revisions.get(path).cloned().unwrap_or(None)
    }

    fn candidate_tests(&self, path: &str) -> Vec<String> {
        self.candidate_tests.get(path).cloned().unwrap_or_default()
    }
}

fn variant_from_json(value: &Value) -> GodotVariantValue {
    let kind = value.get("kind").and_then(Value::as_str).unwrap_or("opaque");
    match kind {
        "null" => GodotVariantValue::Null,
        "boolean" => GodotVariantValue::Boolean(
            value.get("value").and_then(Value::as_bool).unwrap_or(false),
        ),
        "integer" => GodotVariantValue::Integer(
            value.get("value").and_then(Value::as_i64).unwrap_or(0),
        ),
        "float" => GodotVariantValue::Float(
            value.get("value").and_then(Value::as_f64).unwrap_or(0.0),
        ),
        "string" => GodotVariantValue::String(
            value
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        "string_name" => GodotVariantValue::StringName(
            value
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        "node_path" => GodotVariantValue::NodePath(
            value
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        "array" => GodotVariantValue::Array(
            value
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.iter().map(variant_from_json).collect())
                .unwrap_or_default(),
        ),
        "dictionary" => GodotVariantValue::Dictionary(
            value
                .get("entries")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .map(|entry| DictionaryEntry {
                            key: Box::new(variant_from_json(
                                entry.get("key").unwrap_or(&Value::Null),
                            )),
                            value: Box::new(variant_from_json(
                                entry.get("value").unwrap_or(&Value::Null),
                            )),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
        "vector" => GodotVariantValue::Vector {
            type_name: value
                .get("typeName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            components: value
                .get("components")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default(),
        },
        "color" => GodotVariantValue::Color(
            value
                .get("components")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default(),
        ),
        "packed_array" => GodotVariantValue::PackedArray {
            type_name: value
                .get("typeName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            items: value
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.iter().map(variant_from_json).collect())
                .unwrap_or_default(),
        },
        "ext_resource" => GodotVariantValue::ExtResource(
            value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        "sub_resource" => GodotVariantValue::SubResource(
            value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        ),
        "resource" => GodotVariantValue::Resource {
            uid: value.get("uid").and_then(Value::as_str).map(str::to_owned),
            path: value.get("path").and_then(Value::as_str).map(str::to_owned),
            type_name: value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_owned),
        },
        _ => GodotVariantValue::Opaque {
            type_name: value
                .get("typeName")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            raw: GodotRawValue {
                text: value
                    .pointer("/raw/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                truncated: value
                    .pointer("/raw/truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            },
        },
    }
}

fn mutation_property_from_json(value: &Value) -> MutationProperty {
    MutationProperty {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        value: variant_from_json(value.get("value").unwrap_or(&Value::Null)),
    }
}

fn mutation_operation_from_json(
    value: &Value,
) -> Result<MutationOperation, String> {
    let op = value.get("op").and_then(Value::as_str).unwrap_or("");
    let string_field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("missing string field {name}"))
    };
    match op {
        "set_property" => Ok(MutationOperation::SetProperty {
            node_path: value
                .get("nodePath")
                .and_then(Value::as_str)
                .map(str::to_owned),
            property: string_field("property")?,
            value: variant_from_json(
                value.get("value").unwrap_or(&Value::Null),
            ),
        }),
        "remove_property" => Ok(MutationOperation::RemoveProperty {
            node_path: value
                .get("nodePath")
                .and_then(Value::as_str)
                .map(str::to_owned),
            property: string_field("property")?,
        }),
        "add_node" => Ok(MutationOperation::AddNode {
            name: string_field("name")?,
            node_type: string_field("type")?,
            parent_path: value
                .get("parentPath")
                .and_then(Value::as_str)
                .map(str::to_owned),
            properties: value
                .get("properties")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries.iter().map(mutation_property_from_json).collect()
                })
                .unwrap_or_default(),
            groups: value
                .get("groups")
                .and_then(Value::as_array)
                .map(|groups| {
                    groups
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        }),
        "remove_node" => Ok(MutationOperation::RemoveNode {
            node_path: string_field("nodePath")?,
        }),
        "set_script_attachment" => {
            Ok(MutationOperation::SetScriptAttachment {
                node_path: string_field("nodePath")?,
                ext_resource_id: match value.get("extResourceId") {
                    None | Some(Value::Null) => None,
                    Some(id) => {
                        Some(id.as_str().unwrap_or_default().to_owned())
                    }
                },
            })
        }
        "change_resource_reference" => {
            Ok(MutationOperation::ChangeResourceReference {
                resource_id: string_field("resourceId")?,
                new_path: value
                    .get("newPath")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                new_uid: value
                    .get("newUid")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        }
        "add_signal_connection" => {
            Ok(MutationOperation::AddSignalConnection {
                signal: string_field("signal")?,
                from: string_field("from")?,
                to: string_field("to")?,
                method: string_field("method")?,
                flags: value
                    .get("flags")
                    .and_then(Value::as_u64)
                    .and_then(|flags| u32::try_from(flags).ok()),
                binds: value
                    .get("binds")
                    .and_then(Value::as_array)
                    .map(|binds| binds.iter().map(variant_from_json).collect())
                    .unwrap_or_default(),
            })
        }
        "remove_signal_connection" => {
            Ok(MutationOperation::RemoveSignalConnection {
                signal: string_field("signal")?,
                from: string_field("from")?,
                to: string_field("to")?,
                method: string_field("method")?,
            })
        }
        "create_subresource" => Ok(MutationOperation::CreateSubresource {
            id: string_field("id")?,
            resource_type: string_field("type")?,
            properties: value
                .get("properties")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries.iter().map(mutation_property_from_json).collect()
                })
                .unwrap_or_default(),
        }),
        "update_subresource" => Ok(MutationOperation::UpdateSubresource {
            id: string_field("id")?,
            properties: value
                .get("properties")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries.iter().map(mutation_property_from_json).collect()
                })
                .unwrap_or_default(),
        }),
        "remove_subresource" => Ok(MutationOperation::RemoveSubresource {
            id: string_field("id")?,
        }),
        other => Err(format!("unknown mutation operation op {other}")),
    }
}

fn relation_kind_from_str(
    value: &str,
) -> Option<siralos_godot::godot::impact::model::ImpactRelationKind> {
    match value {
        "script_attachment" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::ScriptAttachment),
        "scene_inheritance" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::SceneInheritance),
        "scene_instancing" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::SceneInstancing),
        "resource_dependency" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::ResourceDependency),
        "script_dependency" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::ScriptDependency),
        "signal_connection" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::SignalConnection),
        "autoload_global" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::AutoloadGlobal),
        "test_covers" => Some(siralos_godot::godot::impact::model::ImpactRelationKind::TestCovers),
        _ => None,
    }
}

fn semantic_expectation_to_json(expectation: &SemanticExpectation) -> Value {
    match expectation {
        SemanticExpectation::NodeExists { node_path } => {
            json!({"kind": "node_exists", "nodePath": node_path})
        }
        SemanticExpectation::NodeAbsent { node_path } => {
            json!({"kind": "node_absent", "nodePath": node_path})
        }
        SemanticExpectation::PropertyEquals { node_path, property, value } => {
            json!({
                "kind": "property_equals",
                "nodePath": node_path,
                "property": property,
                "value": siralos_godot::godot::scene_mutation::variant_to_json(value),
            })
        }
        SemanticExpectation::PropertyAbsent { node_path, property } => {
            json!({
                "kind": "property_absent",
                "nodePath": node_path,
                "property": property,
            })
        }
        SemanticExpectation::ConnectionExists { signal, from, to, method } => {
            json!({
                "kind": "connection_exists",
                "signal": signal,
                "from": from,
                "to": to,
                "method": method,
            })
        }
        SemanticExpectation::ConnectionAbsent { signal, from, to, method } => {
            json!({
                "kind": "connection_absent",
                "signal": signal,
                "from": from,
                "to": to,
                "method": method,
            })
        }
        SemanticExpectation::ScriptAttachment {
            node_path,
            ext_resource_id,
        } => json!({
            "kind": "script_attachment",
            "nodePath": node_path,
            "extResourceId": ext_resource_id,
        }),
        SemanticExpectation::SubresourceExists { id } => {
            json!({"kind": "subresource_exists", "id": id})
        }
        SemanticExpectation::SubresourceAbsent { id } => {
            json!({"kind": "subresource_absent", "id": id})
        }
        SemanticExpectation::ResourceReference {
            resource_id,
            new_path,
            new_uid,
        } => {
            let mut object = serde_json::Map::new();
            object.insert("kind".to_owned(), json!("resource_reference"));
            object.insert("resourceId".to_owned(), json!(resource_id));
            if let Some(new_path) = new_path {
                object.insert("newPath".to_owned(), json!(new_path));
            }
            if let Some(new_uid) = new_uid {
                object.insert("newUid".to_owned(), json!(new_uid));
            }
            serde_json::Value::Object(object)
        }
        SemanticExpectation::ResourceType { type_name } => {
            json!({"kind": "resource_type", "type": type_name})
        }
    }
}

fn godot_review_context_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::godot::impact::{AnalyzeImpactInput, analyze_impact};
    let task_contract_revision = input
        .get("taskContractRevision")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            HarnessError::corpus(
                "review-context requires taskContractRevision",
            )
        })?;
    let empty = Vec::new();
    let changed_paths: Vec<String> = input
        .get("changedPaths")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut source = ReviewContextSource {
        edges: Vec::new(),
        signals: std::collections::HashMap::new(),
        autoloads: std::collections::HashMap::new(),
        candidate_tests: std::collections::HashMap::new(),
        revisions: std::collections::HashMap::new(),
        main_scene: None,
    };
    for edge in input.get("edges").and_then(Value::as_array).unwrap_or(&empty)
    {
        let kind = edge
            .get("kind")
            .and_then(Value::as_str)
            .and_then(relation_kind_from_str)
            .ok_or_else(|| HarnessError::corpus("invalid impact edge kind"))?;
        source.edges.push(siralos_godot::godot::impact::ImpactEdge {
            kind,
            from_path: edge
                .get("fromPath")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            to_path: edge
                .get("toPath")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            stale: edge.get("stale").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    for connection in input
        .get("signalConnections")
        .and_then(Value::as_array)
        .unwrap_or(&empty)
    {
        let path = connection
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        source.signals.entry(path).or_default().push(
            siralos_godot::godot::impact::ImpactSignalConnection {
                signal: connection
                    .get("signal")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                source_node: connection
                    .get("sourceNode")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                target_node: connection
                    .get("targetNode")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                target_method: connection
                    .get("targetMethod")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
        );
    }
    for autoload in
        input.get("autoloads").and_then(Value::as_array).unwrap_or(&empty)
    {
        source.autoloads.insert(
            autoload
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            autoload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        );
    }
    for tests in
        input.get("candidateTests").and_then(Value::as_array).unwrap_or(&empty)
    {
        source.candidate_tests.insert(
            tests
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            tests
                .get("tests")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        );
    }
    match input.get("mainScene") {
        Some(Value::Null) | None => {}
        Some(main_scene) => {
            source.main_scene =
                Some(main_scene.as_str().unwrap_or_default().to_owned());
        }
    }
    for revision in
        input.get("revisions").and_then(Value::as_array).unwrap_or(&empty)
    {
        source.revisions.insert(
            revision
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            revision
                .get("revision")
                .and_then(Value::as_str)
                .map(str::to_owned),
        );
    }
    let manifest = analyze_impact(AnalyzeImpactInput {
        task_id: GODOT_REVIEW_CONTEXT_TASK_ID,
        task_contract_revision,
        changed_paths: &changed_paths,
        source: &source,
    })
    .map_err(|error| HarnessError::corpus(error.message))?;

    fn surface_to_json(
        surface: &siralos_godot::godot::impact::ImpactSurface,
    ) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("path".to_owned(), json!(surface.path));
        object.insert("kind".to_owned(), json!(surface.kind.as_str()));
        object.insert(
            "revision".to_owned(),
            surface.revision.as_deref().into_json_value(),
        );
        object.insert(
            "confidence".to_owned(),
            json!(surface.confidence.as_str()),
        );
        object.insert("evidence".to_owned(), json!(surface.evidence));
        if let Some(note) = &surface.note {
            object.insert("note".to_owned(), json!(note));
        }
        serde_json::Value::Object(object)
    }

    fn relation_to_json(
        relation: &siralos_godot::godot::impact::ImpactRelation,
    ) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("kind".to_owned(), json!(relation.kind.as_str()));
        object.insert("sourcePath".to_owned(), json!(relation.source_path));
        object.insert("targetPath".to_owned(), json!(relation.target_path));
        object.insert(
            "sourceRevision".to_owned(),
            relation.source_revision.as_deref().into_json_value(),
        );
        object.insert(
            "targetRevision".to_owned(),
            relation.target_revision.as_deref().into_json_value(),
        );
        object.insert(
            "confidence".to_owned(),
            json!(relation.confidence.as_str()),
        );
        object.insert("evidence".to_owned(), json!(relation.evidence));
        if let Some(note) = &relation.note {
            object.insert("note".to_owned(), json!(note));
        }
        serde_json::Value::Object(object)
    }

    let primary_changes: Vec<Value> =
        manifest.primary_changes.iter().map(surface_to_json).collect();
    let related_surfaces: Vec<Value> =
        manifest.related_surfaces.iter().map(relation_to_json).collect();
    let regression_areas: Vec<Value> = manifest
        .regression_areas
        .iter()
        .map(|area| {
            json!({
                "id": area.id,
                "title": area.title,
                "reason": area.reason,
                "surfaces": area.surfaces,
            })
        })
        .collect();
    let validation: Vec<Value> = manifest
        .validation
        .iter()
        .map(|recommendation| {
            json!({
                "kind": recommendation.kind.as_str(),
                "priority": recommendation.priority.as_str(),
                "rationale": recommendation.rationale,
                "surfaces": recommendation.surfaces,
            })
        })
        .collect();
    let diagnostics: Vec<Value> = manifest
        .diagnostics
        .iter()
        .map(|diagnostic| json!({"code": diagnostic.code, "message": diagnostic.message}))
        .collect();
    Ok(json!({
        "taskId": manifest.task_id,
        "taskContractRevision": manifest.task_contract_revision,
        "primaryChanges": primary_changes,
        "relatedSurfaces": related_surfaces,
        "regressionAreas": regression_areas,
        "validation": validation,
        "evidence": manifest.evidence,
        "completeness": manifest.completeness.as_str(),
        "diagnostics": diagnostics,
    }))
}

trait OptionStrJsonExt {
    fn into_json_value(self) -> Value;
}

impl OptionStrJsonExt for Option<&str> {
    fn into_json_value(self) -> Value {
        match self {
            Some(text) => json!(text),
            None => Value::Null,
        }
    }
}

fn godot_mutation_prepare_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_godot::godot::scene_mutation::{
        CreatePreparedGodotMutationInput, GodotMutationPreview, MutationKind,
        create_prepared_godot_mutation, expected_semantic_effect,
        validate_mutation_operations,
    };
    let operations_json =
        input.get("operations").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("mutation-prepare requires operations"),
        )?;
    let mut parsed = Vec::with_capacity(operations_json.len());
    for operation in operations_json {
        parsed.push(
            mutation_operation_from_json(operation)
                .map_err(HarnessError::corpus)?,
        );
    }
    let validated = match validate_mutation_operations(&parsed) {
        Ok(validated) => validated,
        Err(mutation_error) => {
            return Ok(json!({"ok": false, "error": mutation_error.message}));
        }
    };
    let expectations = expected_semantic_effect(&validated);
    let kind = match input.get("kind").and_then(Value::as_str) {
        Some("resource") => MutationKind::Resource,
        _ => MutationKind::Scene,
    };
    let created =
        create_prepared_godot_mutation(CreatePreparedGodotMutationInput {
            target_path: input
                .get("targetPath")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            source_revision: input
                .get("sourceRevision")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            source_sha256: input
                .get("sourceSha256")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            kind,
            operations: validated,
            preview: GodotMutationPreview {
                structural_summary: input
                    .get("previewSummary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                diff: input
                    .get("previewDiff")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
            serialized_after: input
                .get("serializedAfter")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            added_lines: input
                .get("addedLines")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            removed_lines: input
                .get("removedLines")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        });
    let created = match created {
        Ok(created) => created,
        Err(mutation_error) => {
            return Ok(json!({"ok": false, "error": mutation_error.message}));
        }
    };
    let operations_json: Vec<Value> = created
        .operations
        .iter()
        .map(MutationOperation::to_canonical_json)
        .collect();
    let expectations_json: Vec<Value> =
        expectations.iter().map(semantic_expectation_to_json).collect();
    Ok(json!({
        "ok": true,
        "fingerprint": created.fingerprint,
        "operations": operations_json,
        "expectedSemanticEffect": expectations_json,
        "structuralSummary": created.preview.structural_summary,
        "diff": created.preview.diff,
    }))
}

fn godot_develop_plan_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_godot::godot::development::{
        DevelopmentSurfaceInput, DevelopmentSurfaceTouchpoint,
        DevelopmentTouchpointStatus, ProjectSurfaces, UnifiedOrderTarget,
        classify_development_surface, derive_unified_apply_order,
        derive_unified_order_edges,
    };
    let request =
        input.get("request").and_then(Value::as_str).unwrap_or_default();
    let mut touchpoints = Vec::new();
    for touchpoint in input
        .get("touchpoints")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
    {
        let status = match touchpoint.get("status").and_then(Value::as_str) {
            Some("verified") => DevelopmentTouchpointStatus::Verified,
            _ => DevelopmentTouchpointStatus::Candidate,
        };
        touchpoints.push(DevelopmentSurfaceTouchpoint {
            path: touchpoint
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            status,
        });
    }
    let project_surfaces = match input.get("projectSurfaces") {
        Some(Value::Null) | None => None,
        Some(surfaces) => Some(ProjectSurfaces {
            has_scenes: surfaces
                .get("hasScenes")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_resources: surfaces
                .get("hasResources")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            has_scripts: surfaces
                .get("hasScripts")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
    };
    let targets: Vec<UnifiedOrderTarget> = input
        .get("targets")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .iter()
        .map(|target| UnifiedOrderTarget {
            target_id: target
                .get("targetId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            path: target
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            references: target
                .get("references")
                .and_then(Value::as_array)
                .map(|references| {
                    references
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();

    let decision = classify_development_surface(DevelopmentSurfaceInput {
        request,
        touchpoints: &touchpoints,
        project_surfaces,
    });
    let (edges, unresolved_references) = derive_unified_order_edges(&targets);
    let order = derive_unified_apply_order(&targets, &edges);
    let edges_json: Vec<Value> = edges
        .iter()
        .map(|edge| json!({"before": edge.before, "after": edge.after}))
        .collect();
    let unresolved_json: Vec<Value> = unresolved_references
        .iter()
        .map(|reference| {
            json!({"targetId": reference.target_id, "path": reference.path})
        })
        .collect();
    let mut object = serde_json::Map::new();
    object.insert(
        "surface".to_owned(),
        json!({
            "kind": decision.kind.as_str(),
            "rationale": decision.rationale,
            "evidence": decision.evidence,
        }),
    );
    object.insert("edges".to_owned(), json!(edges_json));
    object.insert("unresolvedReferences".to_owned(), json!(unresolved_json));
    match order {
        Ok(order) => {
            object.insert(
                "applyOrder".to_owned(),
                json!({"order": order.order, "rationale": order.rationale}),
            );
        }
        Err(development_error) => {
            object.insert(
                "applyOrderError".to_owned(),
                json!(development_error.message),
            );
        }
    }
    Ok(serde_json::Value::Object(object))
}

// ---------------------------------------------------------------------------
// Stage 3R R10a subjects: content identity + determinism.
// ---------------------------------------------------------------------------
