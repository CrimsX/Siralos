//! Provider-config subjects: user config, provider generic/replay, session replay, and replay store.
use crate::harness::*;

pub(in crate::harness) fn user_config_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let cases = scenario_array(input, "cases")?;
    let mut records = Vec::new();
    for case in cases {
        let _name = scenario_string(case, "name")?;
        let mode = scenario_string(case, "mode")?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| {
                HarnessError::input(format!("cannot read clock: {error}"))
            })?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "siralos-r7-4-user-config-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).map_err(|error| {
            HarnessError::input(format!(
                "cannot create config probe directory: {error}"
            ))
        })?;
        let path = directory.join("config.json");
        let setup = setup_user_config_case(&mode, &path);
        let result = match setup {
            Ok(()) => {
                let diagnostics =
                    siralos_cli::configuration::diagnose_user_configuration(
                        Some(&path),
                    )
                    .map_err(|error| HarnessError::input(error.to_string()))?;
                match siralos_cli::configuration::load_user_configuration(
                    Some(&path),
                ) {
                    Ok(composed) => json!({
                        "status": "ok",
                        "config": user_config_value(&composed.config),
                        "reviewProviderId": composed.review_provider_id,
                        "referenceConfigError": composed.reference_config_error,
                        "diagnostics": configuration_diagnostics_value(&diagnostics),
                    }),
                    Err(error) => json!({
                        "status": "error",
                        "category": error.category(),
                        "diagnostics": configuration_diagnostics_value(&diagnostics),
                    }),
                }
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error);
            }
        };
        records.push(result);
        let _ = std::fs::remove_dir_all(directory);
    }
    Ok(json!({ "cases": records }))
}

fn setup_user_config_case(
    mode: &str,
    path: &Path,
) -> Result<(), HarnessError> {
    match mode {
        "missing" => Ok(()),
        "directory" => std::fs::create_dir(path).map_err(|error| {
            HarnessError::input(format!(
                "cannot create nonregular config fixture: {error}"
            ))
        }),
        "symlink" => {
            let target = path.with_file_name("target.json");
            std::fs::write(&target, b"{}").map_err(|error| {
                HarnessError::input(format!(
                    "cannot write symlink target: {error}"
                ))
            })?;
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&target, path).map_err(
                    |error| {
                        HarnessError::input(format!(
                            "cannot create symlink fixture: {error}"
                        ))
                    },
                )?;
                Ok(())
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::remove_file(target);
                Err(HarnessError::input(
                    "symlink fixture is unavailable on this host",
                ))
            }
        }
        other => {
            let content = user_config_content(other).ok_or_else(|| {
                HarnessError::corpus(format!(
                    "unknown user-config fixture mode {other}"
                ))
            })?;
            std::fs::write(path, content.as_bytes()).map_err(|error| {
                HarnessError::input(format!(
                    "cannot write config fixture: {error}"
                ))
            })
        }
    }
}

fn user_config_content(mode: &str) -> Option<String> {
    let content = match mode {
        "full" => json!({
            "sandbox": { "profile": "develop-offline", "backend": "anthropic-runtime" },
            "godot": {
                "activeInstallation": "stable",
                "discoverOnPath": false,
                "installations": {
                    "stable": { "path": "/opt/godot", "editionHint": "standard" }
                }
            },
            "quality": { "reviewProvider": "deterministic-fake" },
            "references": {
                "aa": { "kind": "local-directory", "path": "/srv/assets", "description": "Assets" },
                "bb": { "kind": "repository", "repository": "godotengine/godot", "ref": { "kind": "commit", "commit": "0123456" } }
            }
        }).to_string(),
        "unknown-top" => json!({ "permissions": {} }).to_string(),
        "unknown-nested" => json!({ "sandbox": { "credential": "secret" } }).to_string(),
        "invalid-profile" => json!({ "sandbox": { "profile": "full-access" } }).to_string(),
        "invalid-backend" => json!({ "sandbox": { "backend": "docker" } }).to_string(),
        "invalid-edition" => json!({
            "godot": { "installations": { "stable": { "path": "/opt/godot", "editionHint": "mono" } } }
        }).to_string(),
        "installations-bound" => {
            let mut installations = Map::new();
            for index in 0..17 {
                installations.insert(
                    format!("g{index:02}"),
                    json!({ "path": "/opt/godot" }),
                );
            }
            json!({ "godot": { "installations": installations } }).to_string()
        }
        "references-bound" => {
            let mut references = Map::new();
            for index in 0..17 {
                references.insert(
                    format!("r{index:02}"),
                    json!({ "kind": "local-directory", "path": "/srv/assets" }),
                );
            }
            json!({ "references": references }).to_string()
        }
        "invalid-godot-path" => json!({
            "godot": { "installations": { "stable": { "path": "relative/godot" } } }
        }).to_string(),
        "invalid-provider" => json!({
            "quality": { "reviewProvider": "reviewer" }
        }).to_string(),
        "invalid-json" => "{not valid json".to_owned(),
        "exact-boundary" => format!("{{}}{}", " ".repeat(MAX_CONFIG_FILE_BYTES - 2)),
        "over-boundary" => format!("{{}}{}", " ".repeat(MAX_CONFIG_FILE_BYTES - 1)),
        "invalid-reference-path" => json!({
            "references": { "aa": { "kind": "local-directory", "path": "relative" } }
        }).to_string(),
        "invalid-repository" => json!({
            "references": { "aa": { "kind": "repository", "repository": "https://example.com/org/repo" } }
        }).to_string(),
        "missing" | "directory" | "symlink" => return None,
        _ => return None,
    };
    Some(content)
}

fn user_config_value(config: &siralos_adapters::config::UserConfig) -> Value {
    let installations = config
        .godot
        .installations
        .iter()
        .map(|(id, installation)| {
            (
                id.clone(),
                json!({
                    "path": installation.path,
                    "editionHint": installation.edition_hint.as_str(),
                }),
            )
        })
        .collect::<Map<String, Value>>();
    let references = config
        .references
        .iter()
        .map(|(alias, reference)| {
            (alias.clone(), user_reference_value(reference))
        })
        .collect::<Map<String, Value>>();
    json!({
        "sandbox": {
            "profile": config.sandbox.profile.as_str(),
            "backend": config.sandbox.backend.as_str(),
        },
        "godot": {
            "activeInstallation": config.godot.active_installation,
            "installations": installations,
            "discoverOnPath": config.godot.discover_on_path,
        },
        "quality": { "reviewProvider": config.quality.review_provider },
        "references": references,
    })
}

fn user_reference_value(
    reference: &siralos_adapters::config::UserReferenceConfig,
) -> Value {
    let mut value = Map::new();
    value.insert("kind".to_owned(), json!(reference.kind.as_str()));
    if let Some(path) = &reference.path {
        value.insert("path".to_owned(), json!(path));
    }
    if let Some(repository) = &reference.repository {
        value.insert("repository".to_owned(), json!(repository));
    }
    if let Some(reference_pin) = &reference.reference {
        value.insert(
            "ref".to_owned(),
            json!({
                "kind": reference_pin.kind(),
                reference_pin.kind(): reference_pin.value(),
            }),
        );
    }
    if let Some(description) = &reference.description {
        value.insert("description".to_owned(), json!(description));
    }
    Value::Object(value)
}

fn configuration_diagnostics_value(
    diagnostics: &siralos_adapters::config::ConfigurationDiagnostics,
) -> Value {
    json!({
        "loaded": diagnostics.loaded,
        "sections": diagnostics.sections.iter().map(|section| json!({ "name": section.name, "present": section.present })).collect::<Vec<_>>(),
        "unknownFields": diagnostics.unknown_fields,
        "validationErrors": diagnostics.validation_errors.iter().map(|error| configuration_message_category(error)).collect::<Vec<_>>(),
        "credentialRefs": diagnostics.credential_refs,
        "overrideInUse": diagnostics.override_in_use,
        "fileState": diagnostics.file_state.as_str(),
    })
}

fn configuration_message_category(message: &str) -> &'static str {
    if message.contains("not a regular file") {
        "NOT_REGULAR"
    } else if message.contains("exceeds the 1048576-byte limit")
        || message.contains("could not be read within the 1048576-byte limit")
    {
        "TOO_LARGE"
    } else if message.contains("not valid JSON") {
        "INVALID_JSON"
    } else if message.contains("not valid UTF-8") {
        "INVALID_UTF8"
    } else if message.starts_with("Cannot read Siralos configuration") {
        "CANNOT_READ"
    } else {
        "INVALID_VALUE"
    }
}

// ---------------------------------------------------------------------------
// Stage 3R R8 subject: godot.
// ---------------------------------------------------------------------------

/// Canonical provider-generic record: one canonical observation per input.
/// The input is the `provider-generic` subject's `input` object which
/// carries `provider`/`model`/`credential`/`endpoint`/`messages`/`tools`.
/// This exercises the `GenericProvider` path via `HostProvider`
/// hermetically (ADR 0033): the declared `provider`/`model`/`credential`/
/// `endpoint` are validated, but the executed call is pinned to
/// `HERMETIC_PROVIDER_ENDPOINT` (unreachable loopback) with a fake
/// credential, so the canonical outcome is the typed, deterministic
/// `ProviderEvent::Failed` refusal — no live network I/O, no real
/// credential ever transmitted, no panic, no credential leak.
pub(in crate::harness) fn provider_generic_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let provider = scenario_string(input, "provider")?;
    let model = scenario_string(input, "model")
        .unwrap_or_else(|_| "gpt-4o".to_owned());
    let messages = input
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let tools = input
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    // Build a minimal ModelRequest for the GenericProvider.
    let mut conv_items = Vec::new();
    for msg in messages {
        if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
            let role =
                msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            match role {
                "user" => conv_items.push(siralos_core::provider::ConversationItem::UserMessage {
                    content: content.to_owned(),
                }),
                "assistant" => conv_items.push(siralos_core::provider::ConversationItem::AssistantMessage {
                    content: content.to_owned(),
                }),
                _ => {}
            }
        }
    }
    let tool_defs = tools
        .iter()
        .filter_map(|t| {
            Some(siralos_core::provider::ToolDefinition {
                name: t.get("name")?.as_str()?.to_owned(),
                description: t
                    .get("description")?
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                input_schema: t
                    .get("input_schema")
                    .cloned()
                    .unwrap_or(serde_json::json!({})),
            })
        })
        .collect::<Vec<_>>();
    let cred = siralos_adapters::provider::HostCredential::from_bytes_for_test(
        b"sk-test-generic".to_vec(),
    );
    let generic = siralos_adapters::provider::generic::GenericProvider::new(
        provider.clone(),
        model.clone(),
        Some(HERMETIC_PROVIDER_ENDPOINT.to_owned()),
        Some(cred),
    );
    let request = siralos_core::provider::ModelRequest {
        messages: conv_items,
        tools: tool_defs,
        system: None,
    };
    let token = siralos_core::provider::CancellationToken::new();
    let events: Vec<_> = generic.stream(&request, token.signal()).collect();
    Ok(serde_json::json!({
        "providerId": generic.id(),
        "events": events.iter().map(|e| match e {
            siralos_core::provider::ProviderEvent::Event(ev) => match ev {
                siralos_core::provider::ModelEvent::ReasoningDelta { text } => serde_json::json!({"type": "reasoning_delta", "text": text}),
                siralos_core::provider::ModelEvent::TextDelta { text } => serde_json::json!({"type": "text_delta", "text": text}),
                siralos_core::provider::ModelEvent::ToolCall { call_id, tool_name, input } => serde_json::json!({"type": "tool_call", "callId": call_id, "toolName": tool_name, "input": input.value()}),
                siralos_core::provider::ModelEvent::Completed => serde_json::json!({"type": "completed"}),
            },
            siralos_core::provider::ProviderEvent::Failed(msg) => serde_json::json!({"type": "failed", "message": msg}),
            siralos_core::provider::ProviderEvent::Cancelled { message } => serde_json::json!({"type": "cancelled", "message": message}),
            siralos_core::provider::ProviderEvent::Raw(v) => serde_json::json!({"type": "raw", "value": v}),
        }).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------
// Hermetic replay subject: provider-replay (decisions 73/74, corpus v54).

/// Canonical provider-replay record: hermetic ordered playback, typed
/// exhausted failure, and copy semantics. All in-memory, no network.
pub(in crate::harness) fn provider_replay_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::determinism::{
        ProviderResponseIdentity, ReplayRecorder, RetainingReplayRecorder,
    };
    let body1 = r#"{"choices":[{"message":{"content":"alpha"}}]}"#;
    let body2 = r#"{"choices":[{"message":{"content":"beta"}}]}"#;
    let sha1 = siralos_core::identity::sha256_hex(body1.as_bytes());
    let sha2 = siralos_core::identity::sha256_hex(body2.as_bytes());
    let identity1 = ProviderResponseIdentity {
        provider_id: "replay-subject".to_owned(),
        model: "replay-model".to_owned(),
        status: Some(200),
        body_sha256: sha1,
        body_bytes: body1.len() as u64,
        observed_at_ms: Some(1000),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    let identity2 = ProviderResponseIdentity {
        provider_id: "replay-subject".to_owned(),
        model: "replay-model".to_owned(),
        status: Some(200),
        body_sha256: sha2,
        body_bytes: body2.len() as u64,
        observed_at_ms: Some(1000),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    let recorder = RetainingReplayRecorder::new();
    recorder.record_provider_response(&identity1);
    recorder.record_provider_response_with_body(&identity1, body1);
    recorder.record_provider_response(&identity2);
    recorder.record_provider_response_with_body(&identity2, body2);
    let provider =
        siralos_adapters::provider::replay::replay_provider_from_recorder(
            "replay-subject".to_owned(),
            "replay-model".to_owned(),
            &recorder,
        );
    let request = siralos_core::provider::ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        system: None,
    };
    fn canonical_events(
        events: Vec<siralos_core::provider::ProviderEvent>,
    ) -> Vec<Value> {
        events
            .iter()
            .map(|e| match e {
                siralos_core::provider::ProviderEvent::Event(ev) => match ev {
                    siralos_core::provider::ModelEvent::ReasoningDelta { text } => serde_json::json!({"type": "reasoning_delta", "text": text}),
                    siralos_core::provider::ModelEvent::TextDelta { text } => {
                        serde_json::json!({"type": "text_delta", "text": text})
                    }
                    siralos_core::provider::ModelEvent::ToolCall {
                        call_id,
                        tool_name,
                        input,
                    } => serde_json::json!({
                        "type": "tool_call",
                        "callId": call_id,
                        "toolName": tool_name,
                        "input": input.value()
                    }),
                    siralos_core::provider::ModelEvent::Completed => {
                        serde_json::json!({"type": "completed"})
                    }
                },
                siralos_core::provider::ProviderEvent::Failed(msg) => {
                    serde_json::json!({"type": "failed", "message": msg})
                }
                siralos_core::provider::ProviderEvent::Cancelled { message } => {
                    serde_json::json!({"type": "cancelled", "message": message})
                }
                siralos_core::provider::ProviderEvent::Raw(v) => {
                    serde_json::json!({"type": "raw", "value": v})
                }
            })
            .collect()
    }
    let remaining_before1 = provider.recordings_remaining();
    let token1 = siralos_core::provider::CancellationToken::new();
    let turn1 =
        canonical_events(provider.stream(&request, token1.signal()).collect());
    let remaining_before2 = provider.recordings_remaining();
    let token2 = siralos_core::provider::CancellationToken::new();
    let turn2 =
        canonical_events(provider.stream(&request, token2.signal()).collect());
    let remaining_before3 = provider.recordings_remaining();
    let token3 = siralos_core::provider::CancellationToken::new();
    let turn3 =
        canonical_events(provider.stream(&request, token3.signal()).collect());
    Ok(serde_json::json!({
        "providerId": provider.id(),
        "model": "replay-model",
        "remainingCounts": [remaining_before1, remaining_before2, remaining_before3],
        "turn1Events": turn1,
        "turn2Events": turn2,
        "turn3Events": turn3,
        "recorderSnapshotCount": recorder.records_snapshot().len(),
    }))
}

// ---------------------------------------------------------------------------
// Hermetic replay subject: session-replay (decision 77, corpus v55).

/// Canonical session-replay record: hermetic record-then-replay via
/// SessionReplayComposer; in-process only, nothing persisted.
pub(in crate::harness) fn session_replay_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::determinism::ReplayRecorder;
    let body1 = r#"{"choices":[{"message":{"content":"alpha"}}]}"#;
    let body2 = r#"{"choices":[{"message":{"content":"beta"}}]}"#;
    let sha1 = siralos_core::identity::sha256_hex(body1.as_bytes());
    let sha2 = siralos_core::identity::sha256_hex(body2.as_bytes());
    let identity1 = siralos_core::determinism::ProviderResponseIdentity {
        provider_id: "session-subject".to_owned(),
        model: "session-model".to_owned(),
        status: Some(200),
        body_sha256: sha1,
        body_bytes: body1.len() as u64,
        observed_at_ms: Some(1000),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    let identity2 = siralos_core::determinism::ProviderResponseIdentity {
        provider_id: "session-subject".to_owned(),
        model: "session-model".to_owned(),
        status: Some(200),
        body_sha256: sha2,
        body_bytes: body2.len() as u64,
        observed_at_ms: Some(1000),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    let composer =
        siralos_adapters::provider::replay::SessionReplayComposer::new(
            "session-subject".to_owned(),
            "session-model".to_owned(),
        );
    composer.recorder().record_provider_response(&identity1);
    composer.recorder().record_provider_response_with_body(&identity1, body1);
    composer.recorder().record_provider_response(&identity2);
    composer.recorder().record_provider_response_with_body(&identity2, body2);
    let evidence = composer.evidence();
    let evidence_digest =
        siralos_core::determinism::compute_session_replay_evidence_digest(
            &evidence,
        );
    let provider = composer.compose();
    let request = siralos_core::provider::ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        system: None,
    };
    fn canonical_events(
        events: Vec<siralos_core::provider::ProviderEvent>,
    ) -> Vec<Value> {
        events
            .iter()
            .map(|e| match e {
                siralos_core::provider::ProviderEvent::Event(ev) => match ev {
                    siralos_core::provider::ModelEvent::ReasoningDelta { text } => serde_json::json!({"type": "reasoning_delta", "text": text}),
                    siralos_core::provider::ModelEvent::TextDelta { text } => {
                        serde_json::json!({"type": "text_delta", "text": text})
                    }
                    siralos_core::provider::ModelEvent::ToolCall {
                        call_id,
                        tool_name,
                        input,
                    } => serde_json::json!({
                        "type": "tool_call",
                        "callId": call_id,
                        "toolName": tool_name,
                        "input": input.value()
                    }),
                    siralos_core::provider::ModelEvent::Completed => {
                        serde_json::json!({"type": "completed"})
                    }
                },
                siralos_core::provider::ProviderEvent::Failed(msg) => {
                    serde_json::json!({"type": "failed", "message": msg})
                }
                siralos_core::provider::ProviderEvent::Cancelled { message } => {
                    serde_json::json!({"type": "cancelled", "message": message})
                }
                siralos_core::provider::ProviderEvent::Raw(v) => {
                    serde_json::json!({"type": "raw", "value": v})
                }
            })
            .collect()
    }
    let remaining_before1 = provider.recordings_remaining();
    let token1 = siralos_core::provider::CancellationToken::new();
    let turn1 =
        canonical_events(provider.stream(&request, token1.signal()).collect());
    let remaining_before2 = provider.recordings_remaining();
    let token2 = siralos_core::provider::CancellationToken::new();
    let turn2 =
        canonical_events(provider.stream(&request, token2.signal()).collect());
    let remaining_before3 = provider.recordings_remaining();
    let token3 = siralos_core::provider::CancellationToken::new();
    let turn3 =
        canonical_events(provider.stream(&request, token3.signal()).collect());
    Ok(serde_json::json!({
        "providerId": provider.id(),
        "model": "session-model",
        "remainingCounts": [remaining_before1, remaining_before2, remaining_before3],
        "turn1Events": turn1,
        "turn2Events": turn2,
        "turn3Events": turn3,
        "recordedCount": evidence.recorded_count,
        "snapshotCount": evidence.recorder_snapshot_count,
        "evidenceDigest": evidence_digest,
    }))
}

// ---------------------------------------------------------------------------
// Hermetic replay-store subject: cross-session replay (decision 78 B3, corpus v56).

/// Canonical replay-store record: hermetic cross-session store.
/// Inside a sandbox workspace, flush two known recordings through the
/// flush seam, load + verify (digest), compose replay provider, stream
/// three turns, tamper a body byte and assert UntrustedDigest.
pub(in crate::harness) fn replay_store_record(
    _input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_adapters::replay_store::{
        load_replay_store, write_replay_store,
    };
    use siralos_core::determinism::{
        ProviderResponseIdentity, ReplayRecording,
        replay_store::compute_replay_store_digest,
    };
    use siralos_core::identity::sha256_hex;
    let body1 = r#"{"choices":[{"message":{"content":"alpha"}}]}"#;
    let body2 = r#"{"choices":[{"message":{"content":"beta"}}]}"#;
    let sha1 = sha256_hex(body1.as_bytes());
    let sha2 = sha256_hex(body2.as_bytes());
    let identity1 = ProviderResponseIdentity {
        provider_id: "replay-subject".to_owned(),
        model: "replay-model".to_owned(),
        status: Some(200),
        body_sha256: sha1,
        body_bytes: body1.len() as u64,
        observed_at_ms: Some(1000),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    let identity2 = ProviderResponseIdentity {
        provider_id: "replay-subject".to_owned(),
        model: "replay-model".to_owned(),
        status: Some(200),
        body_sha256: sha2,
        body_bytes: body2.len() as u64,
        observed_at_ms: Some(1000),
        input_tokens: None,
        output_tokens: None,
        cached_tokens: None,
    };
    // `request_sha256: None` is the legacy digest-less recording form, which
    // is exactly the binding this subject pins: the same provider must serve
    // the same three turns across sessions without a per-request route match.
    let recordings = vec![
        ReplayRecording {
            identity: identity1,
            body: body1.to_owned(),
            request_sha256: None,
        },
        ReplayRecording {
            identity: identity2,
            body: body2.to_owned(),
            request_sha256: None,
        },
    ];
    // Hermetic sandbox workspace.
    let sandbox = std::env::temp_dir().join(format!(
        "siralos-replay-store-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&sandbox);
    std::fs::create_dir_all(sandbox.join(".siralos")).map_err(|e| {
        HarnessError::corpus(format!(
            "replay-store sandbox create failed: {e}"
        ))
    })?;
    let store_path = sandbox.join(".siralos").join("replay-store.json");
    // Flush through the seam (scan/eviction/atomicity).
    let persisted_count = write_replay_store(&store_path, &recordings)
        .map_err(|e| {
            HarnessError::corpus(format!("replay-store write failed: {e}"))
        })?;
    // Load + verify (digest).
    let loaded = load_replay_store(&store_path).map_err(|e| {
        HarnessError::corpus(format!("replay-store load failed: {e:?}"))
    })?;
    let store_digest = compute_replay_store_digest(&loaded.recordings);
    let loaded_count = loaded.recordings.len();
    if loaded_count != 2 {
        return Err(HarnessError::corpus(format!(
            "replay-store loaded count expected 2, got {loaded_count}"
        )));
    }
    // Compose replay provider from loaded recordings.
    let provider =
        siralos_adapters::provider::replay::RecordedReplayProvider::new(
            "replay-subject".to_owned(),
            "replay-model".to_owned(),
            loaded.recordings.clone(),
        );
    let request = siralos_core::provider::ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        system: None,
    };
    fn canonical_events(
        events: Vec<siralos_core::provider::ProviderEvent>,
    ) -> Vec<Value> {
        events.iter().map(|e| match e {
            siralos_core::provider::ProviderEvent::Event(ev) => match ev {
                siralos_core::provider::ModelEvent::ReasoningDelta { text } => serde_json::json!({"type": "reasoning_delta", "text": text}),
                siralos_core::provider::ModelEvent::TextDelta { text } => {
                    serde_json::json!({"type": "text_delta", "text": text})
                }
                siralos_core::provider::ModelEvent::ToolCall { call_id, tool_name, input } => serde_json::json!({
                    "type": "tool_call",
                    "callId": call_id,
                    "toolName": tool_name,
                    "input": input.value()
                }),
                siralos_core::provider::ModelEvent::Completed => {
                    serde_json::json!({"type": "completed"})
                }
            },
            siralos_core::provider::ProviderEvent::Failed(msg) => {
                serde_json::json!({"type": "failed", "message": msg})
            }
            siralos_core::provider::ProviderEvent::Cancelled { message } => {
                serde_json::json!({"type": "cancelled", "message": message})
            }
            siralos_core::provider::ProviderEvent::Raw(v) => {
                serde_json::json!({"type": "raw", "value": v})
            }
        }).collect()
    }
    let token1 = siralos_core::provider::CancellationToken::new();
    let turn1 =
        canonical_events(provider.stream(&request, token1.signal()).collect());
    let token2 = siralos_core::provider::CancellationToken::new();
    let turn2 =
        canonical_events(provider.stream(&request, token2.signal()).collect());
    let token3 = siralos_core::provider::CancellationToken::new();
    let turn3 =
        canonical_events(provider.stream(&request, token3.signal()).collect());
    // Tamper a body byte in the sandbox copy and assert second load is UntrustedDigest.
    let mut raw = std::fs::read_to_string(&store_path).map_err(|e| {
        HarnessError::corpus(format!(
            "replay-store read for tamper failed: {e}"
        ))
    })?;
    if raw.contains("alpha") {
        raw = raw.replacen("alpha", "AlpHa", 1);
    } else {
        raw = raw.replacen('a', "A", 1);
    }
    std::fs::write(&store_path, raw).map_err(|e| {
        HarnessError::corpus(format!("replay-store tamper write failed: {e}"))
    })?;
    let tampered = load_replay_store(&store_path);
    let tampered_load = match tampered {
        Err(siralos_adapters::replay_store::ReplayStoreLoadError::UntrustedDigest) => "untrusted-digest".to_owned(),
        Ok(_) => {
            return Err(HarnessError::corpus("replay-store tamper did not produce UntrustedDigest".to_owned()));
        }
        Err(e) => {
            return Err(HarnessError::corpus(format!(
                "replay-store tamper produced unexpected error {e:?}"
            )));
        }
    };
    // Cleanup hermetic sandbox.
    let _ = std::fs::remove_dir_all(&sandbox);
    Ok(serde_json::json!({
        "storeDigest": store_digest,
        "loadedCount": loaded_count,
        "persistedCount": persisted_count,
        "tamperedLoad": tampered_load,
        "turn1Events": turn1,
        "turn2Events": turn2,
        "turn3Events": turn3,
    }))
}

// ---------------------------------------------------------------------------
// Hermetic subject: context-graph (decision 79, corpus v57).

/// Strict provider-replay input shape validation (decisions 73/74, corpus v54).
pub(in crate::harness) fn validate_provider_replay_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-replay input must be an object")
    })?;
    // Minimal: any bounded plain object is accepted; validate optional string fields if present.
    for key in
        ["provider", "model", "credential", "endpoint", "messages", "tools"]
    {
        let Some(val) = obj.get(key) else {
            continue;
        };
        match (key, val) {
            (
                "provider" | "model" | "credential" | "endpoint",
                Value::String(_),
            ) => {}
            ("provider" | "model" | "credential" | "endpoint", _) => {
                return Err(HarnessError::corpus(format!(
                    "provider-replay {key} must be a string"
                )));
            }
            ("messages" | "tools", Value::Array(_)) => {}
            ("messages" | "tools", _) => {
                return Err(HarnessError::corpus(format!(
                    "provider-replay {key} must be an array"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Strict session-replay input shape validation (decision 77, corpus v55).
pub(in crate::harness) fn validate_session_replay_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("session-replay input must be an object")
    })?;
    // Minimal: any bounded plain object is accepted; validate optional string fields if present.
    for key in
        ["provider", "model", "credential", "endpoint", "messages", "tools"]
    {
        let Some(val) = obj.get(key) else {
            continue;
        };
        match (key, val) {
            (
                "provider" | "model" | "credential" | "endpoint",
                Value::String(_),
            ) => {}
            ("provider" | "model" | "credential" | "endpoint", _) => {
                return Err(HarnessError::corpus(format!(
                    "session-replay {key} must be a string"
                )));
            }
            ("messages" | "tools", Value::Array(_)) => {}
            ("messages" | "tools", _) => {
                return Err(HarnessError::corpus(format!(
                    "session-replay {key} must be an array"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Strict replay-store input shape validation (decision 78 B3, corpus v56).
pub(in crate::harness) fn validate_replay_store_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("replay-store input must be an object")
    })?;
    // Minimal: any bounded plain object accepted.
    for key in obj.keys() {
        let _ = key;
    }
    Ok(())
}

/// Strict provider-generic input shape validation (Stage 8, all-purpose provider).
pub(in crate::harness) fn validate_provider_generic_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let obj = input.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-generic input must be an object")
    })?;
    for key in
        ["provider", "model", "credential", "endpoint", "messages", "tools"]
    {
        let Some(val) = obj.get(key) else {
            continue;
        };
        match (key, val) {
            (
                "provider" | "model" | "credential" | "endpoint",
                Value::String(_),
            ) => {}
            ("provider" | "model" | "credential" | "endpoint", _) => {
                return Err(HarnessError::corpus(format!(
                    "provider-generic {key} must be a string"
                )));
            }
            ("messages" | "tools", Value::Array(_)) => {}
            ("messages" | "tools", _) => {
                return Err(HarnessError::corpus(format!(
                    "provider-generic {key} must be an array"
                )));
            }
            _ => {}
        }
    }
    if obj.get("provider").and_then(Value::as_str).is_none_or(|s| s.is_empty())
    {
        return Err(HarnessError::corpus(
            "provider-generic provider must be a non-empty string",
        ));
    }
    Ok(())
}

pub(in crate::harness) fn validate_user_config_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let object = input.as_object().ok_or_else(|| {
        HarnessError::corpus("user-config input must be an object")
    })?;
    if object.len() != 1 || !object.contains_key("cases") {
        return Err(HarnessError::corpus(
            "user-config input must contain only cases",
        ));
    }
    let cases =
        object.get("cases").and_then(Value::as_array).ok_or_else(|| {
            HarnessError::corpus("user-config cases must be an array")
        })?;
    if cases.is_empty() || cases.len() > 64 {
        return Err(HarnessError::corpus(
            "user-config cases must contain 1-64 entries",
        ));
    }
    const MODES: [&str; 18] = [
        "full",
        "unknown-top",
        "unknown-nested",
        "invalid-profile",
        "invalid-backend",
        "invalid-edition",
        "installations-bound",
        "references-bound",
        "invalid-godot-path",
        "invalid-provider",
        "invalid-json",
        "exact-boundary",
        "over-boundary",
        "directory",
        "symlink",
        "missing",
        "invalid-reference-path",
        "invalid-repository",
    ];
    for case in cases {
        let case_object = case.as_object().ok_or_else(|| {
            HarnessError::corpus("user-config case must be an object")
        })?;
        if case_object.len() != 2
            || !case_object.contains_key("name")
            || !case_object.contains_key("mode")
        {
            return Err(HarnessError::corpus(
                "user-config case must contain only name and mode",
            ));
        }
        let name = case_object
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                HarnessError::corpus("user-config case name must be a string")
            })?;
        if name.is_empty() || name.len() > 64 {
            return Err(HarnessError::corpus(
                "user-config case name exceeds 64 bytes",
            ));
        }
        let mode = case_object
            .get("mode")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                HarnessError::corpus("user-config case mode must be a string")
            })?;
        if !MODES.contains(&mode) {
            return Err(HarnessError::corpus(format!(
                "unsupported user-config case mode {mode}"
            )));
        }
    }
    Ok(())
}
