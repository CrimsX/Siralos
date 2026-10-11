//! Strict corpus loader: bounded reads, manifest verification, input-shape validation, and corpus digesting.
use crate::harness::*;

pub(in crate::harness) fn read_bounded_utf8(
    path: &Path,
    maximum_bytes: usize,
    label: &str,
    error_kind: HarnessErrorKind,
) -> Result<String, HarnessError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        HarnessError::new(
            error_kind,
            format!("cannot inspect {label}: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(HarnessError::new(
            error_kind,
            format!("{label} must be a regular file"),
        ));
    }
    let declared_len = usize::try_from(metadata.len()).map_err(|_| {
        HarnessError::new(error_kind, format!("{label} is too large"))
    })?;
    if declared_len > maximum_bytes {
        return Err(HarnessError::new(
            error_kind,
            format!("{label} exceeds {maximum_bytes} bytes"),
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        HarnessError::new(error_kind, format!("cannot read {label}: {error}"))
    })?;
    if bytes.len() > maximum_bytes {
        return Err(HarnessError::new(
            error_kind,
            format!("{label} exceeds {maximum_bytes} bytes"),
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        HarnessError::new(error_kind, format!("{label} is not valid UTF-8"))
    })
}

fn absolute_corpus_directory(
    path: &Path,
    label: &str,
) -> Result<std::path::PathBuf, HarnessError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                HarnessError::corpus(format!(
                    "cannot resolve {label}: {error}"
                ))
            })?
            .join(path)
    };
    let metadata = std::fs::symlink_metadata(&absolute).map_err(|error| {
        HarnessError::corpus(format!("cannot inspect {label}: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(HarnessError::corpus(format!(
            "{label} may not be a symlink"
        )));
    }
    if !metadata.is_dir() {
        return Err(HarnessError::corpus(format!(
            "{label} must be a directory"
        )));
    }
    Ok(absolute)
}

fn valid_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return false;
    }
    let mut previous_separator = false;
    for (index, byte) in value.bytes().enumerate() {
        let separator = byte == b'.' || byte == b'-';
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || separator)
            || (separator && (index == 0 || previous_separator))
        {
            return false;
        }
        previous_separator = separator;
    }
    !previous_separator
}

fn valid_file_name(file: &str) -> bool {
    if file.is_empty() || file.len() > MAX_FILE_NAME_BYTES {
        return false;
    }
    let path = Path::new(file);
    if path.is_absolute() {
        return false;
    }
    let mut components = path.components();
    let one_normal = matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none();
    one_normal
        && file.ends_with(".json")
        && file.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'.'
                || byte == b'-'
        })
        && file.as_bytes()[0].is_ascii_alphanumeric()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_environment(scenario: &Scenario) -> Result<(), HarnessError> {
    if scenario.env.len() > MAX_ENV_ENTRIES {
        return Err(HarnessError::corpus(format!(
            "scenario {} has too many environment entries",
            scenario.id
        )));
    }
    for (key, value) in &scenario.env {
        let valid_key = !key.is_empty()
            && key.len() <= MAX_ENV_KEY_BYTES
            && key.bytes().enumerate().all(|(index, byte)| {
                (index == 0 && (byte.is_ascii_uppercase() || byte == b'_'))
                    || (index > 0
                        && (byte.is_ascii_uppercase()
                            || byte.is_ascii_digit()
                            || byte == b'_'))
            })
            && matches!(
                key.as_str(),
                "HOME" | "HOMEDRIVE" | "HOMEPATH" | "USERPROFILE"
            );
        if !valid_key {
            return Err(HarnessError::corpus(format!(
                "scenario {} has unsupported environment key",
                scenario.id
            )));
        }
        if value.len() > MAX_ENV_VALUE_BYTES || value.contains('\0') {
            return Err(HarnessError::corpus(format!(
                "scenario {} has an invalid environment value",
                scenario.id
            )));
        }
    }
    Ok(())
}

pub(in crate::harness) fn validate_scenario(
    scenario: &Scenario,
    file: &str,
) -> Result<(), HarnessError> {
    if !valid_identifier(&scenario.id)
        || format!("{}.json", scenario.id) != file
    {
        return Err(HarnessError::corpus(format!(
            "scenario {file} has a noncanonical or mismatched id"
        )));
    }
    if !matches!(
        scenario.subject.as_str(),
        SUBJECT_STATE_DIR
            | SUBJECT_VERSION_IDENTITY
            | SUBJECT_TASK_CONTRACT
            | SUBJECT_WORKSPACE_READ
            | SUBJECT_WORKSPACE_LIST
            | SUBJECT_WORKSPACE_SEARCH
            | SUBJECT_WORKSPACE_REVISION
            | SUBJECT_WORKSPACE_PREPARE
            | SUBJECT_WORKSPACE_APPLY
            | SUBJECT_CHECKPOINT
            | SUBJECT_GIT_INSPECTION
            | SUBJECT_LANGUAGE_DIAGNOSTICS
            | SUBJECT_LANGUAGE_STRUCTURE
            | SUBJECT_LANGUAGE_DEFINITION
            | SUBJECT_MODULE_LIFECYCLE
            | SUBJECT_MODULE_CAPABILITY
            | SUBJECT_PROVIDER_TURN
            | SUBJECT_TOOL_LOOP
            | SUBJECT_CONTEXT_PROJECTION
            | SUBJECT_USER_CONFIG
            | SUBJECT_SECURITY_PERMISSIONS
            | SUBJECT_COMMAND_CATALOG
            | SUBJECT_CAPABILITY_DOCTOR
            | SUBJECT_INSTRUCTIONS_RESOLUTION
            | SUBJECT_KNOWLEDGE_REVISIONS
            | SUBJECT_REFERENCE_IDENTITY
            | SUBJECT_RESEARCH_POLICY
            | SUBJECT_PLANNING_RUNTIME
            | SUBJECT_EXECUTOR_BRIEF
            | SUBJECT_CI_ARTIFACT_DIGEST
            | SUBJECT_CI_CONTRACT_DIGEST
            | SUBJECT_CI_MANIFESTS
            | SUBJECT_CI_DELTA
            | SUBJECT_DET_REPLAY
            | SUBJECT_ICM_PHASE_CONTRACT
            | SUBJECT_ICM_DEP_MANIFESTS
            | SUBJECT_RECOVERY_TAXONOMY
            | SUBJECT_RR_IDENTITY
            | SUBJECT_RR_BUDGETS
            | SUBJECT_RR_LIFECYCLE
            | SUBJECT_RR_DOCTOR
            | SUBJECT_RUNTIME_EXECUTION
            | SUBJECT_RUNTIME_EVIDENCE
            | SUBJECT_VISUAL_EVIDENCE
            | SUBJECT_RUN_INTERACTION
            | SUBJECT_QA_WORKFLOW
            | SUBJECT_RUN_PROFILE
            | SUBJECT_COMPOSITION_PROFILE
            | SUBJECT_COMPOSITION_EFFECTIVE
            | SUBJECT_CONTEXT_CONTROLS
            | SUBJECT_COMPOSITION_LOCK
            | SUBJECT_COMPOSITION_MODULE_SELECTION
            | SUBJECT_COMPOSITION_SKILLS
            | SUBJECT_COMPOSITION_MODULE_ACTIVATION
            | SUBJECT_COMPOSITION_CONTEXT_CONTROL
            | SUBJECT_COMPOSITION_LOCK_VERIFY
            | SUBJECT_COMPOSITION_SKILL_CONSUMPTION
            | SUBJECT_EVOLVE_CORPUS
            | SUBJECT_EVOLVE_WORKFLOW
            | SUBJECT_EVOLVE_PROPOSAL
            | SUBJECT_EVOLVE_PACKAGING
            | SUBJECT_PROVIDER_GENERIC
            | SUBJECT_PROVIDER_REPLAY
            | SUBJECT_SESSION_REPLAY
            | SUBJECT_REPLAY_STORE
            | SUBJECT_CONTEXT_GRAPH
            | SUBJECT_CONTEXT_REPRESENTATION
            | SUBJECT_CONTEXT_SCHEDULER
            | SUBJECT_CONTEXT_TOOL
            | SUBJECT_CONTEXT_SCAN
            | SUBJECT_CONTEXT_BENCHMARK
            | SUBJECT_CONTEXT_SESSION
            | SUBJECT_TUI_RENDER
            | SUBJECT_CLI_SESSION
    ) {
        return Err(HarnessError::corpus(format!(
            "scenario {} has an unsupported subject",
            scenario.id
        )));
    }
    if !matches!(scenario.parity.as_str(), "required" | "informational") {
        return Err(HarnessError::corpus(format!(
            "scenario {} has an unsupported parity classification",
            scenario.id
        )));
    }
    if scenario.platforms.is_empty() || scenario.platforms.len() > 2 {
        return Err(HarnessError::corpus(format!(
            "scenario {} has an invalid platform count",
            scenario.id
        )));
    }
    let platforms: BTreeSet<&str> =
        scenario.platforms.iter().map(String::as_str).collect();
    if platforms.len() != scenario.platforms.len()
        || platforms
            .iter()
            .any(|platform| !matches!(*platform, "*" | "windows" | "posix"))
        || (platforms.contains("*") && platforms.len() != 1)
    {
        return Err(HarnessError::corpus(format!(
            "scenario {} has invalid or duplicate platforms",
            scenario.id
        )));
    }
    validate_environment(scenario)?;
    match scenario.subject.as_str() {
        SUBJECT_VERSION_IDENTITY => {
            if platforms != BTreeSet::from(["*"]) || !scenario.env.is_empty() {
                return Err(HarnessError::corpus(format!(
                    "scenario {} has invalid version-identity inputs",
                    scenario.id
                )));
            }
        }
        SUBJECT_TASK_CONTRACT => {
            if platforms != BTreeSet::from(["*"]) || !scenario.env.is_empty() {
                return Err(HarnessError::corpus(format!(
                    "scenario {} task-contract inputs must use platforms [\"*\"] and an empty env",
                    scenario.id
                )));
            }
            let input = scenario.input.as_ref().ok_or_else(|| {
                HarnessError::corpus(format!(
                    "scenario {} task-contract requires an input object",
                    scenario.id
                ))
            })?;
            if !input.is_object() {
                return Err(HarnessError::corpus(format!(
                    "scenario {} task-contract input must be an object",
                    scenario.id
                )));
            }
            let serialized = serde_json::to_vec(input).map_err(|error| {
                HarnessError::corpus(format!(
                    "scenario {} input cannot be serialized: {error}",
                    scenario.id
                ))
            })?;
            if serialized.len() > MAX_TASK_INPUT_BYTES {
                return Err(HarnessError::corpus(format!(
                    "scenario {} input exceeds {MAX_TASK_INPUT_BYTES} bytes",
                    scenario.id
                )));
            }
        }
        SUBJECT_WORKSPACE_READ
        | SUBJECT_WORKSPACE_LIST
        | SUBJECT_WORKSPACE_SEARCH
        | SUBJECT_WORKSPACE_REVISION
        | SUBJECT_WORKSPACE_PREPARE
        | SUBJECT_WORKSPACE_APPLY
        | SUBJECT_CHECKPOINT
        | SUBJECT_GIT_INSPECTION
        | SUBJECT_LANGUAGE_DIAGNOSTICS
        | SUBJECT_LANGUAGE_STRUCTURE
        | SUBJECT_LANGUAGE_DEFINITION
        | SUBJECT_MODULE_LIFECYCLE
        | SUBJECT_MODULE_CAPABILITY
        | SUBJECT_PROVIDER_TURN
        | SUBJECT_PROVIDER_GENERIC
        | SUBJECT_PROVIDER_REPLAY
        | SUBJECT_SESSION_REPLAY
        | SUBJECT_REPLAY_STORE
        | SUBJECT_CONTEXT_GRAPH
        | SUBJECT_CONTEXT_REPRESENTATION
        | SUBJECT_CONTEXT_SCHEDULER
        | SUBJECT_CONTEXT_TOOL
        | SUBJECT_CONTEXT_SCAN
        | SUBJECT_CONTEXT_BENCHMARK
        | SUBJECT_CONTEXT_SESSION
        | SUBJECT_TUI_RENDER
        | SUBJECT_TOOL_LOOP
        | SUBJECT_CONTEXT_PROJECTION
        | SUBJECT_USER_CONFIG
        | SUBJECT_SECURITY_PERMISSIONS
        | SUBJECT_COMMAND_CATALOG
        | SUBJECT_CAPABILITY_DOCTOR
        | SUBJECT_INSTRUCTIONS_RESOLUTION
        | SUBJECT_KNOWLEDGE_REVISIONS
        | SUBJECT_REFERENCE_IDENTITY
        | SUBJECT_RESEARCH_POLICY
        | SUBJECT_PLANNING_RUNTIME
        | SUBJECT_EXECUTOR_BRIEF
        | SUBJECT_CLI_SESSION => {
            let input = scenario.input.as_ref().ok_or_else(|| {
                HarnessError::corpus(format!(
                    "scenario {} {} requires an input object",
                    scenario.id, scenario.subject
                ))
            })?;
            if !input.is_object() {
                return Err(HarnessError::corpus(format!(
                    "scenario {} {} input must be an object",
                    scenario.id, scenario.subject
                )));
            }
            let posix_symlink_only = scenario.subject == SUBJECT_USER_CONFIG
                && platforms == BTreeSet::from([PLATFORM_POSIX])
                && input.get("cases").and_then(Value::as_array).is_some_and(
                    |cases| {
                        cases.iter().all(|case| {
                            case.get("mode").and_then(Value::as_str)
                                == Some("symlink")
                        })
                    },
                );
            if (platforms != BTreeSet::from(["*"]) && !posix_symlink_only)
                || !scenario.env.is_empty()
            {
                return Err(HarnessError::corpus(format!(
                    "scenario {} {} inputs must use platforms [\"*\"] or a POSIX-only symlink case and an empty env",
                    scenario.id, scenario.subject
                )));
            }
            let serialized = serde_json::to_vec(input).map_err(|error| {
                HarnessError::corpus(format!(
                    "scenario {} input cannot be serialized: {error}",
                    scenario.id
                ))
            })?;
            let provider_subject =
                scenario.subject.as_str() == SUBJECT_PROVIDER_TURN;
            if provider_subject {
                validate_provider_turn_input(input)?;
            }
            let provider_generic_subject =
                scenario.subject.as_str() == SUBJECT_PROVIDER_GENERIC;
            if provider_generic_subject {
                validate_provider_generic_input(input)?;
            }
            let provider_replay_subject =
                scenario.subject.as_str() == SUBJECT_PROVIDER_REPLAY;
            if provider_replay_subject {
                validate_provider_replay_input(input)?;
            }
            let session_replay_subject =
                scenario.subject.as_str() == SUBJECT_SESSION_REPLAY;
            if session_replay_subject {
                validate_session_replay_input(input)?;
            }
            let replay_store_subject =
                scenario.subject.as_str() == SUBJECT_REPLAY_STORE;
            if replay_store_subject {
                validate_replay_store_input(input)?;
            }
            let context_graph_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_GRAPH;
            if context_graph_subject {
                validate_context_graph_input(input)?;
            }
            let context_representation_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_REPRESENTATION;
            if context_representation_subject {
                validate_context_representation_input(input)?;
            }
            let context_scheduler_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_SCHEDULER;
            if context_scheduler_subject {
                validate_context_scheduler_input(input)?;
            }
            let context_tool_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_TOOL;
            if context_tool_subject {
                validate_context_tool_input(input)?;
            }
            let context_benchmark_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_BENCHMARK;
            if context_benchmark_subject {
                validate_context_benchmark_input(input)?;
            }
            let context_session_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_SESSION;
            if context_session_subject {
                validate_context_session_input(input)?;
            }
            let tui_render_subject =
                scenario.subject.as_str() == SUBJECT_TUI_RENDER;
            if tui_render_subject {
                validate_tui_render_input(input)?;
            }
            let tool_loop_subject =
                scenario.subject.as_str() == SUBJECT_TOOL_LOOP;
            if tool_loop_subject {
                validate_tool_loop_input(input)?;
            }
            let language_subject = matches!(
                scenario.subject.as_str(),
                SUBJECT_LANGUAGE_DIAGNOSTICS
                    | SUBJECT_LANGUAGE_STRUCTURE
                    | SUBJECT_LANGUAGE_DEFINITION
            );
            let domain_subject = matches!(
                scenario.subject.as_str(),
                SUBJECT_MODULE_LIFECYCLE | SUBJECT_MODULE_CAPABILITY
            );
            let context_projection_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_PROJECTION;
            let user_config_subject =
                scenario.subject.as_str() == SUBJECT_USER_CONFIG;
            if user_config_subject {
                validate_user_config_input(input)?;
            }
            let r13_authority_subject = matches!(
                scenario.subject.as_str(),
                SUBJECT_SECURITY_PERMISSIONS
                    | SUBJECT_COMMAND_CATALOG
                    | SUBJECT_CAPABILITY_DOCTOR
            );
            if r13_authority_subject {
                validate_r13_authority_input(
                    scenario.subject.as_str(),
                    input,
                )?;
            }
            let r13_guidance_subject = matches!(
                scenario.subject.as_str(),
                SUBJECT_INSTRUCTIONS_RESOLUTION | SUBJECT_KNOWLEDGE_REVISIONS
            );
            if r13_guidance_subject {
                validate_r13_guidance_input(scenario.subject.as_str(), input)?;
            }
            let r13_external_knowledge_subject = matches!(
                scenario.subject.as_str(),
                SUBJECT_REFERENCE_IDENTITY | SUBJECT_RESEARCH_POLICY
            );
            if r13_external_knowledge_subject {
                validate_r13_external_knowledge_input(
                    scenario.subject.as_str(),
                    input,
                )?;
            }
            let r13_planning_briefing_subject = matches!(
                scenario.subject.as_str(),
                SUBJECT_PLANNING_RUNTIME | SUBJECT_EXECUTOR_BRIEF
            );
            if r13_planning_briefing_subject {
                crate::harness_r134::validate_r13_planning_briefing_input(
                    scenario.subject.as_str(),
                    input,
                )?;
            }
            let cli_session_subject =
                scenario.subject.as_str() == SUBJECT_CLI_SESSION;
            if cli_session_subject {
                crate::harness_cli_session::validate_cli_session_input(input)?;
            }
            let max_input_bytes = if provider_subject
                || provider_generic_subject
                || provider_replay_subject
                || session_replay_subject
                || context_graph_subject
                || context_representation_subject
                || context_scheduler_subject
                || context_tool_subject
                || context_benchmark_subject
                || context_session_subject
                || tui_render_subject
            {
                MAX_PROVIDER_INPUT_BYTES
            } else if tool_loop_subject {
                MAX_TOOL_LOOP_INPUT_BYTES
            } else if context_projection_subject {
                MAX_CONTEXT_PROJECTION_INPUT_BYTES
            } else if user_config_subject {
                MAX_USER_CONFIG_INPUT_BYTES
            } else if r13_authority_subject {
                MAX_R13_AUTHORITY_INPUT_BYTES
            } else if r13_guidance_subject {
                MAX_R13_GUIDANCE_INPUT_BYTES
            } else if r13_external_knowledge_subject {
                MAX_R13_EXTERNAL_KNOWLEDGE_INPUT_BYTES
            } else if r13_planning_briefing_subject {
                crate::harness_r134::MAX_R13_PLANNING_BRIEFING_INPUT_BYTES
            } else if cli_session_subject {
                MAX_CLI_SESSION_INPUT_BYTES
            } else if language_subject {
                MAX_LANGUAGE_INPUT_BYTES
            } else if domain_subject {
                MAX_MODULE_INPUT_BYTES
            } else {
                MAX_WORKSPACE_INPUT_BYTES
            };
            if serialized.len() > max_input_bytes {
                return Err(HarnessError::corpus(format!(
                    "scenario {} input exceeds {max_input_bytes} bytes",
                    scenario.id
                )));
            }
        }
        SUBJECT_CI_ARTIFACT_DIGEST
        | SUBJECT_CI_CONTRACT_DIGEST
        | SUBJECT_CI_MANIFESTS
        | SUBJECT_CI_DELTA
        | SUBJECT_DET_REPLAY
        | SUBJECT_ICM_PHASE_CONTRACT
        | SUBJECT_ICM_DEP_MANIFESTS
        | SUBJECT_RECOVERY_TAXONOMY
        | SUBJECT_RR_IDENTITY
        | SUBJECT_RR_BUDGETS
        | SUBJECT_RR_LIFECYCLE
        | SUBJECT_RR_DOCTOR
        | SUBJECT_RUNTIME_EXECUTION
        | SUBJECT_RUNTIME_EVIDENCE
        | SUBJECT_VISUAL_EVIDENCE
        | SUBJECT_RUN_INTERACTION
        | SUBJECT_QA_WORKFLOW
        | SUBJECT_RUN_PROFILE
        | SUBJECT_COMPOSITION_PROFILE
        | SUBJECT_COMPOSITION_EFFECTIVE
        | SUBJECT_CONTEXT_CONTROLS
        | SUBJECT_COMPOSITION_LOCK
        | SUBJECT_COMPOSITION_MODULE_SELECTION
        | SUBJECT_COMPOSITION_SKILLS
        | SUBJECT_COMPOSITION_MODULE_ACTIVATION
        | SUBJECT_COMPOSITION_CONTEXT_CONTROL
        | SUBJECT_COMPOSITION_LOCK_VERIFY
        | SUBJECT_COMPOSITION_SKILL_CONSUMPTION
        | SUBJECT_EVOLVE_CORPUS
        | SUBJECT_EVOLVE_WORKFLOW
        | SUBJECT_EVOLVE_PROPOSAL
        | SUBJECT_EVOLVE_PACKAGING => {
            if platforms != BTreeSet::from(["*"]) || !scenario.env.is_empty() {
                return Err(HarnessError::corpus(format!(
                    "scenario {} {} inputs must use platforms [\"*\"] and an empty env",
                    scenario.id, scenario.subject
                )));
            }
            let input = scenario.input.as_ref().ok_or_else(|| {
                HarnessError::corpus(format!(
                    "scenario {} {} requires an input object",
                    scenario.id, scenario.subject
                ))
            })?;
            if !input.is_object() {
                return Err(HarnessError::corpus(format!(
                    "scenario {} {} input must be an object",
                    scenario.id, scenario.subject
                )));
            }
            let serialized = serde_json::to_vec(input).map_err(|error| {
                HarnessError::corpus(format!(
                    "scenario {} input cannot be serialized: {error}",
                    scenario.id
                ))
            })?;
            if serialized.len() > MAX_SHARED_INPUT_BYTES {
                return Err(HarnessError::corpus(format!(
                    "scenario {} input exceeds {MAX_SHARED_INPUT_BYTES} bytes",
                    scenario.id
                )));
            }
            validate_scenario_input(&scenario.subject, input)?;
        }
        SUBJECT_STATE_DIR => {
            if platforms.len() != 1 || platforms.contains("*") {
                return Err(HarnessError::corpus(format!(
                    "scenario {} must target one concrete platform",
                    scenario.id
                )));
            }
            let windows = platforms.contains(PLATFORM_WINDOWS);
            let wrong_key = scenario.env.keys().any(|key| {
                if windows {
                    !matches!(
                        key.as_str(),
                        "USERPROFILE" | "HOMEDRIVE" | "HOMEPATH"
                    )
                } else {
                    key != "HOME"
                }
            });
            if wrong_key {
                return Err(HarnessError::corpus(format!(
                    "scenario {} has an environment key for another platform",
                    scenario.id
                )));
            }
            let controlled = if windows {
                scenario.env.contains_key("USERPROFILE")
            } else {
                scenario.env.get("HOME").is_some_and(|home| !home.is_empty())
            };
            if scenario.parity == "required" && !controlled {
                return Err(HarnessError::corpus(format!(
                    "scenario {} required parity does not fully declare its home-resolution input",
                    scenario.id
                )));
            }
        }
        _ => unreachable!("subject was validated above"),
    }
    Ok(())
}

/// Strict scenario-shape check for the nightly fuzz target.
///
/// Feeds arbitrary bytes through the exact per-scenario gate the
/// corpus loader enforces — typed JSON parse of [`Scenario`], strict
/// shape validation, and the canonical scenario digest — without
/// touching the filesystem or spawning a process. Arbitrary input
/// can only yield `Err`, never a panic, a hang, or an over-allocation
/// beyond the 16 KiB scenario bound this function shares with the
/// corpus loader.
///
/// The manifest file name the loader would check against is derived
/// from the decoded id (`{id}.json`), mirroring the loader's
/// per-entry check. This function never loosens that gate: unknown
/// fields, unknown subjects, and bad parity classifications are all
/// typed corpus rejections.
pub fn fuzz_scenario_bytes(data: &[u8]) -> Result<(), HarnessError> {
    if data.len() > MAX_SCENARIO_BYTES {
        return Err(HarnessError::corpus(format!(
            "scenario exceeds {MAX_SCENARIO_BYTES} bytes"
        )));
    }
    let text = std::str::from_utf8(data)
        .map_err(|_| HarnessError::corpus("scenario is not valid UTF-8"))?;
    let scenario: Scenario = serde_json::from_str(text).map_err(|error| {
        HarnessError::corpus(format!("malformed scenario: {error}"))
    })?;
    // The file name is derived from the decoded id, so the
    // id<->file clause of `validate_scenario` is exercised here only
    // in its identifier half; the mismatch arm is covered by the
    // directory loader, which pairs manifest names against payloads.
    let file = format!("{}.json", scenario.id);
    validate_scenario(&scenario, &file)?;
    canonical_scenario_digest(&scenario)?;
    Ok(())
}

pub(in crate::harness) fn canonical_scenario_digest(
    scenario: &Scenario,
) -> Result<String, HarnessError> {
    let value = serde_json::to_value(scenario).map_err(|error| {
        HarnessError::corpus(format!(
            "cannot canonicalize scenario {}: {error}",
            scenario.id
        ))
    })?;
    let canonical = serde_json::to_vec(&value).map_err(|error| {
        HarnessError::corpus(format!(
            "cannot serialize scenario {}: {error}",
            scenario.id
        ))
    })?;
    Ok(sha256_hex(&canonical))
}

fn canonical_corpus_digest(
    manifest: &Manifest,
) -> Result<String, HarnessError> {
    let value = json!({
        "schemaVersion": manifest.schema_version,
        "corpusVersion": manifest.corpus_version,
        "scenarios": &manifest.scenarios,
    });
    let canonical = serde_json::to_vec(&value).map_err(|error| {
        HarnessError::corpus(format!(
            "cannot serialize corpus identity: {error}"
        ))
    })?;
    Ok(sha256_hex(&canonical))
}

pub(in crate::harness) fn load_corpus(
    corpus_dir: &Path,
    platform: &str,
) -> Result<Vec<LoadedScenario>, HarnessError> {
    if !matches!(platform, PLATFORM_WINDOWS | PLATFORM_POSIX) {
        return Err(HarnessError::corpus("unsupported host platform"));
    }
    let corpus_dir =
        absolute_corpus_directory(corpus_dir, "corpus directory")?;
    let manifest_text = read_bounded_utf8(
        &corpus_dir.join("manifest.json"),
        MAX_MANIFEST_BYTES,
        "corpus manifest",
        HarnessErrorKind::Corpus,
    )?;
    let manifest: Manifest =
        serde_json::from_str(&manifest_text).map_err(|error| {
            HarnessError::corpus(format!("malformed corpus manifest: {error}"))
        })?;
    if manifest.schema_version != CORPUS_SCHEMA_VERSION {
        return Err(HarnessError::with_code(
            HarnessErrorKind::Corpus,
            "UNSUPPORTED_VERSION",
            format!(
                "unsupported corpus schemaVersion {}",
                manifest.schema_version
            ),
        ));
    }
    if manifest.corpus_version != CORPUS_VERSION {
        return Err(HarnessError::with_code(
            HarnessErrorKind::Corpus,
            "UNSUPPORTED_VERSION",
            format!("unsupported corpusVersion {}", manifest.corpus_version),
        ));
    }
    if manifest.scenarios.is_empty()
        || manifest.scenarios.len() > MAX_SCENARIOS
    {
        return Err(HarnessError::corpus(format!(
            "corpus must contain 1-{MAX_SCENARIOS} scenarios"
        )));
    }

    let corpus_sha256 =
        manifest.corpus_sha256.as_deref().ok_or_else(|| {
            HarnessError::with_code(
                HarnessErrorKind::Corpus,
                "MISSING_DIGEST",
                "corpus manifest.corpusSha256 is required",
            )
        })?;
    if !valid_sha256(corpus_sha256) {
        return Err(HarnessError::with_code(
            HarnessErrorKind::Corpus,
            "MALFORMED_DIGEST",
            "corpus manifest.corpusSha256 must be a lowercase SHA-256 digest",
        ));
    }

    let mut files = BTreeSet::new();
    for entry in &manifest.scenarios {
        if !valid_file_name(&entry.file) {
            return Err(HarnessError::corpus(
                "manifest entry has an invalid file name",
            ));
        }
        let entry_sha256 = entry.sha256.as_deref().ok_or_else(|| {
            HarnessError::with_code(
                HarnessErrorKind::Corpus,
                "MISSING_DIGEST",
                format!("manifest entry {} has no digest", entry.file),
            )
        })?;
        if !valid_sha256(entry_sha256) {
            return Err(HarnessError::with_code(
                HarnessErrorKind::Corpus,
                "MALFORMED_DIGEST",
                format!("manifest entry {} has an invalid digest", entry.file),
            ));
        }
        if !files.insert(entry.file.clone()) {
            return Err(HarnessError::corpus(format!(
                "corpus contains duplicate file {}",
                entry.file
            )));
        }
    }
    if canonical_corpus_digest(&manifest)? != corpus_sha256 {
        return Err(HarnessError::with_code(
            HarnessErrorKind::Corpus,
            "CONTENT_MISMATCH",
            "corpus manifest does not match corpusSha256",
        ));
    }

    let mut ids = BTreeSet::new();
    let mut loaded = Vec::with_capacity(manifest.scenarios.len());
    for entry in manifest.scenarios {
        let entry_sha256 = entry
            .sha256
            .as_deref()
            .expect("manifest digest was validated before scenario loading");
        let text = read_bounded_utf8(
            &corpus_dir.join(&entry.file),
            MAX_SCENARIO_BYTES,
            &format!("scenario {}", entry.file),
            HarnessErrorKind::Corpus,
        )?;
        let scenario: Scenario =
            serde_json::from_str(&text).map_err(|error| {
                HarnessError::corpus(format!(
                    "malformed scenario {}: {error}",
                    entry.file
                ))
            })?;
        validate_scenario(&scenario, &entry.file)?;
        if canonical_scenario_digest(&scenario)? != entry_sha256 {
            return Err(HarnessError::with_code(
                HarnessErrorKind::Corpus,
                "CONTENT_MISMATCH",
                format!(
                    "scenario {} does not match its manifest digest",
                    entry.file
                ),
            ));
        }
        if !ids.insert(scenario.id.clone()) {
            return Err(HarnessError::corpus(format!(
                "corpus contains duplicate scenario id {}",
                scenario.id
            )));
        }
        let applicable = scenario
            .platforms
            .iter()
            .any(|candidate| candidate == "*" || candidate == platform);
        loaded.push(LoadedScenario { scenario, applicable });
    }
    Ok(loaded)
}

pub(in crate::harness) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    // sha2 0.11 returns a hybrid-array Array that no longer implements
    // LowerHex; format the digest bytes explicitly. The produced hex
    // string is byte-identical to the previous LowerHex output.
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(in crate::harness) fn object_field<'a>(
    object: &'a Value,
    key: &str,
) -> Result<&'a Value, HarnessError> {
    object.get(key).ok_or_else(|| {
        HarnessError::corpus(format!("task input missing field {key}"))
    })
}

pub(in crate::harness) fn string_value(
    value: &Value,
    label: &str,
) -> Result<String, HarnessError> {
    value.as_str().map(str::to_owned).ok_or_else(|| {
        HarnessError::corpus(format!("task input {label} must be a string"))
    })
}

pub(in crate::harness) fn string_field(
    object: &Value,
    key: &str,
) -> Result<String, HarnessError> {
    string_value(object_field(object, key)?, key)
}

pub(in crate::harness) fn optional_string_field(
    object: &Value,
    key: &str,
) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub(in crate::harness) fn scenario_string(
    input: &Value,
    key: &str,
) -> Result<String, HarnessError> {
    input.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(
        || {
            HarnessError::corpus(format!(
                "scenario input missing string field {key}"
            ))
        },
    )
}

pub(in crate::harness) fn scenario_u64(
    input: &Value,
    key: &str,
) -> Option<u64> {
    input.get(key).and_then(Value::as_u64)
}

pub(in crate::harness) fn scenario_array<'a>(
    input: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, HarnessError> {
    input.get(key).and_then(Value::as_array).ok_or_else(|| {
        HarnessError::corpus(format!(
            "scenario input missing array field {key}"
        ))
    })
}

/// Scenario string array field (typed corpus-integrity failure).
pub(in crate::harness) fn scenario_string_array(
    input: &Value,
    key: &str,
) -> Result<Vec<String>, HarnessError> {
    input
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_owned).ok_or_else(|| {
                        HarnessError::corpus(format!(
                            "scenario input field {key} must contain only strings"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .ok_or_else(|| {
            HarnessError::corpus(format!(
                "scenario input missing string array field {key}"
            ))
        })?
}

pub(in crate::harness) fn scenario_object<'a>(
    input: &'a Value,
    key: &str,
) -> Result<&'a serde_json::Map<String, Value>, HarnessError> {
    input.get(key).and_then(Value::as_object).ok_or_else(|| {
        HarnessError::corpus(format!(
            "scenario input missing object field {key}"
        ))
    })
}
