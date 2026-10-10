//! Subject dispatch: probe/version infrastructure and the scenario router that runs the corpus.
use crate::harness::*;

/// Candidate state-directory probe bytes for the internal subprocess.
///
/// POSIX paths retain their exact native bytes. Windows fixture paths must be
/// valid Unicode because the TypeScript oracle can only observe a JavaScript
/// string; an unpaired native surrogate is therefore a harness protocol error,
/// not a lossy identity conversion.
///
/// # Errors
///
/// Returns [`HarnessErrorKind::ProbeProtocol`] if a Windows native path cannot
/// be represented without loss.
pub fn probe_state_dir_bytes() -> Result<Vec<u8>, HarnessError> {
    let path = match siralos_adapters::paths::state_dir() {
        Ok(path) => path,
        Err(_) => return Ok(PROBE_ERROR_MARKER.to_vec()),
    };
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(path.as_os_str().as_bytes().to_vec())
    }
    #[cfg(windows)]
    {
        path.as_os_str()
            .to_str()
            .map(|text| text.as_bytes().to_vec())
            .ok_or_else(|| {
                HarnessError::new(
                    HarnessErrorKind::ProbeProtocol,
                    "state-dir probe path is not representable by the TypeScript oracle",
                )
            })
    }
}

pub(in crate::harness) fn execute_bounded_child(
    command: &mut Command,
    timeout: Duration,
) -> Result<Vec<u8>, HarnessError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot spawn state-dir probe: {error}"),
            )
        })?;
    let deadline = Instant::now() + timeout;
    loop {
        let status = child.try_wait().map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeExit,
                format!("cannot observe state-dir probe: {error}"),
            )
        })?;
        if let Some(status) = status {
            let mut stdout = child.stdout.take().ok_or_else(|| {
                HarnessError::new(
                    HarnessErrorKind::ProbeProtocol,
                    "state-dir probe stdout was not captured",
                )
            })?;
            let mut bytes = Vec::new();
            stdout
                .by_ref()
                .take((MAX_PROBE_OUTPUT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeProtocol,
                        format!("cannot read state-dir probe output: {error}"),
                    )
                })?;
            if !status.success() {
                return Err(HarnessError::new(
                    HarnessErrorKind::ProbeExit,
                    format!(
                        "state-dir probe exited unsuccessfully ({status})"
                    ),
                ));
            }
            if bytes.is_empty() || bytes.len() > MAX_PROBE_OUTPUT_BYTES {
                return Err(HarnessError::new(
                    HarnessErrorKind::ProbeProtocol,
                    "state-dir probe output is empty or exceeds the byte bound",
                ));
            }
            return Ok(bytes);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HarnessError::new(
                HarnessErrorKind::ProbeTimeout,
                "state-dir probe timed out",
            ));
        }
        std::thread::sleep(PROBE_POLL_INTERVAL);
    }
}

fn spawn_state_dir_probe(
    env: &BTreeMap<String, String>,
) -> Result<Vec<u8>, HarnessError> {
    let executable = std::env::current_exe().map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot resolve current executable: {error}"),
        )
    })?;
    let mut command = Command::new(executable);
    command.arg("probe-state-dir").env_clear().envs(env);
    execute_bounded_child(&mut command, PROBE_TIMEOUT)
}

pub(in crate::harness) fn state_dir_record(
    scenario_id: &str,
    stdout: &[u8],
) -> Value {
    if stdout == PROBE_ERROR_MARKER {
        json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_STATE_DIR,
            "outcome": "PRODUCT_FAILURE",
            "error": { "category": "NO_HOME_DIRECTORY" },
        })
    } else {
        json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_STATE_DIR,
            "outcome": "COMPLETED",
            "result": { "stateDirSha256": sha256_hex(stdout) },
        })
    }
}

pub(in crate::harness) fn cargo_workspace_version(
    root: &Path,
) -> Result<String, HarnessError> {
    let text = read_bounded_utf8(
        &root.join("Cargo.toml"),
        MAX_MANIFEST_BYTES,
        "Cargo.toml",
        HarnessErrorKind::Input,
    )?;
    let value: toml::Value = toml::from_str(&text).map_err(|error| {
        HarnessError::input(format!("malformed Cargo.toml: {error}"))
    })?;
    value
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            HarnessError::input(
                "Cargo.toml declares no [workspace.package] version",
            )
        })
}

fn version_identity_record(
    scenario_id: &str,
    version: Result<String, HarnessError>,
) -> Value {
    match version {
        Ok(version) => json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_VERSION_IDENTITY,
            "outcome": "COMPLETED",
            "result": { "version": version },
        }),
        Err(_) => json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_VERSION_IDENTITY,
            "outcome": "PRODUCT_FAILURE",
            "error": { "category": "VERSION_UNAVAILABLE" },
        }),
    }
}

fn unsupported_record(scenario: &Scenario) -> Value {
    json!({
        "scenarioId": scenario.id,
        "subject": scenario.subject,
        "outcome": "UNSUPPORTED",
        "error": { "category": "PLATFORM_NOT_APPLICABLE" },
    })
}

fn run_scenario(
    scenario: &Scenario,
    root: &Path,
) -> Result<Value, HarnessError> {
    match scenario.subject.as_str() {
        SUBJECT_STATE_DIR => {
            let stdout = spawn_state_dir_probe(&scenario.env)?;
            Ok(state_dir_record(&scenario.id, &stdout))
        }
        SUBJECT_TASK_CONTRACT => {
            let input = scenario.input.as_ref().expect(
                "task-contract input was validated while loading the corpus",
            );
            task_contract_record(&scenario.id, input)
        }
        SUBJECT_VERSION_IDENTITY => Ok(version_identity_record(
            &scenario.id,
            cargo_workspace_version(root),
        )),
        SUBJECT_WORKSPACE_READ
        | SUBJECT_WORKSPACE_LIST
        | SUBJECT_WORKSPACE_SEARCH
        | SUBJECT_WORKSPACE_PREPARE
        | SUBJECT_WORKSPACE_APPLY
        | SUBJECT_GIT_INSPECTION => {
            let input = scenario.input.as_ref().expect(
                "workspace input was validated while loading the corpus",
            );
            let result =
                workspace_record(&scenario.id, &scenario.subject, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_SCAN => {
            let input = scenario.input.as_ref().expect(
                "context-scan input was validated while loading the corpus",
            );
            let result = context_scan_record(&scenario.id, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_WORKSPACE_REVISION => {
            let input = scenario.input.as_ref().expect(
                "workspace input was validated while loading the corpus",
            );
            let result = revision_record(&scenario.id, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CHECKPOINT => {
            let input = scenario.input.as_ref().expect(
                "workspace input was validated while loading the corpus",
            );
            let result = checkpoint_record(&scenario.id, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_LANGUAGE_DIAGNOSTICS => {
            let input = scenario.input.as_ref().expect(
                "language input was validated while loading the corpus",
            );
            let result = language_diagnostics_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_LANGUAGE_STRUCTURE => {
            let input = scenario.input.as_ref().expect(
                "language input was validated while loading the corpus",
            );
            let result = language_structure_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_LANGUAGE_DEFINITION => {
            let input = scenario.input.as_ref().expect(
                "language input was validated while loading the corpus",
            );
            let result = language_definition_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_DOMAIN_LIFECYCLE => {
            let input = scenario
                .input
                .as_ref()
                .expect("domain input was validated while loading the corpus");
            let result = domain_lifecycle_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_DOMAIN_CAPABILITY => {
            let input = scenario
                .input
                .as_ref()
                .expect("domain input was validated while loading the corpus");
            let result = domain_capability_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_PROVIDER_TURN => {
            let input = scenario.input.as_ref().expect(
                "provider input was validated while loading the corpus",
            );
            let result = provider_turn_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_PROVIDER_GENERIC => {
            let input = scenario.input.as_ref().expect(
                "provider-generic input was validated while loading the corpus",
            );
            let result = provider_generic_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_PROVIDER_REPLAY => {
            let input = scenario.input.as_ref().expect(
                "provider-replay input was validated while loading the corpus",
            );
            let result = provider_replay_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_SESSION_REPLAY => {
            let input = scenario.input.as_ref().expect(
                "session-replay input was validated while loading the corpus",
            );
            let result = session_replay_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_REPLAY_STORE => {
            let input = scenario.input.as_ref().expect(
                "replay-store input was validated while loading the corpus",
            );
            let result = replay_store_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_GRAPH => {
            let input = scenario.input.as_ref().expect(
                "context-graph input was validated while loading the corpus",
            );
            let result = context_graph_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_REPRESENTATION => {
            let input = scenario.input.as_ref().expect(
                "context-representation input was validated while loading the corpus",
            );
            let result = context_representation_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_SCHEDULER => {
            let input = scenario.input.as_ref().expect(
                "context-scheduler input was validated while loading the corpus",
            );
            let result = context_scheduler_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_TOOL => {
            let input = scenario.input.as_ref().expect(
                "context-tool input was validated while loading the corpus",
            );
            let result = context_tool_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_BENCHMARK => {
            let input = scenario.input.as_ref().expect(
                "context-benchmark input was validated while loading the corpus",
            );
            let result = context_benchmark_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_SESSION => {
            let input = scenario.input.as_ref().expect(
                "context-session input was validated while loading the corpus",
            );
            let result = context_session_record(&scenario.id, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_TUI_RENDER => {
            let input = scenario.input.as_ref().expect(
                "tui-render input was validated while loading the corpus",
            );
            let result = tui_render_record(&scenario.id, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_TOOL_LOOP => {
            let input = scenario.input.as_ref().expect(
                "tool-loop input was validated while loading the corpus",
            );
            let result = tool_loop_record(&scenario.id, input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CONTEXT_PROJECTION => {
            let input = scenario.input.as_ref().expect(
                "context-projection input was validated while loading the corpus",
            );
            let result = context_projection_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_USER_CONFIG => {
            let input = scenario.input.as_ref().expect(
                "user-config input was validated while loading the corpus",
            );
            let result = user_config_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_SECURITY_PERMISSIONS => {
            let input = scenario.input.as_ref().expect(
                "security-permissions input was validated while loading the corpus",
            );
            let result = security_permissions_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_COMMAND_CATALOG => {
            let input = scenario.input.as_ref().expect(
                "command-catalog input was validated while loading the corpus",
            );
            let result = command_catalog_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_CAPABILITY_DOCTOR => {
            let input = scenario.input.as_ref().expect(
                "capability-doctor input was validated while loading the corpus",
            );
            let result = capability_doctor_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_INSTRUCTIONS_RESOLUTION => {
            let input = scenario.input.as_ref().expect(
                "instructions-resolution input was validated while loading the corpus",
            );
            let result = instructions_resolution_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_KNOWLEDGE_REVISIONS => {
            let input = scenario.input.as_ref().expect(
                "knowledge-revisions input was validated while loading the corpus",
            );
            let result = knowledge_revisions_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_REFERENCE_IDENTITY => {
            let input = scenario.input.as_ref().expect(
                "reference-identity input was validated while loading the corpus",
            );
            let result = reference_identity_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_RESEARCH_POLICY => {
            let input = scenario.input.as_ref().expect(
                "research-policy input was validated while loading the corpus",
            );
            let result = research_policy_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_PLANNING_RUNTIME => {
            let input = scenario.input.as_ref().expect(
                "planning-runtime input was validated while loading the corpus",
            );
            let result = crate::harness_r134::planning_runtime_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_EXECUTOR_BRIEF => {
            let input = scenario.input.as_ref().expect(
                "executor-brief input was validated while loading the corpus",
            );
            let result = crate::harness_r134::executor_brief_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        SUBJECT_GODOT_SCENE_RESOLVE
        | SUBJECT_GODOT_DISCOVERY
        | SUBJECT_GODOT_KNOWLEDGE
        | SUBJECT_GODOT_DIAGNOSTICS
        | SUBJECT_GODOT_LSP
        | SUBJECT_GODOT_REVIEW_CONTEXT
        | SUBJECT_GODOT_MUTATION_PREPARE
        | SUBJECT_GODOT_DEVELOP_PLAN
        | SUBJECT_CI_ARTIFACT_DIGEST
        | SUBJECT_CI_CONTRACT_DIGEST
        | SUBJECT_CI_MANIFESTS
        | SUBJECT_CI_DELTA
        | SUBJECT_DET_REPLAY => {
            let input = scenario
                .input
                .as_ref()
                .expect("godot input was validated while loading the corpus");
            let result = godot_record(&scenario.subject, input)?;
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        SUBJECT_ICM_PHASE_CONTRACT | SUBJECT_ICM_DEP_MANIFESTS => {
            let input = scenario
                .input
                .as_ref()
                .expect("icm input was validated while loading the corpus");
            let result = match scenario.subject.as_str() {
                SUBJECT_ICM_PHASE_CONTRACT => {
                    icm_phase_contract_record(input)?
                }
                _ => icm_dependency_manifests_record(input)?,
            };
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        SUBJECT_RR_IDENTITY | SUBJECT_RR_BUDGETS | SUBJECT_RR_LIFECYCLE
        | SUBJECT_RR_DOCTOR => {
            let input = scenario.input.as_ref().expect(
                "runtime-readiness input was validated while loading the corpus",
            );
            let result = runtime_readiness_record(&scenario.subject, input)?;
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        SUBJECT_RUNTIME_EXECUTION
        | SUBJECT_RUNTIME_EVIDENCE
        | SUBJECT_VISUAL_EVIDENCE
        | SUBJECT_RUN_INTERACTION
        | SUBJECT_QA_WORKFLOW
        | SUBJECT_RUN_PROFILE
        | SUBJECT_COMPOSITION_PROFILE
        | SUBJECT_COMPOSITION_EFFECTIVE
        | SUBJECT_CONTEXT_CONTROLS
        | SUBJECT_COMPOSITION_LOCK
        | SUBJECT_COMPOSITION_PLUGIN_SELECTION
        | SUBJECT_COMPOSITION_SKILLS
        | SUBJECT_COMPOSITION_PLUGIN_ACTIVATION
        | SUBJECT_COMPOSITION_CONTEXT_CONTROL
        | SUBJECT_COMPOSITION_LOCK_VERIFY
        | SUBJECT_COMPOSITION_SKILL_CONSUMPTION
        | SUBJECT_EVOLVE_CORPUS
        | SUBJECT_EVOLVE_WORKFLOW
        | SUBJECT_EVOLVE_PROPOSAL
        | SUBJECT_EVOLVE_PACKAGING => {
            let input = scenario.input.as_ref().expect(
                "runtime input was validated while loading the corpus",
            );
            let result = match scenario.subject.as_str() {
                SUBJECT_RUNTIME_EXECUTION => runtime_execution_record(input)?,
                SUBJECT_VISUAL_EVIDENCE => visual_evidence_record(input)?,
                SUBJECT_RUN_INTERACTION => run_interaction_record(input)?,
                SUBJECT_QA_WORKFLOW => qa_workflow_record(input)?,
                SUBJECT_RUN_PROFILE => run_profile_record(input)?,
                SUBJECT_COMPOSITION_PROFILE => {
                    composition_profile_record(input)?
                }
                SUBJECT_COMPOSITION_EFFECTIVE => {
                    composition_effective_record(input)?
                }
                SUBJECT_CONTEXT_CONTROLS => context_controls_record(input)?,
                SUBJECT_COMPOSITION_LOCK => composition_lock_record(input)?,
                SUBJECT_COMPOSITION_PLUGIN_SELECTION => {
                    composition_plugin_selection_record(input)?
                }
                SUBJECT_COMPOSITION_SKILLS => {
                    composition_skills_record(input)?
                }
                SUBJECT_COMPOSITION_PLUGIN_ACTIVATION => {
                    composition_plugin_activation_record(input)?
                }
                SUBJECT_COMPOSITION_CONTEXT_CONTROL => {
                    composition_context_control_record(input)?
                }
                SUBJECT_COMPOSITION_LOCK_VERIFY => {
                    composition_lock_verify_record(input)?
                }
                SUBJECT_COMPOSITION_SKILL_CONSUMPTION => {
                    composition_skill_consumption_record(input)?
                }
                SUBJECT_EVOLVE_CORPUS => evolve_corpus_record(input)?,
                SUBJECT_EVOLVE_WORKFLOW => evolve_workflow_record(input)?,
                SUBJECT_EVOLVE_PROPOSAL => evolve_proposal_record(input)?,
                SUBJECT_EVOLVE_PACKAGING => evolve_packaging_record(input)?,
                _ => runtime_evidence_record(input)?,
            };
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        SUBJECT_GODOT_RUNTIME_LAUNCH | SUBJECT_GODOT_RUNTIME_EVIDENCE => {
            let input = scenario.input.as_ref().expect(
                "godot runtime input was validated while loading the corpus",
            );
            let result = if scenario.subject == SUBJECT_GODOT_RUNTIME_LAUNCH {
                godot_runtime_launch_record(input)?
            } else {
                godot_runtime_evidence_record(input)?
            };
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        SUBJECT_RECOVERY_TAXONOMY => {
            let input = scenario.input.as_ref().expect(
                "recovery-taxonomy input was validated while loading the corpus",
            );
            let result = recovery_taxonomy_record(input)?;
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        SUBJECT_CLI_SESSION => {
            let input = scenario.input.as_ref().expect(
                "cli-session input was validated while loading the corpus",
            );
            let result =
                crate::harness_cli_session::cli_session_record(input)?;
            Ok(json!({
                "scenarioId": scenario.id,
                "subject": scenario.subject,
                "outcome": "COMPLETED",
                "result": result,
            }))
        }
        _ => unreachable!("subject was validated while loading the corpus"),
    }
}

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Stage 3R R13.3 subjects: reference-identity, research-policy.
// ---------------------------------------------------------------------------

/// Run every applicable scenario and return exact canonical record bytes.
///
/// # Errors
///
/// Returns a typed harness failure for malformed corpus input, repository
/// input errors, or a probe lifecycle/protocol failure.
pub fn run_corpus(
    corpus_dir: &Path,
    root: &Path,
    scenario_id: Option<&str>,
) -> Result<String, HarnessError> {
    let scenarios = load_corpus(corpus_dir, platform_name())?;
    let mut records = Vec::new();
    let mut selected = scenario_id.is_none();
    for loaded in scenarios {
        if scenario_id.is_some_and(|id| id != loaded.scenario.id) {
            continue;
        }
        selected = true;
        records.push(if loaded.applicable {
            run_scenario(&loaded.scenario, root)?
        } else {
            unsupported_record(&loaded.scenario)
        });
    }
    if !selected {
        return Err(HarnessError::with_code(
            HarnessErrorKind::Corpus,
            "UNKNOWN_SCENARIO",
            "requested scenario is not present in the digest-bound corpus",
        ));
    }
    Ok(canonical_records_text(records))
}

pub(in crate::harness) fn canonical_records_text(
    records: Vec<Value>,
) -> String {
    let document = json!({
        "schemaVersion": RUNNER_PROTOCOL_SCHEMA_VERSION,
        "records": records,
    });
    let text = serde_json::to_string(&document)
        .expect("outcome records are constructed from serializable values");
    format!("{text}\n")
}
