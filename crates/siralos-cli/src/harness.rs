#![allow(clippy::items_after_test_module)]

//! Differential behavioral harness candidate runner (ADR 0033).
//!
//! The checked-in corpus is parsed as a strict, bounded protocol. Each
//! applicable scenario runs against the Rust candidate and produces one
//! canonical outcome record in manifest order.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Component, Path};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod tool_loop;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use siralos_adapters::config::MAX_CONFIG_FILE_BYTES;
use siralos_adapters::provider::DeterministicFakeProvider;
use siralos_adapters::tool::WorkspaceReadTool;
use siralos_core::provider::{
    AssistantToolCallInput, CancellationSignal, CancellationToken,
    ConversationItem, LimitClass, ModelProvider, ModelRequest,
    ProtocolFailure, ProviderEvent, ToolDefinition, ToolExecutionResult,
    TurnFailure, TurnOutcome, TurnToolCall, collect_provider_turn,
    detach_bounded_tool_result,
};
use tool_loop::{tool_loop_record, validate_tool_loop_input};

/// Scenario platform value for Windows hosts.
pub const PLATFORM_WINDOWS: &str = "windows";

/// Scenario platform value for POSIX hosts.
pub const PLATFORM_POSIX: &str = "posix";

const SUBJECT_STATE_DIR: &str = "state-dir";
const SUBJECT_VERSION_IDENTITY: &str = "version-identity";
const SUBJECT_TASK_CONTRACT: &str = "task-contract";
const SUBJECT_WORKSPACE_READ: &str = "workspace-read";
const SUBJECT_WORKSPACE_LIST: &str = "workspace-list";
const SUBJECT_WORKSPACE_SEARCH: &str = "workspace-search";
const SUBJECT_WORKSPACE_REVISION: &str = "workspace-revision";
const SUBJECT_WORKSPACE_PREPARE: &str = "workspace-prepare";
const SUBJECT_CHECKPOINT: &str = "checkpoint";
const SUBJECT_GIT_INSPECTION: &str = "git-inspection";
const SUBJECT_LANGUAGE_DIAGNOSTICS: &str = "language-diagnostics";
const SUBJECT_LANGUAGE_STRUCTURE: &str = "language-structure";
const SUBJECT_LANGUAGE_DEFINITION: &str = "language-definition";
const SUBJECT_DOMAIN_LIFECYCLE: &str = "domain-lifecycle";
const SUBJECT_DOMAIN_CAPABILITY: &str = "domain-capability";
const SUBJECT_PROVIDER_TURN: &str = "provider-turn";
const SUBJECT_TOOL_LOOP: &str = "tool-loop";
const SUBJECT_CONTEXT_PROJECTION: &str = "context-projection";
const SUBJECT_USER_CONFIG: &str = "user-config";
const SUBJECT_GODOT_SCENE_RESOLVE: &str = "godot-scene-resolve";
const SUBJECT_GODOT_DISCOVERY: &str = "godot-discovery";
const SUBJECT_GODOT_KNOWLEDGE: &str = "godot-knowledge";
const SUBJECT_GODOT_DIAGNOSTICS: &str = "godot-diagnostics";
const SUBJECT_GODOT_LSP: &str = "godot-lsp";
const CORPUS_SCHEMA_VERSION: u64 = 3;
const CORPUS_VERSION: u64 = 15;
const MAX_LANGUAGE_INPUT_BYTES: usize = 64 * 1024;
const MAX_DOMAIN_INPUT_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_INPUT_BYTES: usize = 64 * 1024;
const MAX_TOOL_LOOP_INPUT_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_PROJECTION_INPUT_BYTES: usize = 64 * 1024;
const MAX_USER_CONFIG_INPUT_BYTES: usize = 64 * 1024;
const MAX_GODOT_INPUT_BYTES: usize = 64 * 1024;
const MAX_TASK_INPUT_BYTES: usize = 8 * 1024;
const MAX_WORKSPACE_INPUT_BYTES: usize = 64 * 1024;
const RUNNER_PROTOCOL_SCHEMA_VERSION: u64 = 1;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_SCENARIO_BYTES: usize = 16 * 1024;
const MAX_SCENARIOS: usize = 256;
const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_FILE_NAME_BYTES: usize = 160;
const MAX_ENV_ENTRIES: usize = 16;
const MAX_ENV_KEY_BYTES: usize = 64;
const MAX_ENV_VALUE_BYTES: usize = 4 * 1024;
const MAX_PROBE_OUTPUT_BYTES: usize = 16 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(5);
const PROBE_ERROR_MARKER: &[u8] = b"ERR";

/// Stable category for an exit-2 harness failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessErrorKind {
    /// Corpus bytes or structure violate the versioned protocol.
    Corpus,
    /// A required repository input could not be read or decoded.
    Input,
    /// The probe process could not be created.
    ProbeSpawn,
    /// The probe exceeded its deterministic deadline.
    ProbeTimeout,
    /// The probe exited unsuccessfully or was terminated.
    ProbeExit,
    /// The probe emitted an invalid or over-limit outcome.
    ProbeProtocol,
}

/// A harness failure, distinct from a behavioral parity deviation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError {
    kind: HarnessErrorKind,
    code: String,
    detail: String,
}

impl HarnessError {
    /// Stable machine-branchable error category.
    pub fn kind(&self) -> HarnessErrorKind {
        self.kind
    }

    /// Stable code used by the runner-process protocol.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Stable category separating fixture integrity from runner failures.
    pub fn category(&self) -> &'static str {
        match self.kind {
            HarnessErrorKind::Corpus => "CORPUS_INTEGRITY_FAILURE",
            _ => "HARNESS_INTERNAL_FAILURE",
        }
    }

    fn new(kind: HarnessErrorKind, detail: impl Into<String>) -> Self {
        let code = match kind {
            HarnessErrorKind::Corpus => "MALFORMED_CORPUS",
            HarnessErrorKind::Input => "REPOSITORY_INPUT_FAILURE",
            HarnessErrorKind::ProbeSpawn => "PROBE_SPAWN_FAILURE",
            HarnessErrorKind::ProbeTimeout => "PROBE_TIMEOUT",
            HarnessErrorKind::ProbeExit => "PROBE_PROCESS_CRASHED",
            HarnessErrorKind::ProbeProtocol => "PROBE_PROTOCOL_ERROR",
        };
        Self { kind, code: code.to_owned(), detail: detail.into() }
    }

    fn with_code(
        kind: HarnessErrorKind,
        code: &str,
        detail: impl Into<String>,
    ) -> Self {
        Self { kind, code: code.to_owned(), detail: detail.into() }
    }

    fn corpus(detail: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Corpus, detail)
    }

    fn input(detail: impl Into<String>) -> Self {
        Self::new(HarnessErrorKind::Input, detail)
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for HarnessError {}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u64,
    #[serde(rename = "corpusVersion")]
    corpus_version: u64,
    #[serde(rename = "corpusSha256")]
    corpus_sha256: Option<String>,
    scenarios: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    file: String,
    sha256: Option<String>,
}

/// One validated corpus scenario (public for the nightly fuzz crate).
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    id: String,
    subject: String,
    platforms: Vec<String>,
    parity: String,
    env: BTreeMap<String, String>,
    /// Subject-specific inputs (task-contract scenarios only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<Value>,
}

struct LoadedScenario {
    scenario: Scenario,
    applicable: bool,
}

/// Current-host platform value used by scenario applicability.
pub fn platform_name() -> &'static str {
    if cfg!(windows) { PLATFORM_WINDOWS } else { PLATFORM_POSIX }
}

fn read_bounded_utf8(
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

fn validate_scenario(
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
            | SUBJECT_CHECKPOINT
            | SUBJECT_GIT_INSPECTION
            | SUBJECT_LANGUAGE_DIAGNOSTICS
            | SUBJECT_LANGUAGE_STRUCTURE
            | SUBJECT_LANGUAGE_DEFINITION
            | SUBJECT_DOMAIN_LIFECYCLE
            | SUBJECT_DOMAIN_CAPABILITY
            | SUBJECT_PROVIDER_TURN
            | SUBJECT_TOOL_LOOP
            | SUBJECT_CONTEXT_PROJECTION
            | SUBJECT_USER_CONFIG
            | SUBJECT_GODOT_SCENE_RESOLVE
            | SUBJECT_GODOT_DISCOVERY
            | SUBJECT_GODOT_KNOWLEDGE
            | SUBJECT_GODOT_DIAGNOSTICS
            | SUBJECT_GODOT_LSP
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
        | SUBJECT_CHECKPOINT
        | SUBJECT_GIT_INSPECTION
        | SUBJECT_LANGUAGE_DIAGNOSTICS
        | SUBJECT_LANGUAGE_STRUCTURE
        | SUBJECT_LANGUAGE_DEFINITION
        | SUBJECT_DOMAIN_LIFECYCLE
        | SUBJECT_DOMAIN_CAPABILITY
        | SUBJECT_PROVIDER_TURN
        | SUBJECT_TOOL_LOOP
        | SUBJECT_CONTEXT_PROJECTION
        | SUBJECT_USER_CONFIG => {
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
                SUBJECT_DOMAIN_LIFECYCLE | SUBJECT_DOMAIN_CAPABILITY
            );
            let context_projection_subject =
                scenario.subject.as_str() == SUBJECT_CONTEXT_PROJECTION;
            let user_config_subject =
                scenario.subject.as_str() == SUBJECT_USER_CONFIG;
            if user_config_subject {
                validate_user_config_input(input)?;
            }
            let max_input_bytes = if provider_subject {
                MAX_PROVIDER_INPUT_BYTES
            } else if tool_loop_subject {
                MAX_TOOL_LOOP_INPUT_BYTES
            } else if context_projection_subject {
                MAX_CONTEXT_PROJECTION_INPUT_BYTES
            } else if user_config_subject {
                MAX_USER_CONFIG_INPUT_BYTES
            } else if language_subject {
                MAX_LANGUAGE_INPUT_BYTES
            } else if domain_subject {
                MAX_DOMAIN_INPUT_BYTES
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
        SUBJECT_GODOT_SCENE_RESOLVE
        | SUBJECT_GODOT_DISCOVERY
        | SUBJECT_GODOT_KNOWLEDGE
        | SUBJECT_GODOT_DIAGNOSTICS
        | SUBJECT_GODOT_LSP => {
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
            if serialized.len() > MAX_GODOT_INPUT_BYTES {
                return Err(HarnessError::corpus(format!(
                    "scenario {} input exceeds {MAX_GODOT_INPUT_BYTES} bytes",
                    scenario.id
                )));
            }
            validate_godot_input(&scenario.subject, input)?;
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

fn canonical_scenario_digest(
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

fn load_corpus(
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

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    // sha2 0.11 returns a hybrid-array Array that no longer implements
    // LowerHex; format the digest bytes explicitly. The produced hex
    // string is byte-identical to the previous LowerHex output.
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

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

fn execute_bounded_child(
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

fn state_dir_record(scenario_id: &str, stdout: &[u8]) -> Value {
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

fn cargo_workspace_version(root: &Path) -> Result<String, HarnessError> {
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
        SUBJECT_GODOT_SCENE_RESOLVE
        | SUBJECT_GODOT_DISCOVERY
        | SUBJECT_GODOT_KNOWLEDGE
        | SUBJECT_GODOT_DIAGNOSTICS
        | SUBJECT_GODOT_LSP => {
            let input = scenario
                .input
                .as_ref()
                .expect("godot input was validated while loading the corpus");
            let result = godot_record(&scenario.subject, input)?;
            Ok(
                json!({"scenarioId": scenario.id, "subject": scenario.subject, "outcome": "COMPLETED", "result": result}),
            )
        }
        _ => unreachable!("subject was validated while loading the corpus"),
    }
}

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Stage 3R R7.4 subject: user-config.
// ---------------------------------------------------------------------------

fn user_config_record(input: &Value) -> Result<Value, HarnessError> {
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
                    crate::configuration::diagnose_user_configuration(Some(
                        &path,
                    ))
                    .map_err(|error| HarnessError::input(error.to_string()))?;
                match crate::configuration::load_user_configuration(Some(
                    &path,
                )) {
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

const GODOT_PLATFORMS: [&str; 3] = ["win32", "linux", "darwin"];

/// Schema-level validation for one Godot scenario input. Mirrors the
/// JS contract: platforms `["*"]`, empty env, a plain object bounded by
/// `MAX_GODOT_INPUT_BYTES`, plus per-subject key and type checks.
fn validate_godot_input(
    subject: &str,
    input: &Value,
) -> Result<(), HarnessError> {
    let reject = |message: String| {
        Err(HarnessError::corpus(format!(
            "scenario input rejected for subject {subject}: {message}"
        )))
    };
    let string_at = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("field {key} must be a string"))
    };
    if let Some(platform) = input.get("platform") {
        let Some(text) = platform.as_str() else {
            return reject("platform must be a string".to_owned());
        };
        if !GODOT_PLATFORMS.contains(&text) {
            return reject(format!("platform {text} is not a Godot platform"));
        }
    }
    match subject {
        SUBJECT_GODOT_SCENE_RESOLVE => {
            const KEYS: [&str; 3] = ["tscn", "tres", "path"];
            for key in input.as_object().into_iter().flat_map(|map| map.keys())
            {
                if !KEYS.contains(&key.as_str()) {
                    return reject(format!("unexpected field {key}"));
                }
            }
            for key in ["tscn", "tres"] {
                let value = input.get(key);
                if value.is_some_and(|value| !value.is_string()) {
                    return reject(format!("field {key} must be a string"));
                }
            }
            if let Ok(path) = string_at("path") {
                if path.len() > 1024 {
                    return reject("field path exceeds its bound".to_owned());
                }
            } else if input.get("path").is_some() {
                return reject("field path must be a string".to_owned());
            }
            Ok(())
        }
        SUBJECT_GODOT_KNOWLEDGE => {
            match input.get("op").and_then(Value::as_str) {
                Some("status") | Some("refresh") => {
                    if let Some(cancelled) = input.get("cancelled") {
                        if !cancelled.is_boolean() {
                            return reject(
                                "cancelled must be a boolean".to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                Some("search") => {
                    if let Err(message) = string_at("query") {
                        return reject(message);
                    }
                    if let Some(kinds) = input.get("kinds") {
                        let Some(entries) = kinds.as_array() else {
                            return reject(
                                "kinds must be an array".to_owned(),
                            );
                        };
                        const KINDS: [&str; 8] = [
                            "class", "method", "property", "signal",
                            "constant", "enum", "utility", "operator",
                        ];
                        if entries.len() > 16
                            || entries.iter().any(|entry| {
                                !entry
                                    .as_str()
                                    .is_some_and(|text| KINDS.contains(&text))
                            })
                        {
                            return reject(
                                "kinds must be at most 16 known kind strings"
                                    .to_owned(),
                            );
                        }
                    }
                    if let Some(limit) = input.get("limit") {
                        if limit.as_u64().is_none_or(|limit| limit > 1000) {
                            return reject(
                                "limit must be an integer in 1..=1000"
                                    .to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                Some("lookup") => {
                    if let Err(message) = string_at("symbol") {
                        return reject(message);
                    }
                    Ok(())
                }
                _ => reject(
                    "op must be status, refresh, search, or lookup".to_owned(),
                ),
            }
        }
        SUBJECT_GODOT_DISCOVERY => {
            match input.get("op").and_then(Value::as_str) {
                Some("discover" | "select") => {}
                _ => {
                    return reject("op must be discover or select".to_owned());
                }
            }
            if let Some(override_source) = input.get("overrideSource") {
                if !override_source.is_null()
                    && override_source.as_str() != Some("cli")
                {
                    return reject(
                        "overrideSource must be null or \"cli\"".to_owned(),
                    );
                }
            }
            for key in ["hostPath", "hostPathExt", "workspaceRoot"] {
                if let Some(value) = input.get(key) {
                    if !value.is_null() && value.as_str().is_none() {
                        return reject(format!(
                            "field {key} must be a string or null"
                        ));
                    }
                }
            }
            let Some(config) = input.get("config") else {
                return Ok(());
            };
            let Some(config) = config.as_object() else {
                return reject("config must be an object".to_owned());
            };
            if let Some(active) = config.get("activeInstallation") {
                if !active.is_null() && active.as_str().is_none() {
                    return reject(
                        "config.activeInstallation must be a string or null"
                            .to_owned(),
                    );
                }
            }
            if let Some(discover_on_path) = config.get("discoverOnPath") {
                if !discover_on_path.is_boolean() {
                    return reject(
                        "config.discoverOnPath must be a boolean".to_owned(),
                    );
                }
            }
            let installations = config.get("installations");
            if installations.is_some_and(|value| !value.is_array()) {
                return reject(
                    "config.installations must be an array".to_owned(),
                );
            }
            for entry in
                installations.and_then(Value::as_array).into_iter().flatten()
            {
                let Some(entry) = entry.as_object() else {
                    return reject(
                        "config.installations entries must be objects"
                            .to_owned(),
                    );
                };
                if let Err(message) = entry
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty() && id.len() <= 128)
                    .ok_or("entry id must be a non-empty bounded string")
                {
                    return reject(message.to_owned());
                }
                if let Err(message) = entry
                    .get("path")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty() && path.len() <= 1024)
                    .ok_or("entry path must be a non-empty bounded string")
                {
                    return reject(message.to_owned());
                }
                match entry.get("editionHint").and_then(Value::as_str) {
                    None | Some("standard") | Some("dotnet")
                    | Some("unknown") => {}
                    Some(hint) => {
                        return reject(format!(
                            "entry editionHint {hint} is not standard, dotnet, or unknown"
                        ));
                    }
                }
            }
            Ok(())
        }
        SUBJECT_GODOT_DIAGNOSTICS => {
            match input.get("op").and_then(Value::as_str) {
                Some("support" | "status") => Ok(()),
                Some("prepare") => {
                    if let Some(paths) = input.get("paths") {
                        if paths.is_null() {
                            return Ok(());
                        }
                        let Some(entries) = paths.as_array() else {
                            return reject(
                                "paths must be an array or null".to_owned(),
                            );
                        };
                        if entries.len() > 64
                            || entries.iter().any(|entry| !entry.is_string())
                        {
                            return reject(
                                "paths must be at most 64 strings".to_owned(),
                            );
                        }
                    }
                    if let Some(cancelled) = input.get("cancelled") {
                        if !cancelled.is_boolean() {
                            return reject(
                                "cancelled must be a boolean".to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                Some("execute") => {
                    if let Some(digest) = input.get("approvedDigest") {
                        let Some(text) = digest.as_str() else {
                            return reject(
                                "approvedDigest must be a string".to_owned(),
                            );
                        };
                        if text.len() != 64
                            || !text
                                .bytes()
                                .all(|byte| byte.is_ascii_hexdigit())
                        {
                            return reject(
                                "approvedDigest must be a hex SHA-256 digest"
                                    .to_owned(),
                            );
                        }
                    }
                    Ok(())
                }
                _ => reject(
                    "op must be support, prepare, execute, or status"
                        .to_owned(),
                ),
            }
        }
        SUBJECT_GODOT_LSP => match input.get("op").and_then(Value::as_str) {
            Some("support" | "status") => Ok(()),
            Some("prepare") => {
                if let Some(cancelled) = input.get("cancelled") {
                    if !cancelled.is_boolean() {
                        return reject(
                            "cancelled must be a boolean".to_owned(),
                        );
                    }
                }
                Ok(())
            }
            _ => reject("op must be support, prepare, or status".to_owned()),
        },
        _ => unreachable!("godot subject was validated above"),
    }
}

fn godot_record(subject: &str, input: &Value) -> Result<Value, HarnessError> {
    match subject {
        SUBJECT_GODOT_SCENE_RESOLVE => godot_scene_resolve_record(input),
        SUBJECT_GODOT_DISCOVERY => godot_discovery_record(input),
        SUBJECT_GODOT_KNOWLEDGE => godot_knowledge_record(input),
        SUBJECT_GODOT_DIAGNOSTICS => godot_diagnostics_record(input),
        SUBJECT_GODOT_LSP => godot_lsp_record(input),
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
    use siralos_core::godot::scene::{
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
    use siralos_adapters::godot::knowledge::GodotKnowledgeService;
    use siralos_core::godot::{
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
) -> Option<siralos_core::godot::GodotApiSearchKind> {
    use siralos_core::godot::GodotApiSearchKind;
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
    status: siralos_core::godot::KnowledgeRefreshStatus,
) -> &'static str {
    use siralos_core::godot::KnowledgeRefreshStatus;
    match status {
        KnowledgeRefreshStatus::Unavailable => "unavailable",
        KnowledgeRefreshStatus::Unsupported => "unsupported",
        KnowledgeRefreshStatus::Failed => "failed",
        KnowledgeRefreshStatus::Cancelled => "cancelled",
    }
}

fn query_status_str(
    status: siralos_core::godot::KnowledgeQueryStatus,
) -> &'static str {
    use siralos_core::godot::KnowledgeQueryStatus;
    match status {
        KnowledgeQueryStatus::Unavailable => "unavailable",
        KnowledgeQueryStatus::InvalidInput => "invalid_input",
        KnowledgeQueryStatus::Cancelled => "cancelled",
    }
}

fn lookup_status_str(
    status: siralos_core::godot::KnowledgeLookupStatus,
) -> &'static str {
    use siralos_core::godot::KnowledgeLookupStatus;
    match status {
        KnowledgeLookupStatus::NotFound => "not_found",
        KnowledgeLookupStatus::Unavailable => "unavailable",
        KnowledgeLookupStatus::InvalidInput => "invalid_input",
        KnowledgeLookupStatus::Cancelled => "cancelled",
    }
}

fn check_run_status_str(
    status: siralos_core::godot::GodotProjectCheckRunStatus,
) -> &'static str {
    use siralos_core::godot::GodotProjectCheckRunStatus;
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
    state: siralos_core::godot::GodotDiagnosticsState,
) -> &'static str {
    use siralos_core::godot::GodotDiagnosticsState;
    match state {
        GodotDiagnosticsState::Untrusted => "untrusted",
        GodotDiagnosticsState::CheckInvalidated => "check-invalidated",
    }
}

fn godot_diagnostics_record(input: &Value) -> Result<Value, HarnessError> {
    use siralos_adapters::godot::diagnostics::GodotDiagnosticsService;
    use siralos_core::godot::{
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
    use siralos_adapters::godot::lsp::GodotLspService;
    use siralos_core::godot::{
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
    use siralos_adapters::config::{
        UserGodotConfig, UserGodotEditionHint, UserGodotInstallationConfig,
    };
    use siralos_adapters::godot::profile::engine_profiler::{
        GodotOverrideSource, GodotProfilerInputs, discover, selected_profile,
    };
    use siralos_core::godot::{
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
        overview: &siralos_core::godot::GodotInstallationOverview,
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

#[cfg(test)]
mod godot_tests {
    use super::godot_record;
    use serde_json::{Value, json};

    fn record(input: Value) -> Value {
        godot_record("godot-scene-resolve", &input)
            .expect("scene-resolve record succeeds")
    }

    #[test]
    fn scene_resolve_transcribes_the_oracle_selection_rule() {
        // Null tres is a MISSING key, never a resource marker.
        assert_eq!(
            record(json!({"tres": null, "tscn": "[gd_scene]\n"})),
            json!({"status": "complete", "diagnostics": 0, "truncated": false})
        );
        // Unknown keys are ignored; empty scene text parses on default path "".
        assert_eq!(
            record(json!({"content": "[gd_resource type=\"Resource\"]"})),
            json!({"status": "invalid", "diagnostics": 0, "truncated": false})
        );
        // tres wins over tscn.
        assert_eq!(
            record(json!({"tres": "[gd_resource]", "tscn": "[gd_scene]"})),
            json!({"status": "partial", "diagnostics": 1, "truncated": false})
        );
        // A resource body under tscn parses as a resource; the typed
        // header alone is complete, while an untyped header is partial.
        assert_eq!(
            record(json!({"tscn": "[gd_resource type=\"Resource\"]"})),
            json!({"status": "complete", "diagnostics": 0, "truncated": false})
        );
    }

    #[test]
    fn knowledge_search_validates_before_availability() {
        let invalid = godot_record(
            "godot-knowledge",
            &json!({"op": "search", "query": "   "}),
        )
        .unwrap();
        assert_eq!(
            invalid,
            json!({"status": "invalid_input", "message": "A non-empty query is required."})
        );
        let unavailable = godot_record(
            "godot-knowledge",
            &json!({"op": "search", "query": "node"}),
        )
        .unwrap();
        assert_eq!(unavailable["status"], "unavailable");
        assert_eq!(
            unavailable["message"],
            "No Godot API knowledge is loaded: exact-engine API generation is unavailable on this platform."
        );
    }

    #[test]
    fn knowledge_status_reports_unavailable_platform_facts() {
        let status = godot_record(
            "godot-knowledge",
            &json!({"op": "status", "platform": "linux"}),
        )
        .unwrap();
        assert_eq!(status["state"], "unavailable");
        assert_eq!(status["platform"], "linux");
        assert_eq!(status["cacheEnabled"], false);
        assert_eq!(status["schemaVersion"], 1);
    }

    #[test]
    fn diagnostics_prepare_refuses_before_effects() {
        let prepared = godot_record(
            "godot-diagnostics",
            &json!({"op": "prepare", "paths": ["src/player.gd"]}),
        )
        .unwrap();
        assert_eq!(
            prepared,
            json!({
                "status": "unsupported",
                "message": "No trusted Godot installation is selected; GDScript diagnostics cannot run."
            })
        );
        let cancelled = godot_record(
            "godot-diagnostics",
            &json!({"op": "prepare", "cancelled": true}),
        )
        .unwrap();
        assert_eq!(
            cancelled,
            json!({"status": "cancelled", "message": "The Godot project operation was aborted."})
        );
        let executed =
            godot_record("godot-diagnostics", &json!({"op": "execute"}))
                .unwrap();
        assert_eq!(
            executed,
            json!({
                "status": "failed",
                "message": "The prepared check is not valid for this session; prepare a new check."
            })
        );
    }

    #[test]
    fn lsp_prepare_refuses_and_status_stays_empty() {
        let prepared =
            godot_record("godot-lsp", &json!({"op": "prepare"})).unwrap();
        assert_eq!(
            prepared,
            json!({
                "status": "unsupported",
                "message": "No trusted Godot installation is selected; the language session cannot start."
            })
        );
        let status =
            godot_record("godot-lsp", &json!({"op": "status"})).unwrap();
        assert_eq!(
            status,
            json!({
                "state": "unavailable",
                "openDocumentCount": 0,
                "diagnosticCount": 0,
                "networkIsolation": "unavailable",
            })
        );
    }

    #[test]
    fn discovery_reports_missing_explicit_paths_fail_closed() {
        let explicit = godot_record(
            "godot-discovery",
            &json!({
                "op": "select",
                "preference": {"path": "/no-such-place/godot.exe"}
            }),
        )
        .unwrap();
        assert_eq!(explicit["ok"], false);
        assert!(
            explicit["error"]
                .as_str()
                .is_some_and(|message| message.contains("did not resolve"))
        );
    }

    #[test]
    fn discovery_auto_with_no_candidates_stays_deterministic() {
        let discovered = godot_record(
            "godot-discovery",
            &json!({
                "op": "discover",
                "hostPath": "/no-such-a:/no-such-b",
                "config": {"discoverOnPath": true}
            }),
        )
        .unwrap();
        assert_eq!(discovered["ok"], true);
        assert_eq!(discovered["selected"], Value::Null);
        assert_eq!(discovered["candidates"], json!([]),);
        assert_eq!(
            discovered["configuration"],
            json!({
                "activeInstallation": null,
                "configuredCount": 0,
                "discoverOnPath": true,
                "overrides": [],
            })
        );
        assert_eq!(
            discovered["rationale"][0],
            "No selectable Godot installation was discovered."
        );
        assert_eq!(discovered["diagnostics"], json!([]));
    }

    #[test]
    fn discovery_configured_missing_installation_is_invalid_not_selected() {
        let discovered = godot_record(
            "godot-discovery",
            &json!({
                "op": "discover",
                "config": {
                    "installations": [
                        {"id": "cfg-1", "path": "/missing/Godot.exe", "editionHint": "standard"}
                    ]
                }
            }),
        )
        .unwrap();
        assert_eq!(discovered["ok"], true);
        assert_eq!(discovered["selected"], Value::Null);
        let candidate = &discovered["candidates"][0];
        assert_eq!(candidate["id"], "cfg-1");
        assert_eq!(candidate["source"], "user-config");
        assert_eq!(candidate["selected"], false);
        assert!(
            candidate["invalid"]
                .as_str()
                .is_some_and(|message| message.contains("does not exist"))
        );
    }
}

// Stage 3R R7.1 subject: provider-turn.
// ---------------------------------------------------------------------------

/// Canonical provider-turn record: one canonical observation per input
/// case (turn and/or detach).
fn provider_turn_record(input: &Value) -> Result<Value, HarnessError> {
    let mut records = Vec::new();
    for case in scenario_array(input, "cases")? {
        let record = match (case.get("turn"), case.get("detach")) {
            (Some(turn), None) => {
                json!({ "turn": run_provider_turn_case(turn)? })
            }
            (None, Some(detach)) => {
                json!({ "detach": run_provider_detach_case(detach)? })
            }
            _ => {
                return Err(HarnessError::corpus(
                    "provider-turn case must have exactly one of turn/detach",
                ));
            }
        };
        records.push(record);
    }
    Ok(json!({ "cases": records }))
}

/// Run one provider-turn case through the production core collector.
fn run_provider_turn_case(case: &Value) -> Result<Value, HarnessError> {
    let case = materialize_value(case)?;
    let provider_spec = case.get("provider").ok_or_else(|| {
        HarnessError::corpus("provider-turn turn case requires a provider")
    })?;
    let kind = scenario_string(provider_spec, "kind")?;
    let messages =
        parse_conversation_items(scenario_array(&case, "messages")?)?;
    let tools = parse_tool_definitions(scenario_array(&case, "tools")?)?;
    // The provider request system is Host-projection-owned (R7.3); the
    // R7.1 differential input does not carry it.
    let system = None;
    let cancel_after =
        scenario_u64(&case, "cancelAfterEvents").map(|value| value as usize);
    let token = CancellationToken::new();
    let outcome = match kind.as_str() {
        "fake" => {
            let provider = DeterministicFakeProvider::new();
            collect_with_cancellation_script(
                &provider,
                &messages,
                &tools,
                system,
                &token,
                cancel_after,
            )
        }
        "scripted" => {
            let events = scenario_array(provider_spec, "events")?;
            let provider = ScriptedProvider { events: events.clone() };
            collect_with_cancellation_script(
                &provider,
                &messages,
                &tools,
                system,
                &token,
                cancel_after,
            )
        }
        _ => {
            return Err(HarnessError::corpus(
                "provider-turn provider kind must be fake or scripted",
            ));
        }
    };
    Ok(provider_turn_value(outcome))
}

/// Run one tool-result detach case through the production core boundary.
fn run_provider_detach_case(detach: &Value) -> Result<Value, HarnessError> {
    let detach = materialize_value(detach)?;
    let value = detach.get("value").ok_or_else(|| {
        HarnessError::corpus("provider-turn detach case requires a value")
    })?;
    let max_bytes =
        detach.get("maxBytes").and_then(Value::as_u64).ok_or_else(|| {
            HarnessError::corpus(
                "provider-turn detach case requires an integer maxBytes",
            )
        })? as usize;
    let actor = scenario_string(&detach, "actor")?;
    match detach_bounded_tool_result(value, max_bytes) {
        Ok((result, byte_length)) => Ok(json!({
            "ok": true,
            "result": tool_execution_result_value(&result),
            "byteLength": byte_length,
        })),
        Err(failure) => {
            Ok(json!({ "ok": false, "message": failure.message(&actor) }))
        }
    }
}

/// The canonical turn observation for one outcome.
fn provider_turn_value(outcome: TurnOutcome) -> Value {
    match outcome {
        TurnOutcome::Cancelled => json!({ "kind": "cancelled" }),
        TurnOutcome::Failed { failure } => json!({
            "kind": "failed",
            "failure": provider_failure_code(&failure),
            "message": failure.application_message(),
        }),
        TurnOutcome::Turn { assistant_text, text_deltas, tool_calls } => {
            json!({
                "kind": "turn",
                "assistantText": assistant_text,
                "textDeltas": text_deltas,
                "toolCalls": tool_calls
                    .iter()
                    .map(tool_call_value)
                    .collect::<Vec<_>>(),
            })
        }
    }
}

/// Stable failure category for a typed turn failure.
fn provider_failure_code(failure: &TurnFailure) -> &'static str {
    match failure {
        TurnFailure::LimitExceeded(LimitClass::AssistantTextBytes) => {
            "LIMIT_ASSISTANT_TEXT_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::TextEventCount) => {
            "LIMIT_TEXT_EVENT_COUNT"
        }
        TurnFailure::LimitExceeded(LimitClass::ToolCallCount) => {
            "LIMIT_TOOL_CALL_COUNT"
        }
        TurnFailure::LimitExceeded(LimitClass::CallIdBytes) => {
            "LIMIT_CALL_ID_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::ToolNameBytes) => {
            "LIMIT_TOOL_NAME_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::ToolArgumentBytes) => {
            "LIMIT_TOOL_ARGUMENT_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::AggregateTurnBytes) => {
            "LIMIT_AGGREGATE_TURN_BYTES"
        }
        TurnFailure::EventAfterCompletion => "EVENT_AFTER_COMPLETION",
        TurnFailure::EofWithoutCompletion => "EOF_WITHOUT_COMPLETION",
        TurnFailure::Protocol(ProtocolFailure::UnknownEventType) => {
            "UNKNOWN_EVENT_TYPE"
        }
        TurnFailure::Protocol(ProtocolFailure::MalformedEvent) => {
            "MALFORMED_EVENT"
        }
        TurnFailure::Protocol(ProtocolFailure::MalformedTextEvent) => {
            "MALFORMED_TEXT_EVENT"
        }
        TurnFailure::Protocol(ProtocolFailure::MalformedToolCall) => {
            "MALFORMED_TOOL_CALL"
        }
        TurnFailure::Protocol(ProtocolFailure::InvalidToolArgumentJson) => {
            "INVALID_TOOL_ARGUMENT_JSON"
        }
        TurnFailure::ProviderFailed(_) => "PROVIDER_FAILED",
        TurnFailure::InvalidTranscript(_) => "INVALID_TRANSCRIPT",
    }
}

/// The canonical tool-call proposal observation.
fn tool_call_value(call: &TurnToolCall) -> Value {
    match call {
        TurnToolCall::Execute { call_id, tool_name, input } => json!({
            "kind": "execute",
            "callId": call_id,
            "toolName": tool_name,
            "input": input.value(),
        }),
        TurnToolCall::Invalid { call_id, tool_name, message } => json!({
            "kind": "invalid",
            "callId": call_id,
            "toolName": tool_name,
            "message": message,
        }),
    }
}

/// The canonical typed tool-result value.
fn tool_execution_result_value(result: &ToolExecutionResult) -> Value {
    match result {
        ToolExecutionResult::Success { output, summary } => json!({
            "status": "success",
            "output": output,
            "summary": summary,
        }),
        other => json!({
            "status": other.status_str(),
            "message": other.message(),
        }),
    }
}

/// Collect one turn, optionally wrapping the provider stream with a
/// deterministic host-scripted cancellation point.
fn collect_with_cancellation_script<P: ModelProvider>(
    provider: &P,
    messages: &[ConversationItem],
    tools: &[ToolDefinition],
    system: Option<String>,
    token: &CancellationToken,
    cancel_after: Option<usize>,
) -> TurnOutcome {
    match cancel_after {
        Some(cancel_after) => {
            // The wrapper holds the Host controller: the cancellation
            // point is Host/test-harness authority. The wrapped provider
            // receives only the read-only observation signal.
            let provider = CancelAfterProvider {
                inner: provider,
                controller: token,
                cancel_after,
            };
            collect_provider_turn(&provider, messages, tools, system, token)
        }
        None => {
            collect_provider_turn(provider, messages, tools, system, token)
        }
    }
}

/// Harness-local host cancellation wrapper: cancels the Host controller
/// after exactly 'cancel_after' events have been emitted (0 = before the
/// first event). The production collector and controller decide the
/// outcome; the wrapped provider receives only the read-only
/// 'CancellationSignal' observation view.
struct CancelAfterProvider<'a, P: ModelProvider> {
    inner: &'a P,
    controller: &'a CancellationToken,
    cancel_after: usize,
}

impl<P: ModelProvider> ModelProvider for CancelAfterProvider<'_, P> {
    type Stream<'a>
        = CancelAfterStream<'a, P::Stream<'a>>
    where
        Self: 'a;

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        CancelAfterStream {
            inner: self.inner.stream(request, cancellation),
            controller: self.controller,
            emitted: 0,
            cancel_after: self.cancel_after,
        }
    }
}

struct CancelAfterStream<'a, S> {
    inner: S,
    controller: &'a CancellationToken,
    emitted: usize,
    cancel_after: usize,
}

impl<S: Iterator<Item = ProviderEvent>> Iterator for CancelAfterStream<'_, S> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        if self.emitted == self.cancel_after {
            self.controller.cancel();
            return None;
        }
        let event = self.inner.next()?;
        self.emitted += 1;
        Some(event)
    }
}

/// Harness-local scripted provider: yields untrusted raw events that the
/// production collector validates through the real trust boundary.
struct ScriptedProvider {
    events: Vec<Value>,
}

impl ModelProvider for ScriptedProvider {
    type Stream<'a>
        = ScriptedStream<'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "scripted-provider"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        ScriptedStream { events: self.events.iter() }
    }
}

struct ScriptedStream<'a> {
    events: std::slice::Iter<'a, Value>,
}

impl Iterator for ScriptedStream<'_> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        self.events.next().map(|raw| ProviderEvent::Raw(raw.clone()))
    }
}

/// Materialize deterministic '$repeat' fixture markers into strings
/// (mirroring the oracle probe), recursively. A marker object
/// {"$repeat": {"character": <one scalar>, "count": N}} becomes the
/// character repeated N times.
fn materialize_value(value: &Value) -> Result<Value, HarnessError> {
    if let Some(object) = value.as_object() {
        if let Some(repeat) = object.get("$eventsRepeat") {
            let repeat_object = repeat.as_object().ok_or_else(|| {
                HarnessError::corpus("$eventsRepeat marker must be an object")
            })?;
            let events = repeat_object
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$eventsRepeat marker requires an events array",
                    )
                })?;
            let count = repeat_object
                .get("count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$eventsRepeat marker requires an integer count",
                    )
                })? as usize;
            if count > 4096 || events.len().saturating_mul(count) > 4096 {
                return Err(HarnessError::corpus(
                    "$eventsRepeat expands beyond the 4096-event bound",
                ));
            }
            let materialized = events
                .iter()
                .map(materialize_value)
                .collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::with_capacity(materialized.len() * count);
            for _ in 0..count {
                out.extend(materialized.iter().cloned());
            }
            return Ok(Value::Array(out));
        }
        if let Some(repeat) = object.get("$repeat") {
            let repeat_object = repeat.as_object().ok_or_else(|| {
                HarnessError::corpus("$repeat marker must be an object")
            })?;
            let character = repeat_object
                .get("character")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$repeat marker requires a string character",
                    )
                })?;
            if character.chars().count() != 1 {
                return Err(HarnessError::corpus(
                    "$repeat character must be a single Unicode scalar value",
                ));
            }
            let count = repeat_object
                .get("count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$repeat marker requires an integer count",
                    )
                })? as usize;
            if count > 1_048_576 {
                return Err(HarnessError::corpus(
                    "$repeat count exceeds the materialization bound",
                ));
            }
            return Ok(Value::String(character.repeat(count)));
        }
        let mut out = serde_json::Map::new();
        for (key, child) in object {
            out.insert(key.clone(), materialize_value(child)?);
        }
        return Ok(Value::Object(out));
    }
    if let Some(array) = value.as_array() {
        let entries = array
            .iter()
            .map(materialize_value)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Value::Array(entries));
    }
    Ok(value.clone())
}

/// Parse conversation items from validated fixture JSON.
fn parse_conversation_items(
    items: &[Value],
) -> Result<Vec<ConversationItem>, HarnessError> {
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        let item = match scenario_string(item, "type")?.as_str() {
            "user_message" => ConversationItem::UserMessage {
                content: scenario_string(item, "content")?,
            },
            "assistant_message" => ConversationItem::AssistantMessage {
                content: scenario_string(item, "content")?,
            },
            "assistant_tool_call" => ConversationItem::AssistantToolCall {
                call_id: scenario_string(item, "callId")?,
                tool_name: scenario_string(item, "toolName")?,
                input: AssistantToolCallInput::Present(
                    item.get("input").cloned().ok_or_else(|| {
                        HarnessError::corpus(
                            "assistant_tool_call item requires an input",
                        )
                    })?,
                ),
            },
            "tool_result" => ConversationItem::ToolResult {
                call_id: scenario_string(item, "callId")?,
                tool_name: scenario_string(item, "toolName")?,
                result: parse_tool_result(item.get("result").ok_or_else(
                    || {
                        HarnessError::corpus(
                            "tool_result item requires a result",
                        )
                    },
                )?)?,
            },
            other => {
                return Err(HarnessError::corpus(format!(
                    "provider-turn conversation item has an unknown type {other}"
                )));
            }
        };
        parsed.push(item);
    }
    Ok(parsed)
}

/// Parse a typed tool-result value from fixture JSON.
fn parse_tool_result(
    value: &Value,
) -> Result<ToolExecutionResult, HarnessError> {
    let object = value.as_object().ok_or_else(|| {
        HarnessError::corpus("tool result must be an object")
    })?;
    let status =
        object.get("status").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("tool result requires a string status")
        })?;
    let string_field = |key: &str| {
        object.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(
            || {
                HarnessError::corpus(format!(
                    "tool result requires a string {key}"
                ))
            },
        )
    };
    let result = match status {
        "success" => ToolExecutionResult::Success {
            output: object.get("output").cloned().ok_or_else(|| {
                HarnessError::corpus("success tool result requires an output")
            })?,
            summary: string_field("summary")?,
        },
        "invalid_input" => ToolExecutionResult::InvalidInput {
            message: string_field("message")?,
        },
        "denied" => {
            ToolExecutionResult::Denied { message: string_field("message")? }
        }
        "conflict" => {
            ToolExecutionResult::Conflict { message: string_field("message")? }
        }
        "failed" => {
            ToolExecutionResult::Failed { message: string_field("message")? }
        }
        "cancelled" => ToolExecutionResult::Cancelled {
            message: string_field("message")?,
        },
        "timed_out" => {
            ToolExecutionResult::TimedOut { message: string_field("message")? }
        }
        "output_limit" => ToolExecutionResult::OutputLimit {
            message: string_field("message")?,
        },
        "sandbox_denied" => ToolExecutionResult::SandboxDenied {
            message: string_field("message")?,
        },
        "sandbox_unavailable" => ToolExecutionResult::SandboxUnavailable {
            message: string_field("message")?,
        },
        "workspace_violation" => ToolExecutionResult::WorkspaceViolation {
            message: string_field("message")?,
        },
        "unavailable" => ToolExecutionResult::Unavailable {
            message: string_field("message")?,
        },
        other => {
            return Err(HarnessError::corpus(format!(
                "tool result has an unknown status {other}"
            )));
        }
    };
    Ok(result)
}

/// Parse tool definitions from validated fixture JSON.
fn parse_tool_definitions(
    tools: &[Value],
) -> Result<Vec<ToolDefinition>, HarnessError> {
    let mut parsed = Vec::with_capacity(tools.len());
    for tool in tools {
        parsed.push(ToolDefinition {
            name: scenario_string(tool, "name")?,
            description: scenario_string(tool, "description")?,
            input_schema: tool.get("inputSchema").cloned().ok_or_else(
                || {
                    HarnessError::corpus(
                        "tool definition requires an inputSchema",
                    )
                },
            )?,
        });
    }
    Ok(parsed)
}

/// Whether a value is a valid '$repeat' materialization marker.
fn is_repeat_marker(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(repeat) = object.get("$repeat") else {
        return false;
    };
    let Some(repeat_object) = repeat.as_object() else {
        return false;
    };
    match repeat_object.get("character") {
        Some(Value::String(character)) if character.chars().count() == 1 => {
            repeat_object
                .get("count")
                .and_then(Value::as_u64)
                .is_some_and(|count| count <= 1_048_576)
        }
        _ => false,
    }
}

/// Strict provider-turn input shape validation (mirrors contract.mjs).
fn validate_provider_turn_input(input: &Value) -> Result<(), HarnessError> {
    let input_object = input.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn input must be an object")
    })?;
    if input_object.len() != 1 || !input_object.contains_key("cases") {
        return Err(HarnessError::corpus(
            "provider-turn input must contain exactly the cases field",
        ));
    }
    let cases =
        input_object.get("cases").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("provider-turn cases must be an array"),
        )?;
    if cases.is_empty() || cases.len() > 32 {
        return Err(HarnessError::corpus(
            "provider-turn cases must contain 1-32 entries",
        ));
    }
    for case in cases {
        validate_provider_turn_case(case)?;
    }
    Ok(())
}

fn validate_provider_turn_case(case: &Value) -> Result<(), HarnessError> {
    let case_object = case.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn cases must contain objects")
    })?;
    let has_turn = case_object.contains_key("turn");
    let has_detach = case_object.contains_key("detach");
    if has_turn == has_detach {
        return Err(HarnessError::corpus(
            "provider-turn case must have exactly one of turn/detach",
        ));
    }
    if has_turn {
        validate_provider_turn_case_input(
            case_object.get("turn").ok_or_else(|| {
                HarnessError::corpus("provider-turn case turn is missing")
            })?,
        )?;
    } else {
        validate_provider_detach_case_input(
            case_object.get("detach").ok_or_else(|| {
                HarnessError::corpus("provider-turn case detach is missing")
            })?,
        )?;
    }
    Ok(())
}

fn validate_provider_turn_case_input(
    case: &Value,
) -> Result<(), HarnessError> {
    let case_object = case.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn turn case must be an object")
    })?;
    for key in case_object.keys() {
        if !matches!(
            key.as_str(),
            "provider" | "messages" | "tools" | "cancelAfterEvents"
        ) {
            return Err(HarnessError::corpus(format!(
                "provider-turn turn case has an unknown field {key}"
            )));
        }
    }
    for required in ["provider", "messages", "tools"] {
        if !case_object.contains_key(required) {
            return Err(HarnessError::corpus(format!(
                "provider-turn turn case requires the {required} field"
            )));
        }
    }
    if let Some(cancel) = case_object.get("cancelAfterEvents") {
        let value = cancel.as_u64().ok_or_else(|| {
            HarnessError::corpus(
                "provider-turn cancelAfterEvents must be an integer",
            )
        })?;
        if value > 1024 {
            return Err(HarnessError::corpus(
                "provider-turn cancelAfterEvents exceeds the bound",
            ));
        }
    }
    let provider = case_object
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            HarnessError::corpus("provider-turn provider must be an object")
        })?;
    match provider.get("kind").and_then(Value::as_str) {
        Some("fake") => {}
        Some("scripted") => {
            let events = provider
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "scripted provider requires an events array",
                    )
                })?;
            if events.len() > 4096 {
                return Err(HarnessError::corpus(
                    "scripted provider events exceed the bound",
                ));
            }
            // Events are untrusted raw data: any JSON value is
            // admissible and the production collector validation decides
            // malformed events.
        }
        _ => {
            return Err(HarnessError::corpus(
                "provider-turn provider kind must be fake or scripted",
            ));
        }
    }
    let messages =
        case_object.get("messages").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("provider-turn messages must be an array"),
        )?;
    if messages.len() > 128 {
        return Err(HarnessError::corpus(
            "provider-turn messages exceed the bound",
        ));
    }
    for item in messages {
        validate_provider_message(item)?;
    }
    let tools =
        case_object.get("tools").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("provider-turn tools must be an array"),
        )?;
    if tools.len() > 128 {
        return Err(HarnessError::corpus(
            "provider-turn tools exceed the bound",
        ));
    }
    for tool in tools {
        let object = tool.as_object().ok_or_else(|| {
            HarnessError::corpus("provider-turn tools must be objects")
        })?;
        for required in ["name", "description", "inputSchema"] {
            if !object.contains_key(required) {
                return Err(HarnessError::corpus(format!(
                    "provider-turn tool requires the {required} field"
                )));
            }
        }
        if !object.get("name").is_some_and(Value::is_string)
            || !object.get("description").is_some_and(Value::is_string)
        {
            return Err(HarnessError::corpus(
                "provider-turn tool name and description must be strings",
            ));
        }
    }
    Ok(())
}

/// Validate one conversation item (strings may be '$repeat' markers).
fn validate_provider_message(item: &Value) -> Result<(), HarnessError> {
    let object = item.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn messages must contain objects")
    })?;
    let valid_string = |value: Option<&Value>| {
        value.is_some_and(|value| value.is_string() || is_repeat_marker(value))
    };
    match object.get("type").and_then(Value::as_str) {
        Some("user_message") | Some("assistant_message") => {
            if !valid_string(object.get("content")) {
                return Err(HarnessError::corpus(
                    "provider-turn message content must be a string or repeat marker",
                ));
            }
        }
        Some("assistant_tool_call") => {
            if !valid_string(object.get("callId"))
                || !valid_string(object.get("toolName"))
            {
                return Err(HarnessError::corpus(
                    "provider-turn tool call requires string callId and toolName",
                ));
            }
            if !object.contains_key("input") {
                return Err(HarnessError::corpus(
                    "provider-turn tool call requires an input",
                ));
            }
        }
        Some("tool_result") => {
            if !valid_string(object.get("callId"))
                || !valid_string(object.get("toolName"))
            {
                return Err(HarnessError::corpus(
                    "provider-turn tool result requires string callId and toolName",
                ));
            }
            if !object.get("result").is_some_and(Value::is_object) {
                return Err(HarnessError::corpus(
                    "provider-turn tool result requires a result object",
                ));
            }
        }
        other => {
            return Err(HarnessError::corpus(format!(
                "provider-turn message has an unknown type {other:?}"
            )));
        }
    }
    Ok(())
}

fn validate_provider_detach_case_input(
    detach: &Value,
) -> Result<(), HarnessError> {
    let object = detach.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn detach case must be an object")
    })?;
    if object.len() != 3
        || !object.contains_key("value")
        || !object.contains_key("maxBytes")
        || !object.contains_key("actor")
    {
        return Err(HarnessError::corpus(
            "provider-turn detach case must contain exactly value, maxBytes, and actor",
        ));
    }
    let max_bytes =
        object.get("maxBytes").and_then(Value::as_u64).ok_or_else(|| {
            HarnessError::corpus(
                "provider-turn detach maxBytes must be an integer",
            )
        })?;
    if max_bytes == 0 || max_bytes > 1_048_576 {
        return Err(HarnessError::corpus(
            "provider-turn detach maxBytes is outside the bound",
        ));
    }
    let actor =
        object.get("actor").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("provider-turn detach actor must be a string")
        })?;
    if actor.is_empty() || actor.len() > 128 {
        return Err(HarnessError::corpus(
            "provider-turn detach actor is outside the bound",
        ));
    }
    Ok(())
}

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

fn canonical_records_text(records: Vec<Value>) -> String {
    let document = json!({
        "schemaVersion": RUNNER_PROTOCOL_SCHEMA_VERSION,
        "records": records,
    });
    let text = serde_json::to_string(&document)
        .expect("outcome records are constructed from serializable values");
    format!("{text}\n")
}

// ---------------------------------------------------------------------------
// Stage 3R R3 subject: task-contract (host-owned task kernel parity).
// ---------------------------------------------------------------------------
//
// Executes each R3 scenario against the real siralos-core task runtime and
// builds the canonical R3 observation object that the TypeScript oracle
// probe emits for the same inputs. The observation is semantic parity data,
// never a TypeScript object layout.

use siralos_core::task::contract::{
    AcceptanceCriterion, ConstraintKind, CreateTaskContractInput, PausePolicy,
    ReviseContext, ReviseTaskContractInput, TaskConstraint, TaskContract,
    VerificationKind,
};
use siralos_core::task::evidence::FindingInput;
use siralos_core::task::model::{
    ActivityEvent, ApprovalDecision, DispositionSource, EvidenceKind,
    EvidenceSource, EvidenceVerification, FindingSeverity, TaskPhase,
    TaskReviewStatus, TaskStepKind, TaskStepSpec, TaskValidationStatus,
    VerificationOutcome, WorkflowDisposition,
};
use siralos_core::task::progress::HostObservation;
use siralos_core::task::runtime::{
    AttachResult, CompletionResult, CreateTaskInput, CriterionResult,
    StepOpResult, TaskRuntime,
};

fn object_field<'a>(
    object: &'a Value,
    key: &str,
) -> Result<&'a Value, HarnessError> {
    object.get(key).ok_or_else(|| {
        HarnessError::corpus(format!("task input missing field {key}"))
    })
}

fn string_value(value: &Value, label: &str) -> Result<String, HarnessError> {
    value.as_str().map(str::to_owned).ok_or_else(|| {
        HarnessError::corpus(format!("task input {label} must be a string"))
    })
}

fn string_field(object: &Value, key: &str) -> Result<String, HarnessError> {
    string_value(object_field(object, key)?, key)
}

fn optional_string_field(object: &Value, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn verification_kind(value: &str) -> Result<VerificationKind, HarnessError> {
    match value {
        "deterministic" => Ok(VerificationKind::Deterministic),
        "review" => Ok(VerificationKind::Review),
        "user" => Ok(VerificationKind::User),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported verification kind {value}"
        ))),
    }
}

fn constraint_kind(value: &str) -> Result<ConstraintKind, HarnessError> {
    match value {
        "scope" => Ok(ConstraintKind::Scope),
        "process" => Ok(ConstraintKind::Process),
        "security" => Ok(ConstraintKind::Security),
        "escalation" => Ok(ConstraintKind::Escalation),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported constraint kind {value}"
        ))),
    }
}

fn pause_policy(value: &str) -> Result<PausePolicy, HarnessError> {
    match value {
        "none" => Ok(PausePolicy::None),
        "on_approval" => Ok(PausePolicy::OnApproval),
        "on_escalation" => Ok(PausePolicy::OnEscalation),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported pause policy {value}"
        ))),
    }
}

fn evidence_kind(value: &str) -> Result<EvidenceKind, HarnessError> {
    match value {
        "workspace_read" => Ok(EvidenceKind::WorkspaceRead),
        "parser_result" => Ok(EvidenceKind::ParserResult),
        "validation_result" => Ok(EvidenceKind::ValidationResult),
        "review_result" => Ok(EvidenceKind::ReviewResult),
        "user_approval" => Ok(EvidenceKind::UserApproval),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported evidence kind {value}"
        ))),
    }
}

fn task_phase(value: &str) -> Result<TaskPhase, HarnessError> {
    match value {
        "prepared" => Ok(TaskPhase::Prepared),
        "working" => Ok(TaskPhase::Working),
        "validating" => Ok(TaskPhase::Validating),
        "reviewing" => Ok(TaskPhase::Reviewing),
        "blocked" => Ok(TaskPhase::Blocked),
        "completed" => Ok(TaskPhase::Completed),
        "cancelled" => Ok(TaskPhase::Cancelled),
        "failed" => Ok(TaskPhase::Failed),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported phase {value}"
        ))),
    }
}

fn step_kind(value: &str) -> Result<TaskStepKind, HarnessError> {
    match value {
        "research" => Ok(TaskStepKind::Research),
        "implementation" => Ok(TaskStepKind::Implementation),
        "review" => Ok(TaskStepKind::Review),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported step kind {value}"
        ))),
    }
}

fn validation_status(
    value: &str,
) -> Result<TaskValidationStatus, HarnessError> {
    match value {
        "not_run" => Ok(TaskValidationStatus::NotRun),
        "clean" => Ok(TaskValidationStatus::Clean),
        "warnings" => Ok(TaskValidationStatus::Warnings),
        "failed" => Ok(TaskValidationStatus::Failed),
        "incomplete" => Ok(TaskValidationStatus::Incomplete),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported validation status {value}"
        ))),
    }
}

fn review_status(value: &str) -> Result<TaskReviewStatus, HarnessError> {
    match value {
        "not_run" => Ok(TaskReviewStatus::NotRun),
        "clean" => Ok(TaskReviewStatus::Clean),
        "findings" => Ok(TaskReviewStatus::Findings),
        "incomplete" => Ok(TaskReviewStatus::Incomplete),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported review status {value}"
        ))),
    }
}

fn finding_severity(value: &str) -> Result<FindingSeverity, HarnessError> {
    match value {
        "critical" => Ok(FindingSeverity::Critical),
        "high" => Ok(FindingSeverity::High),
        "medium" => Ok(FindingSeverity::Medium),
        "low" => Ok(FindingSeverity::Low),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported finding severity {value}"
        ))),
    }
}

fn verification_outcome(
    value: &str,
) -> Result<VerificationOutcome, HarnessError> {
    match value {
        "passed" => Ok(VerificationOutcome::Passed),
        "failed" => Ok(VerificationOutcome::Failed),
        "incomplete" => Ok(VerificationOutcome::Incomplete),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported verification outcome {value}"
        ))),
    }
}

fn approval_decision(value: &str) -> Result<ApprovalDecision, HarnessError> {
    match value {
        "approved" => Ok(ApprovalDecision::Approved),
        "denied" => Ok(ApprovalDecision::Denied),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported approval decision {value}"
        ))),
    }
}

fn parse_acceptance_criterion(
    value: &Value,
) -> Result<AcceptanceCriterion, HarnessError> {
    Ok(AcceptanceCriterion::new(
        string_field(value, "id")?,
        string_field(value, "description")?,
        verification_kind(&string_field(value, "verificationKind")?)?,
    ))
}

fn parse_constraint(value: &Value) -> Result<TaskConstraint, HarnessError> {
    Ok(TaskConstraint::new(
        string_field(value, "id")?,
        string_field(value, "description")?,
        constraint_kind(&string_field(value, "kind")?)?,
    ))
}

/// Contract parsing outcome: a validated contract, a reference
/// contract-validation rejection (canonical code), or a structural
/// harness failure (malformed scenario input).
enum ContractParseOutcome {
    Valid(TaskContract),
    Rejected(String),
    Structure(HarnessError),
}

fn parse_contract(value: &Value) -> ContractParseOutcome {
    let parse = (|| -> Result<TaskContract, HarnessError> {
        let constraints = value
            .get("constraints")
            .map(|entries| {
                entries
                    .as_array()
                    .ok_or_else(|| {
                        HarnessError::corpus(
                            "task input constraints must be an array",
                        )
                    })?
                    .iter()
                    .map(parse_constraint)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let criteria = object_field(value, "acceptanceCriteria")?
            .as_array()
            .ok_or_else(|| {
                HarnessError::corpus(
                    "task input acceptanceCriteria must be an array",
                )
            })?
            .iter()
            .map(parse_acceptance_criterion)
            .collect::<Result<Vec<_>, _>>()?;
        TaskContract::create(CreateTaskContractInput {
            id: string_field(value, "id")?,
            request: string_field(value, "request")?,
            context: optional_string_field(value, "context"),
            constraints,
            acceptance_criteria: criteria,
            pause_policy: Some(pause_policy(
                value
                    .get("pausePolicy")
                    .map(|entry| entry.as_str().unwrap_or("none"))
                    .unwrap_or("none"),
            )?),
        })
        .map_err(|error| {
            HarnessError::corpus(format!(
                "task input contract is invalid: {}",
                error.code()
            ))
        })
    })();
    match parse {
        Ok(contract) => ContractParseOutcome::Valid(contract),
        Err(error) => {
            let text = error.to_string();
            if let Some(code) =
                text.strip_prefix("task input contract is invalid: ")
            {
                ContractParseOutcome::Rejected(code.to_owned())
            } else {
                ContractParseOutcome::Structure(error)
            }
        }
    }
}

fn parse_evidence_source(
    value: &Value,
) -> Result<EvidenceSource, HarnessError> {
    let source_type = string_field(value, "type")?;
    match source_type.as_str() {
        "workspace_read" => {
            let paths = object_field(value, "paths")?
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus("task input workspace_read paths must be an array")
                })?
                .iter()
                .map(|entry| string_value(entry, "path"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(EvidenceSource::WorkspaceRead {
                paths,
                revision: optional_string_field(value, "revision"),
            })
        }
        "parser" => Ok(EvidenceSource::Parser {
            checked_files: object_field(value, "checkedFiles")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus("task input parser checkedFiles must be an integer")
                })?,
            valid_files: object_field(value, "validFiles")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus("task input parser validFiles must be an integer")
                })?,
            errors: object_field(value, "errors")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus("task input parser errors must be an integer")
                })?,
        }),
        "validation" => Ok(EvidenceSource::Validation {
            outcome: string_field(value, "outcome")?,
            workspace_integrity_verified: object_field(value, "workspaceIntegrityVerified")?
                .as_bool()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input validation workspaceIntegrityVerified must be a boolean",
                    )
                })?,
            unexpected_changes: object_field(value, "unexpectedChanges")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input validation unexpectedChanges must be an integer",
                    )
                })?,
        }),
        "review" => Ok(EvidenceSource::Review {
            status: string_field(value, "status")?,
            blocking_findings: object_field(value, "blockingFindings")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input review blockingFindings must be an integer",
                    )
                })?,
        }),
        "user_approval" => Ok(EvidenceSource::UserApproval {
            approval_id: string_field(value, "approvalId")?,
            subject_id: string_field(value, "subjectId")?,
            decision: approval_decision(&string_field(value, "decision")?)?,
        }),
        _ => Err(HarnessError::corpus(format!(
            "task input has an unsupported evidence source type {source_type}"
        ))),
    }
}

fn parse_verification(
    value: &Value,
) -> Result<EvidenceVerification, HarnessError> {
    Ok(EvidenceVerification {
        check_id: string_field(value, "checkId")?,
        criterion_id: value
            .get("criterionId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        outcome: verification_outcome(&string_field(value, "outcome")?)?,
    })
}

fn parse_step(value: &Value) -> Result<TaskStepSpec, HarnessError> {
    let accepts = object_field(value, "accepts")?
        .as_array()
        .ok_or_else(|| {
            HarnessError::corpus("task input step accepts must be an array")
        })?
        .iter()
        .map(|entry| evidence_kind(&string_value(entry, "accepts entry")?))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TaskStepSpec {
        id: string_field(value, "id")?,
        description: string_field(value, "description")?,
        kind: step_kind(&string_field(value, "kind")?)?,
        accepts,
    })
}

fn parse_finding(value: &Value) -> Result<FindingInput, HarnessError> {
    Ok(FindingInput {
        finding_id: string_field(value, "findingId")?,
        severity: finding_severity(&string_field(value, "severity")?)?,
        source: string_field(value, "source")?,
    })
}

fn parse_disposition(
    value: &Value,
) -> Result<WorkflowDisposition, HarnessError> {
    match string_field(value, "type")?.as_str() {
        "continue" => Ok(WorkflowDisposition::Continue {
            next_action: optional_string_field(value, "nextAction"),
        }),
        "complete" => Ok(WorkflowDisposition::Complete),
        "blocked" => Ok(WorkflowDisposition::Blocked {
            reason: string_field(value, "reason")?,
        }),
        other => Err(HarnessError::corpus(format!(
            "task input has an unsupported disposition type {other}"
        ))),
    }
}

fn step_op_json(op: &str, result: &StepOpResult) -> Value {
    match result {
        StepOpResult::Ok => json!({ "op": op, "ok": true }),
        StepOpResult::Rejected(error) => {
            json!({ "op": op, "ok": false, "code": error.code() })
        }
    }
}

fn criterion_json(op: &str, result: &CriterionResult) -> Value {
    match result {
        CriterionResult::Verified => json!({ "op": op, "status": "verified" }),
        CriterionResult::Failed => json!({ "op": op, "status": "failed" }),
        CriterionResult::Rejected(error) => {
            json!({ "op": op, "status": "rejected", "code": error.code() })
        }
    }
}

fn disposition_json(disposition: &WorkflowDisposition) -> Value {
    match disposition {
        WorkflowDisposition::Continue { next_action } => match next_action {
            Some(next_action) => {
                json!({ "type": "continue", "nextAction": next_action })
            }
            None => json!({ "type": "continue" }),
        },
        WorkflowDisposition::Complete => json!({ "type": "complete" }),
        WorkflowDisposition::Blocked { reason } => {
            json!({ "type": "blocked", "reason": reason })
        }
    }
}

fn evidence_ref_json(
    reference: &siralos_core::task::model::EvidenceRef,
) -> Value {
    json!({
        "evidenceId": reference.evidence_id,
        "kind": reference.kind.as_str(),
    })
}

fn activity_json(event: &ActivityEvent) -> Value {
    let base =
        json!({ "type": event.type_str(), "sequence": event.sequence() });
    match event {
        ActivityEvent::TaskStarted { contract_revision, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "contractRevision": contract_revision })
        }
        ActivityEvent::TaskPhaseChanged { phase, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "phase": phase.as_str() })
        }
        ActivityEvent::StepStarted { step_id, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "stepId": step_id })
        }
        ActivityEvent::StepCompleted { step_id, evidence_refs, .. } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "stepId": step_id,
                "evidenceRefs": evidence_refs.iter().map(evidence_ref_json).collect::<Vec<_>>(),
            })
        }
        ActivityEvent::StepFailed { step_id, reason, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "stepId": step_id, "reason": reason })
        }
        ActivityEvent::EvidenceAttached { evidence_id, kind, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "evidenceId": evidence_id, "kind": kind.as_str() })
        }
        ActivityEvent::CriterionVerified {
            criterion_id, verified_by, ..
        } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "criterionId": criterion_id, "verifiedBy": verified_by })
        }
        ActivityEvent::TaskBlocked { reason, .. }
        | ActivityEvent::TaskCancelled { reason, .. }
        | ActivityEvent::TaskFailed { reason, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "reason": reason })
        }
        ActivityEvent::TaskCompleted { .. } => base,
        ActivityEvent::TaskContractRevised { revision, .. } => {
            json!({ "type": event.type_str(), "sequence": event.sequence(), "revision": revision })
        }
        ActivityEvent::DispositionSubmitted {
            disposition,
            source,
            accepted,
            note,
            ..
        } => {
            json!({
                "type": event.type_str(),
                "sequence": event.sequence(),
                "disposition": disposition_json(disposition),
                "source": source.as_str(),
                "accepted": accepted,
                "note": note,
            })
        }
    }
}

/// Deterministic per-scenario clock value (set before each scenario).
static SCENARIO_NOW: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

/// Zero-capture clock feeding the runtime the scenario's controlled now.
fn scenario_clock() -> i64 {
    SCENARIO_NOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// Execute one task-contract scenario against the real siralos-core task
/// runtime and build the canonical R3 observation object.
fn run_task_scenario(input: &Value) -> Result<Value, HarnessError> {
    let now_value =
        input.get("now").and_then(Value::as_i64).ok_or_else(|| {
            HarnessError::corpus("task input requires an integer now")
        })?;
    let contract_value = object_field(input, "contract")?;
    let contract = match parse_contract(contract_value) {
        ContractParseOutcome::Valid(contract) => contract,
        ContractParseOutcome::Rejected(code) => {
            // Contract validation failure: the canonical rejection shape.
            return Ok(json!({ "rejected": true, "code": code }));
        }
        ContractParseOutcome::Structure(error) => return Err(error),
    };
    let steps = input
        .get("steps")
        .map(|entries| {
            entries
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus("task input steps must be an array")
                })?
                .iter()
                .map(parse_step)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let iteration = input.get("iteration").and_then(Value::as_f64);

    // The core clock is a zero-capture fn pointer; the scenario now is
    // deterministic per run, so a module-level atomic seam carries it.
    SCENARIO_NOW.store(now_value, std::sync::atomic::Ordering::Relaxed);
    let mut runtime = TaskRuntime::with_clock(scenario_clock);
    let task_id = match runtime.create_task(CreateTaskInput {
        contract,
        steps,
        iteration,
    }) {
        Ok(task_id) => task_id,
        Err(error) => {
            return Ok(json!({ "rejected": true, "code": error.code() }));
        }
    };

    let mut ops: Vec<Value> = Vec::new();
    if let Some(entries) = input.get("ops") {
        let entries = entries.as_array().ok_or_else(|| {
            HarnessError::corpus("task input ops must be an array")
        })?;
        for entry in entries {
            let op = string_field(entry, "op")?;
            let observation = run_task_op(&mut runtime, &task_id, &op, entry)?;
            ops.push(observation);
        }
    }

    let handle = runtime.task(&task_id).expect("task handle exists");
    let state = handle.snapshot();
    let completion = handle.evaluate_completion();
    let progress = handle.progress();
    let activity =
        handle.activity_log().iter().map(activity_json).collect::<Vec<_>>();
    let steps_json = state
        .steps
        .iter()
        .map(|step| {
            json!({
                "id": step.id,
                "status": step.status.as_str(),
                "evidenceRefs": step
                    .evidence_refs
                    .iter()
                    .map(evidence_ref_json)
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    let acceptance_json = state
        .acceptance
        .iter()
        .map(|criterion| {
            json!({
                "criterionId": criterion.criterion_id,
                "status": criterion.status.as_str(),
                "verifiedBy": criterion.verified_by,
            })
        })
        .collect::<Vec<_>>();
    let findings_json = state
        .current_findings
        .iter()
        .map(|finding| {
            json!({
                "findingId": finding.finding_id,
                "severity": finding.severity.as_str(),
                "source": finding.source,
            })
        })
        .collect::<Vec<_>>();
    let evidence_ids = state
        .evidence
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();

    Ok(json!({
        "rejected": false,
        "finalPhase": state.phase.as_str(),
        "contractRevision": state.contract_revision,
        "contractDigest": state.contract_digest,
        "stepStates": steps_json,
        "acceptance": acceptance_json,
        "evidenceIds": evidence_ids,
        "validationStatus": state.validation_status.as_str(),
        "reviewStatus": state.review_status.as_str(),
        "iteration": state.iteration,
        "currentFindings": findings_json,
        "terminalReason": state.terminal_reason,
        "startedAtMs": state.started_at_ms,
        "completedAtMs": state.completed_at_ms,
        "ops": ops,
        "activity": activity,
        "completion": { "allowed": completion.allowed, "missing": completion.missing },
        "progress": {
            "state": progress.state.as_str(),
            "usefulObservations": progress.useful_observations,
            "repeatedActions": progress.repeated_actions,
        },
    }))
}

/// Execute one operation and return its canonical observation.
fn run_task_op(
    runtime: &mut TaskRuntime,
    task_id: &str,
    op: &str,
    entry: &Value,
) -> Result<Value, HarnessError> {
    match op {
        "transitionPhase" => {
            let phase = task_phase(&string_field(entry, "phase")?)?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.transition_phase(phase);
            Ok(step_op_json("transitionPhase", &result))
        }
        "beginStep" => {
            let step_id = string_field(entry, "stepId")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.begin_step(&step_id);
            Ok(step_op_json("beginStep", &result))
        }
        "completeStep" => {
            let step_id = string_field(entry, "stepId")?;
            let refs = object_field(entry, "refs")?
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input completeStep refs must be an array",
                    )
                })?
                .iter()
                .map(|reference| {
                    Ok(siralos_core::task::model::EvidenceRef {
                        evidence_id: string_field(reference, "evidenceId")?,
                        kind: evidence_kind(&string_field(
                            reference, "kind",
                        )?)?,
                    })
                })
                .collect::<Result<Vec<_>, HarnessError>>()?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.complete_step(&step_id, &refs);
            Ok(step_op_json("completeStep", &result))
        }
        "failStep" => {
            let step_id = string_field(entry, "stepId")?;
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.fail_step(&step_id, &reason);
            Ok(step_op_json("failStep", &result))
        }
        "attachEvidence" => {
            let id = string_field(entry, "id")?;
            let kind = evidence_kind(&string_field(entry, "kind")?)?;
            let source =
                parse_evidence_source(object_field(entry, "source")?)?;
            let verification = entry
                .get("verification")
                .filter(|value| !value.is_null())
                .map(parse_verification)
                .transpose()?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result =
                handle.attach_evidence(&id, kind, source, verification);
            match result {
                AttachResult::Attached => {
                    Ok(json!({ "op": "attachEvidence", "ok": true }))
                }
                AttachResult::Rejected(rejection) => Ok(json!({
                    "op": "attachEvidence",
                    "ok": false,
                    "code": rejection.code(),
                })),
            }
        }
        "verifyCriterion" => {
            let criterion_id = string_field(entry, "criterionId")?;
            let verified_by = entry
                .get("verifiedBy")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let note = optional_string_field(entry, "note");
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.verify_criterion(
                &criterion_id,
                verified_by.as_deref(),
                note.as_deref(),
            );
            Ok(criterion_json("verifyCriterion", &result))
        }
        "markCriterionFailed" => {
            let criterion_id = string_field(entry, "criterionId")?;
            let note = optional_string_field(entry, "note");
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result =
                handle.mark_criterion_failed(&criterion_id, note.as_deref());
            Ok(criterion_json("markCriterionFailed", &result))
        }
        "setFindings" => {
            let findings = object_field(entry, "findings")?
                .as_array()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input findings must be an array",
                    )
                })?
                .iter()
                .map(parse_finding)
                .collect::<Result<Vec<_>, _>>()?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            match handle.set_findings(findings) {
                Ok(()) => Ok(json!({ "op": "setFindings", "ok": true })),
                Err(error) => Ok(
                    json!({ "op": "setFindings", "ok": false, "code": error.code() }),
                ),
            }
        }
        "setValidationStatus" => {
            let status = validation_status(&string_field(entry, "status")?)?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.set_validation_status(status);
            Ok(json!({ "op": "setValidationStatus", "ok": true }))
        }
        "setReviewStatus" => {
            let status = review_status(&string_field(entry, "status")?)?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.set_review_status(status);
            Ok(json!({ "op": "setReviewStatus", "ok": true }))
        }
        "setIteration" => {
            let iteration = object_field(entry, "iteration")?
                .as_u64()
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "task input iteration must be an integer",
                    )
                })?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.set_iteration(iteration);
            Ok(json!({ "op": "setIteration", "ok": true }))
        }
        "reviseContract" => {
            let changes_value = object_field(entry, "changes")?;
            let changes = ReviseTaskContractInput {
                id: string_field(changes_value, "id")?,
                request: optional_string_field(changes_value, "request"),
                context: match changes_value.get("context") {
                    Some(context) => Some(ReviseContext::Set(
                        context
                            .as_str()
                            .ok_or_else(|| {
                                HarnessError::corpus(
                                    "task input revise context must be a string",
                                )
                            })?
                            .to_owned(),
                    )),
                    None => None,
                },
                constraints: changes_value
                    .get("constraints")
                    .map(|entries| {
                        entries
                            .as_array()
                            .ok_or_else(|| {
                                HarnessError::corpus(
                                    "task input revise constraints must be an array",
                                )
                            })?
                            .iter()
                            .map(parse_constraint)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?,
                acceptance_criteria: changes_value
                    .get("acceptanceCriteria")
                    .map(|entries| {
                        entries
                            .as_array()
                            .ok_or_else(|| {
                                HarnessError::corpus(
                                    "task input revise acceptanceCriteria must be an array",
                                )
                            })?
                            .iter()
                            .map(parse_acceptance_criterion)
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?,
                pause_policy: changes_value
                    .get("pausePolicy")
                    .map(|policy| {
                        pause_policy(
                            policy
                                .as_str()
                                .ok_or_else(|| {
                                    HarnessError::corpus(
                                        "task input revise pausePolicy must be a string",
                                    )
                                })?,
                        )
                    })
                    .transpose()?,
            };
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            match handle.revise_contract(changes) {
                Ok(revision) => Ok(json!({
                    "op": "reviseContract",
                    "ok": true,
                    "revision": revision.revision(),
                })),
                Err(error) => Ok(json!({
                    "op": "reviseContract",
                    "ok": false,
                    "code": error.code(),
                })),
            }
        }
        "submitDisposition" => {
            let disposition =
                parse_disposition(object_field(entry, "disposition")?)?;
            let source = match string_field(entry, "source")?.as_str() {
                "host" => DispositionSource::Host,
                "model" => DispositionSource::Model,
                other => {
                    return Err(HarnessError::corpus(format!(
                        "task input has an unsupported disposition source {other}"
                    )));
                }
            };
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            let result = handle.submit_disposition(disposition, source);
            if result.accepted {
                Ok(json!({ "op": "submitDisposition", "accepted": true }))
            } else {
                Ok(json!({
                    "op": "submitDisposition",
                    "accepted": false,
                    "code": result.code.map(|code| code.code()).unwrap_or("rejected"),
                }))
            }
        }
        "completeTask" => {
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            match handle.complete_task() {
                CompletionResult::Completed => {
                    Ok(json!({ "op": "completeTask", "status": "completed" }))
                }
                CompletionResult::Rejected { reasons } => Ok(json!({
                    "op": "completeTask",
                    "status": "rejected",
                    "missing": reasons,
                })),
            }
        }
        "cancel" => {
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.cancel(&reason);
            Ok(json!({ "op": "cancel", "ok": true }))
        }
        "fail" => {
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.fail(&reason);
            Ok(json!({ "op": "fail", "ok": true }))
        }
        "markBlocked" => {
            let reason = string_field(entry, "reason")?;
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.mark_blocked(&reason);
            Ok(json!({ "op": "markBlocked", "ok": true }))
        }
        "observe" => {
            let action = string_field(entry, "action")?;
            let fingerprint = string_field(entry, "fingerprint")?;
            let progress = entry
                .get("progress")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut handle =
                runtime.task(task_id).expect("task handle exists");
            handle.observe(HostObservation { action, fingerprint, progress });
            Ok(json!({ "op": "observe", "ok": true }))
        }
        other => Err(HarnessError::corpus(format!(
            "task input has an unsupported op {other}"
        ))),
    }
}

fn task_contract_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let result = run_task_scenario(input)?;
    Ok(json!({
        "scenarioId": scenario_id,
        "subject": SUBJECT_TASK_CONTRACT,
        "outcome": "COMPLETED",
        "result": result,
    }))
}

// ---------------------------------------------------------------------------
// Stage 3R R4 subjects: generic workspace / project foundation.
// ---------------------------------------------------------------------------
//
// Executes each R4 scenario against the REAL Rust candidate workspace
// implementation (siralos-core workspace contracts and siralos-adapters
// workspace adapters) and builds the canonical R4 observation object
// that the TypeScript oracle probe emits for the same inputs. Fixtures
// are created in the host temp directory from declared inputs; records
// contain workspace-relative paths and content identities only.

use siralos_adapters::workspace::checkpoint::{
    open_checkpoint_store, reconcile_checkpoints,
};
use siralos_adapters::workspace::effects::{
    MutationTool, PreparationOutcome, prepare_mutation,
};
use siralos_adapters::workspace::git::git_inspection_disposition;
use siralos_adapters::workspace::list::{ListOutcome, list_directory};
use siralos_adapters::workspace::read::{
    ReadInput, ReadMode, ReadOutcome, parse_read_input, read_file,
};
use siralos_adapters::workspace::search::{
    SearchOutcome, parse_search_input, search,
};

use siralos_core::workspace::bounds::WORKSPACE_LIMITS;
use siralos_core::workspace::checkpoint::{
    CheckpointState, FileCheckpoint, WorkspaceFileState, plan_undo,
};

use siralos_core::workspace::revision::{
    ObservedReadMode, WorkspaceRevisionRegistry,
    WorkspaceRevisionRegistryOptions, compute_workspace_revision_handle,
};

use std::path::PathBuf;

fn scenario_string(input: &Value, key: &str) -> Result<String, HarnessError> {
    input.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(
        || {
            HarnessError::corpus(format!(
                "scenario input missing string field {key}"
            ))
        },
    )
}

fn scenario_u64(input: &Value, key: &str) -> Option<u64> {
    input.get(key).and_then(Value::as_u64)
}

fn scenario_array<'a>(
    input: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, HarnessError> {
    input.get(key).and_then(Value::as_array).ok_or_else(|| {
        HarnessError::corpus(format!(
            "scenario input missing array field {key}"
        ))
    })
}

fn validate_user_config_input(input: &Value) -> Result<(), HarnessError> {
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
/// Scenario string array field (typed corpus-integrity failure).
fn scenario_string_array(
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

fn scenario_object<'a>(
    input: &'a Value,
    key: &str,
) -> Result<&'a serde_json::Map<String, Value>, HarnessError> {
    input.get(key).and_then(Value::as_object).ok_or_else(|| {
        HarnessError::corpus(format!(
            "scenario input missing object field {key}"
        ))
    })
}

/// Deterministic fixture content generation mirroring the oracle probe.
fn fixture_bytes(spec: &Value) -> Result<Vec<u8>, HarnessError> {
    if let Some(content) = spec.get("content").and_then(Value::as_str) {
        return Ok(content.as_bytes().to_vec());
    }
    if let Some(bytes) = spec.get("bytes").and_then(Value::as_array) {
        let mut out = Vec::with_capacity(bytes.len());
        for byte in bytes {
            let value = byte.as_u64().ok_or_else(|| {
                HarnessError::corpus(
                    "fixture bytes must be non-negative integers",
                )
            })?;
            out.push(value.min(255) as u8);
        }
        return Ok(out);
    }
    if let Some(kind) = spec.get("kind").and_then(Value::as_str) {
        match kind {
            "nul-after-probe" => {
                let mut out = vec![0x61; 9000];
                out.push(0);
                out.extend_from_slice(b"b");
                return Ok(out);
            }
            "crlf" => return Ok(b"a\r\nb\r\n".to_vec()),
            "unicode" => {
                return Ok("héllo wörld\nsnowman ☃\nemoji 😀\n"
                    .as_bytes()
                    .to_vec());
            }
            "many-lines" => {
                let lines: Vec<String> =
                    (1..=300).map(|index| format!("line {index}")).collect();
                return Ok(lines.join("\n").into_bytes());
            }
            "empty" => return Ok(Vec::new()),
            "no-trailing-newline" => return Ok(b"hello".to_vec()),
            _ => {}
        }
    }
    if let Some(size) = spec.get("size").and_then(Value::as_u64) {
        let fill =
            spec.get("fill").and_then(Value::as_str).ok_or_else(|| {
                HarnessError::corpus("size fixture requires a fill character")
            })?;
        let byte = fill.as_bytes().first().copied().unwrap_or(b'x');
        return Ok(vec![byte; size as usize]);
    }
    Err(HarnessError::corpus("unsupported fixture spec"))
}

/// Create the fixture workspace tree and return its canonical root.
fn create_fixture_workspace(
    input: &Value,
    label: &str,
) -> Result<PathBuf, HarnessError> {
    let root = std::env::temp_dir().join(format!("siralos-cand-ws-{label}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot create fixture workspace: {error}"),
        )
    })?;
    if let Ok(files) = scenario_array(input, "files") {
        for spec in files {
            if spec.get("kind").and_then(Value::as_str) == Some("bulk") {
                let directory = root.join(scenario_string(spec, "path")?);
                let count = scenario_u64(spec, "count").ok_or_else(|| {
                    HarnessError::corpus("bulk fixture requires a count")
                })?;
                let content = spec
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        HarnessError::corpus("bulk fixture requires content")
                    })?;
                std::fs::create_dir_all(&directory).map_err(|error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!(
                            "cannot create bulk fixture directory: {error}"
                        ),
                    )
                })?;
                for index in 0..count {
                    let name = format!("f{index:03}.txt");
                    std::fs::write(directory.join(name), content).map_err(
                        |error| {
                            HarnessError::new(
                                HarnessErrorKind::ProbeSpawn,
                                format!("cannot write bulk fixture: {error}"),
                            )
                        },
                    )?;
                }
                continue;
            }
            let path = scenario_string(spec, "path")?;
            let target = root.join(&path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!("cannot create fixture parent: {error}"),
                    )
                })?;
            }
            std::fs::write(&target, fixture_bytes(spec)?).map_err(
                |error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!("cannot write fixture: {error}"),
                    )
                },
            )?;
        }
    }
    if let Ok(symlinks) = scenario_array(input, "symlinks") {
        for spec in symlinks {
            let link = root.join(scenario_string(spec, "link")?);
            let target = scenario_string(spec, "target")?;
            let directory =
                spec.get("directory").and_then(Value::as_bool) == Some(true);
            let target_path = if let Some(relative) =
                target.strip_prefix("../")
            {
                let outside =
                    root.parent().map(|parent| parent.join(relative));
                if let Some(outside) = &outside {
                    if directory {
                        let _ = std::fs::create_dir_all(outside);
                        let _ = std::fs::write(
                            outside.join("secret.txt"),
                            b"outside secret\n",
                        );
                    } else {
                        let _ = std::fs::write(outside, b"outside secret\n");
                    }
                }
                outside.unwrap_or_else(|| root.join(&target))
            } else {
                root.join(&target)
            };
            if let Some(parent) = link.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&target_path, &link);
            }
            #[cfg(windows)]
            {
                let _ =
                    std::os::windows::fs::symlink_file(&target_path, &link);
            }
        }
    }
    std::fs::canonicalize(&root).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot canonicalize fixture workspace: {error}"),
        )
    })
}

/// Run one R4 record builder under a fixture workspace with cleanup.
fn with_fixture_workspace(
    scenario_id: &str,
    input: &Value,
    build: impl FnOnce(&Path) -> Result<Value, HarnessError>,
) -> Result<Value, HarnessError> {
    let root = create_fixture_workspace(input, scenario_id)?;
    let outcome = build(&root);
    let _ = std::fs::remove_dir_all(&root);
    outcome
}

/// Canonical code for a read denial/failure message (mirror of the
/// oracle probe message mapping).
fn read_code(message: &str) -> &'static str {
    if message.contains("Path is empty.") {
        return "empty";
    }
    if message.contains("Path contains a null byte.") {
        return "null_byte";
    }
    if message.contains("Path must be relative to the workspace.") {
        return "absolute";
    }
    if message.contains("Path is outside the Siralos workspace.") {
        return "outside_workspace";
    }
    if message.contains("Path is inside the excluded directory") {
        return "excluded";
    }
    if message.contains("Path cannot be resolved") {
        return "unresolvable";
    }
    if message.contains("Cannot inspect file") {
        return "inspect_failed";
    }
    if message.contains("Target is not a regular file.") {
        return "not_file";
    }
    if message.contains("File is too large") {
        return "too_large";
    }
    if message.contains("Cannot read file") {
        return "unreadable";
    }
    if message.contains("File appears to be binary.") {
        return "binary";
    }
    if message.contains("File is not valid UTF-8 text.") {
        return "not_utf8";
    }
    if message.contains("beyond the end of the file") {
        return "start_beyond";
    }
    "inspect_failed"
}

fn list_code(message: &str) -> &'static str {
    if message.contains("Path is empty.") {
        return "empty";
    }
    if message.contains("Path contains a null byte.") {
        return "null_byte";
    }
    if message.contains("Path must be relative to the workspace.") {
        return "absolute";
    }
    if message.contains("Path is outside the Siralos workspace.") {
        return "outside_workspace";
    }
    if message.contains("Path is inside the excluded directory") {
        return "excluded";
    }
    if message.contains("Path cannot be resolved") {
        return "unresolvable";
    }
    if message.contains("Target is not a directory.") {
        return "not_directory";
    }
    if message.contains("Cannot inspect directory") {
        return "inspect_failed";
    }
    if message.contains("Cannot list directory") {
        return "list_failed";
    }
    if message.contains("Cannot inspect entry") {
        return "entry_inspect_failed";
    }
    "list_failed"
}

fn search_code(message: &str) -> &'static str {
    if message.contains("Tool input must be a JSON object.") {
        return "not_an_object";
    }
    if message.contains("\"query\" is required.") {
        return "query_required";
    }
    if message.contains("\"query\" must be a string.") {
        return "query_not_string";
    }
    if message.contains("\"path\" must be a string.") {
        return "path_not_string";
    }
    if message.contains("\"maxResults\" must be a positive integer.") {
        return "max_results_invalid";
    }
    if message.contains("Path is empty.") {
        return "empty";
    }
    if message.contains("Path contains a null byte.") {
        return "null_byte";
    }
    if message.contains("Path must be relative to the workspace.") {
        return "absolute";
    }
    if message.contains("Path is outside the Siralos workspace.") {
        return "outside_workspace";
    }
    if message.contains("Path is inside the excluded directory") {
        return "excluded";
    }
    if message.contains("Path cannot be resolved") {
        return "unresolvable";
    }
    "query_required"
}

fn read_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let fingerprint = scenario_string(input, "fingerprint")?;
    let mut registry =
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: fingerprint,
            max_entries: None,
        })
        .map_err(HarnessError::corpus)?;
    let mut reads: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "reads")? {
        let parsed = parse_read_input(entry);
        let request_path =
            entry.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
        let parsed = match parsed {
            Ok(parsed) => parsed,
            Err(_) => {
                reads.push(json!({
                    "path": request_path,
                    "status": "invalid_input",
                    "code": "invalid_input",
                }));
                continue;
            }
        };
        let outcome = read_file(
            root,
            &parsed,
            &WORKSPACE_LIMITS,
            Some(&mut registry),
            false,
        );
        match outcome {
            ReadOutcome::Success {
                path: _,
                sha256,
                revision,
                content,
                start_line,
                end_line,
                total_lines,
                truncated,
            } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "success",
                    "sha256": sha256,
                    "revision": revision,
                    "content": content,
                    "startLine": start_line,
                    "endLine": end_line,
                    "totalLines": total_lines,
                    "truncated": truncated,
                }));
            }
            ReadOutcome::Unsupported {
                path: _,
                mode,
                revision,
                supported,
                reason,
            } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "success",
                    "mode": mode.as_str(),
                    "revision": revision,
                    "supported": supported,
                    "reason": reason,
                }));
            }
            ReadOutcome::Denied { message } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "denied",
                    "code": read_code(&message),
                }));
            }
            ReadOutcome::Failed { message } => {
                reads.push(json!({
                    "path": request_path,
                    "status": "failed",
                    "code": read_code(&message),
                }));
            }
            ReadOutcome::InvalidInput { message } => {
                let _ = message;
                reads.push(json!({
                    "path": request_path,
                    "status": "invalid_input",
                    "code": "invalid_input",
                }));
            }
            ReadOutcome::Cancelled => {
                reads.push(
                    json!({ "path": request_path, "status": "cancelled" }),
                );
            }
        }
    }
    for entry in input
        .get("cancelledReads")
        .and_then(Value::as_array)
        .unwrap_or(&vec![])
    {
        let request_path =
            entry.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
        match parse_read_input(entry) {
            Ok(parsed) => {
                let outcome =
                    read_file(root, &parsed, &WORKSPACE_LIMITS, None, true);
                match outcome {
                    ReadOutcome::Cancelled => {
                        reads.push(json!({ "path": request_path, "status": "cancelled" }));
                    }
                    _ => {
                        reads.push(json!({ "path": request_path, "status": "failed" }));
                    }
                }
            }
            Err(_) => {
                reads.push(json!({ "path": request_path, "status": "invalid_input", "code": "invalid_input" }));
            }
        }
    }
    Ok(json!({ "reads": reads }))
}

fn list_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut lists: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "lists")? {
        let requested = match entry.get("path") {
            None => ".".to_owned(),
            Some(Value::String(value)) => value.clone(),
            Some(_) => {
                lists.push(json!({ "path": ".", "status": "invalid_input", "code": "invalid_input" }));
                continue;
            }
        };
        let outcome = list_directory(root, &requested, &WORKSPACE_LIMITS);
        match outcome {
            ListOutcome::Success { path, entries, truncated } => {
                let entries: Vec<Value> = entries
                    .into_iter()
                    .map(|entry| {
                        let (kind, size) = match entry.kind {
                            siralos_adapters::workspace::list::EntryKind::File { size } => {
                                ("file", Some(size))
                            }
                            siralos_adapters::workspace::list::EntryKind::Directory => ("directory", None),
                            siralos_adapters::workspace::list::EntryKind::Symlink => ("symlink", None),
                            siralos_adapters::workspace::list::EntryKind::Other => ("other", None),
                        };
                        match size {
                            Some(size) => json!({
                                "name": entry.name,
                                "path": entry.path,
                                "type": kind,
                                "size": size,
                            }),
                            None => json!({
                                "name": entry.name,
                                "path": entry.path,
                                "type": kind,
                            }),
                        }
                    })
                    .collect();
                lists.push(json!({
                    "path": requested,
                    "status": "success",
                    "resolvedPath": path,
                    "entries": entries,
                    "truncated": truncated,
                }));
            }
            ListOutcome::Denied { message } => {
                lists.push(json!({
                    "path": requested,
                    "status": "denied",
                    "code": list_code(&message),
                }));
            }
            ListOutcome::Failed { message } => {
                lists.push(json!({
                    "path": requested,
                    "status": "failed",
                    "code": list_code(&message),
                }));
            }
        }
    }
    Ok(json!({ "lists": lists }))
}

fn search_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut searches: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "searches")? {
        let parsed = match parse_search_input(entry, &WORKSPACE_LIMITS) {
            Ok(parsed) => parsed,
            Err(message) => {
                let query = entry
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                searches.push(json!({
                    "query": query,
                    "status": "invalid_input",
                    "code": search_code(&message),
                }));
                continue;
            }
        };
        let outcome = search(root, &parsed, &WORKSPACE_LIMITS, false);
        match outcome {
            SearchOutcome::Success {
                query,
                path,
                matches,
                scanned_files,
                skipped_files,
                truncated,
                truncation_reason,
            } => {
                let matches: Vec<Value> = matches
                    .into_iter()
                    .map(|match_entry| {
                        json!({
                            "path": match_entry.path,
                            "line": match_entry.line,
                            "column": match_entry.column,
                            "text": match_entry.text,
                        })
                    })
                    .collect();
                searches.push(json!({
                    "query": query,
                    "status": "success",
                    "path": path,
                    "matches": matches,
                    "scannedFiles": scanned_files,
                    "skippedFiles": skipped_files,
                    "truncated": truncated,
                    "truncationReason": truncation_reason.map(|reason| reason.as_str()),
                }));
            }
            SearchOutcome::Denied { message } => {
                let query = parsed.query;
                searches.push(json!({
                    "query": query,
                    "status": "denied",
                    "code": search_code(&message),
                }));
            }
            SearchOutcome::Cancelled => {
                searches.push(
                    json!({ "query": parsed.query, "status": "cancelled" }),
                );
            }
            SearchOutcome::InvalidInput { message } => {
                searches.push(json!({
                    "query": parsed.query,
                    "status": "invalid_input",
                    "code": search_code(&message),
                }));
            }
        }
    }
    Ok(json!({ "searches": searches }))
}

fn prepare_record(root: &Path, input: &Value) -> Result<Value, HarnessError> {
    let mut prepares: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "prepares")? {
        let tool_name = scenario_string(entry, "tool")?;
        let tool = match tool_name.as_str() {
            "workspace.create_file" => MutationTool::CreateFile,
            "workspace.edit_file" => MutationTool::EditFile,
            "workspace.delete_file" => MutationTool::DeleteFile,
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown mutation tool {other}"
                )));
            }
        };
        let cancelled =
            entry.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
        match prepare_mutation(tool, cancelled) {
            PreparationOutcome::Unavailable { .. } => {
                prepares.push(json!({
                    "tool": tool_name,
                    "status": "unavailable",
                    "code": "mutation_unavailable",
                }));
            }
            PreparationOutcome::Cancelled { .. } => {
                prepares
                    .push(json!({ "tool": tool_name, "status": "cancelled" }));
            }
        }
    }
    let verify_path = scenario_string(input, "verifyPath")?;
    let verify = read_file(
        root,
        &ReadInput {
            path: verify_path,
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        },
        &WORKSPACE_LIMITS,
        None,
        false,
    );
    let workspace_sha256 = match verify {
        ReadOutcome::Success { sha256, .. } => sha256,
        _ => "missing".to_owned(),
    };
    let checkpoint_root = std::env::temp_dir()
        .join(format!("siralos-cand-cp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&checkpoint_root);
    let store =
        open_checkpoint_store(root, &checkpoint_root).map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot open fixture checkpoint store: {error}"),
            )
        })?;
    let checkpoint_count = store.list(None, None).len();
    let _ = std::fs::remove_dir_all(&checkpoint_root);
    Ok(json!({
        "prepares": prepares,
        "workspaceSha256": workspace_sha256,
        "checkpointCount": checkpoint_count,
    }))
}

fn git_record() -> Value {
    let disposition = git_inspection_disposition();
    match disposition {
        siralos_core::workspace::git::GitInspectionDisposition::Unavailable { code, .. } => json!({
            "disposition": "unavailable",
            "code": code.as_str(),
        }),
    }
}

fn workspace_record(
    scenario_id: &str,
    subject: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    match subject {
        SUBJECT_WORKSPACE_READ => {
            with_fixture_workspace(scenario_id, input, |root| {
                read_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_LIST => {
            with_fixture_workspace(scenario_id, input, |root| {
                list_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_SEARCH => {
            with_fixture_workspace(scenario_id, input, |root| {
                search_record(root, input)
            })
        }
        SUBJECT_WORKSPACE_PREPARE => {
            with_fixture_workspace(scenario_id, input, |root| {
                prepare_record(root, input)
            })
        }
        SUBJECT_GIT_INSPECTION => Ok(git_record()),
        _ => Err(HarnessError::corpus(format!(
            "unsupported workspace subject {subject}"
        ))),
    }
}

fn revision_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let _ = scenario_id;
    let fingerprint = scenario_string(input, "fingerprint")?;
    let limit = scenario_u64(input, "limit").map(|value| value as usize);
    let mut registry =
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: fingerprint,
            max_entries: limit,
        })
        .map_err(HarnessError::corpus)?;
    let mut other =
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: "fixture-other-workspace".to_owned(),
            max_entries: None,
        })
        .map_err(HarnessError::corpus)?;
    let mut ops: Vec<Value> = Vec::new();
    for op in scenario_array(input, "ops")? {
        let op_name = scenario_string(op, "op")?;
        let result: Value = match op_name.as_str() {
            "issue" => {
                json!(registry.issue(
                    &scenario_string(op, "path")?,
                    &scenario_string(op, "sha256")?,
                ))
            }
            "resolve" => {
                match registry.resolve(&scenario_string(op, "handle")?) {
                    Some(identity) => json!({
                        "workspaceFingerprint": identity.workspace_fingerprint,
                        "path": identity.path,
                        "sha256": identity.sha256,
                    }),
                    None => Value::Null,
                }
            }
            "current" => registry
                .current_revision(&scenario_string(op, "path")?)
                .map(|handle| json!(handle))
                .unwrap_or(Value::Null),
            "state" => registry
                .revision_for_state(
                    &scenario_string(op, "path")?,
                    &scenario_string(op, "sha256")?,
                )
                .map(|handle| json!(handle))
                .unwrap_or(Value::Null),
            "invalidate" => {
                registry.invalidate_path(&scenario_string(op, "path")?);
                Value::Null
            }
            "observe" => {
                let path = scenario_string(op, "path")?;
                if let Some(handle) =
                    registry.current_revision(&path).map(str::to_owned)
                {
                    let mode = match scenario_string(op, "mode")?.as_str() {
                        "exact" => ObservedReadMode::Exact,
                        "structural" => ObservedReadMode::Structural,
                        "summary" => ObservedReadMode::Summary,
                        other => {
                            return Err(HarnessError::corpus(format!(
                                "unsupported read mode {other}"
                            )));
                        }
                    };
                    registry.observe_read(&path, &handle, mode);
                }
                Value::Null
            }
            "observed" => {
                let reads: Vec<Value> = registry
                    .observed_reads()
                    .iter()
                    .map(|read| {
                        json!({
                            "path": read.path,
                            "revision": read.revision,
                            "mode": read.mode.as_str(),
                            "atMs": read.at_ms,
                        })
                    })
                    .collect();
                json!(reads)
            }
            "size" => json!(registry.size()),
            "clear" => {
                registry.clear();
                Value::Null
            }
            "foreign-resolve" => {
                match other.resolve(&scenario_string(op, "handle")?) {
                    Some(identity) => json!({
                        "workspaceFingerprint": identity.workspace_fingerprint,
                        "path": identity.path,
                        "sha256": identity.sha256,
                    }),
                    None => Value::Null,
                }
            }
            "foreign-issue" => json!(other.issue(
                &scenario_string(op, "path")?,
                &scenario_string(op, "sha256")?,
            )),
            "compute" => json!(compute_workspace_revision_handle(
                &scenario_string(op, "workspace")?,
                &scenario_string(op, "path")?,
                &scenario_string(op, "sha256")?,
            )),
            other => {
                return Err(HarnessError::corpus(format!(
                    "unsupported revision op {other}"
                )));
            }
        };
        ops.push(json!({ "op": op_name, "result": result }));
    }
    Ok(json!({ "ops": ops }))
}

fn checkpoint_json(checkpoint: &FileCheckpoint, fingerprint: &str) -> Value {
    let before = &checkpoint.before;
    let after = &checkpoint.after;
    json!({
        "id": checkpoint.id,
        "operation": checkpoint.operation.as_str(),
        "state": checkpoint.state.as_str(),
        "relativePath": checkpoint.relative_path,
        "before": {
            "exists": before.exists,
            "sha256": before.sha256,
            "byteLength": before.byte_length,
        },
        "after": {
            "exists": after.exists,
            "sha256": after.sha256,
            "byteLength": after.byte_length,
        },
        "preview": {
            "addedLines": checkpoint.preview.added_lines,
            "removedLines": checkpoint.preview.removed_lines,
        },
        "fingerprintValid": checkpoint.workspace_fingerprint == fingerprint,
    })
}

fn write_fixture_checkpoint(
    store_root: &Path,
    fingerprint: &str,
    spec: &Value,
) -> Result<(), HarnessError> {
    let id = scenario_string(spec, "id")?;
    let directory = store_root.join(fingerprint).join(&id);
    std::fs::create_dir_all(&directory).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot create fixture checkpoint directory: {error}"),
        )
    })?;
    if let Some(raw) = spec.get("raw").and_then(Value::as_str) {
        std::fs::write(directory.join("metadata.json"), raw).map_err(
            |error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!(
                        "cannot write fixture checkpoint metadata: {error}"
                    ),
                )
            },
        )?;
        return Ok(());
    }
    if let Some(record_json) = spec.get("recordJson").and_then(Value::as_str) {
        let resolved = record_json.replace("__FINGERPRINT__", fingerprint);
        std::fs::write(directory.join("metadata.json"), resolved).map_err(
            |error| {
                HarnessError::new(
                    HarnessErrorKind::ProbeSpawn,
                    format!(
                        "cannot write fixture checkpoint metadata: {error}"
                    ),
                )
            },
        )?;
        return Ok(());
    }
    let record = scenario_object(spec, "record")?;
    let stored_fingerprint =
        if spec.get("foreignFingerprint").and_then(Value::as_bool)
            == Some(true)
        {
            "0".repeat(64)
        } else {
            fingerprint.to_owned()
        };
    let stored = json!({
        "version": 1,
        "id": id,
        "workspaceFingerprint": stored_fingerprint,
        "relativePath": record.get("relativePath").and_then(Value::as_str).unwrap_or(""),
        "operation": record.get("operation").and_then(Value::as_str).unwrap_or(""),
        "toolName": record.get("toolName").and_then(Value::as_str).unwrap_or(""),
        "createdAt": record.get("createdAt").and_then(Value::as_str).unwrap_or(""),
        "state": record.get("state").and_then(Value::as_str).unwrap_or(""),
        "before": record.get("before").cloned().unwrap_or(Value::Null),
        "after": record.get("after").cloned().unwrap_or(Value::Null),
        "preview": record.get("preview").cloned().unwrap_or(Value::Null),
    });
    let serialized =
        serde_json::to_string_pretty(&stored).map_err(|error| {
            HarnessError::corpus(format!(
                "cannot serialize fixture checkpoint: {error}"
            ))
        })?;
    std::fs::write(directory.join("metadata.json"), format!("{serialized}\n"))
        .map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot write fixture checkpoint metadata: {error}"),
            )
        })?;
    Ok(())
}

fn checkpoint_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let workspace =
        std::env::temp_dir().join(format!("siralos-cand-cpws-{scenario_id}"));
    let store_root = std::env::temp_dir()
        .join(format!("siralos-cand-cproot-{scenario_id}"));
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&store_root);
    std::fs::create_dir_all(&workspace).map_err(|error| {
        HarnessError::new(
            HarnessErrorKind::ProbeSpawn,
            format!("cannot create checkpoint fixture workspace: {error}"),
        )
    })?;
    if let Ok(files) = scenario_object(input, "workspaceFiles") {
        for (path, content) in files {
            let target = workspace.join(path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&target, content.as_str().unwrap_or("")).map_err(
                |error| {
                    HarnessError::new(
                        HarnessErrorKind::ProbeSpawn,
                        format!(
                            "cannot write checkpoint fixture file: {error}"
                        ),
                    )
                },
            )?;
        }
    }
    if let Ok(symlinks) = scenario_array(input, "workspaceSymlinks") {
        for spec in symlinks {
            let link = workspace.join(scenario_string(spec, "link")?);
            let target = scenario_string(spec, "target")?;
            let directory =
                spec.get("directory").and_then(Value::as_bool) == Some(true);
            let target_path = if let Some(relative) =
                target.strip_prefix("../")
            {
                let outside =
                    workspace.parent().map(|parent| parent.join(relative));
                if let Some(outside) = &outside {
                    if directory {
                        let _ = std::fs::create_dir_all(outside);
                        let _ = std::fs::write(
                            outside.join("secret.txt"),
                            b"outside secret\n",
                        );
                    } else {
                        let _ = std::fs::write(outside, b"outside secret\n");
                    }
                }
                outside.unwrap_or_else(|| workspace.join(&target))
            } else {
                workspace.join(&target)
            };
            if let Some(parent) = link.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&target_path, &link);
            }
            #[cfg(windows)]
            {
                let _ =
                    std::os::windows::fs::symlink_file(&target_path, &link);
            }
        }
    }
    let store =
        open_checkpoint_store(&workspace, &store_root).map_err(|error| {
            HarnessError::new(
                HarnessErrorKind::ProbeSpawn,
                format!("cannot open fixture checkpoint store: {error}"),
            )
        })?;
    let fingerprint = store.workspace_fingerprint().to_owned();
    if let Ok(checkpoints) = scenario_array(input, "checkpoints") {
        for spec in checkpoints {
            write_fixture_checkpoint(&store_root, &fingerprint, spec)?;
        }
    }
    let mut ops: Vec<Value> = Vec::new();
    for op in scenario_array(input, "ops")? {
        let op_name = scenario_string(op, "op")?;
        match op_name.as_str() {
            "list" | "list-after" => {
                let states =
                    op.get("states").and_then(Value::as_array).map(|states| {
                        states
                            .iter()
                            .filter_map(|state| {
                                CheckpointState::parse(state.as_str()?)
                            })
                            .collect::<Vec<CheckpointState>>()
                    });
                let checkpoints = store.list(states.as_deref(), None);
                let checkpoints: Vec<Value> = checkpoints
                    .iter()
                    .map(|checkpoint| {
                        checkpoint_json(checkpoint, &fingerprint)
                    })
                    .collect();
                ops.push(json!({ "op": op_name, "checkpoints": checkpoints }));
            }
            "get" => {
                let checkpoint = store.get(&scenario_string(op, "id")?);
                ops.push(json!({
                    "op": "get",
                    "checkpoint": checkpoint.as_ref().map(|c| checkpoint_json(c, &fingerprint)),
                }));
            }
            "reconcile" => {
                let report = reconcile_checkpoints(&store, 1024 * 1024);
                ops.push(json!({
                    "op": "reconcile",
                    "checked": report.checked,
                    "abandoned": report.abandoned,
                    "applied": report.applied,
                    "uncertain": report.uncertain,
                    "undoneAfterRestore": report.undone_after_restore,
                }));
            }
            "undo-plan" => {
                let id = scenario_string(op, "id")?;
                let checkpoint = store.get(&id).ok_or_else(|| {
                    HarnessError::corpus(format!(
                        "undo-plan requires a valid checkpoint {id}"
                    ))
                })?;
                let current = scenario_object(op, "current")?;
                let current_state = WorkspaceFileState {
                    exists: current
                        .get("exists")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    sha256: current
                        .get("sha256")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                };
                let decision = match plan_undo(&checkpoint, &current_state) {
                    siralos_core::workspace::checkpoint::UndoPlanDecision::ReadyCreate => "ready_create",
                    siralos_core::workspace::checkpoint::UndoPlanDecision::ReadyRestore => "ready_restore",
                    siralos_core::workspace::checkpoint::UndoPlanDecision::ReadyDelete => "ready_delete",
                    siralos_core::workspace::checkpoint::UndoPlanDecision::Conflict => "conflict",
                };
                ops.push(json!({ "op": "undo-plan", "decision": decision }));
            }
            other => {
                return Err(HarnessError::corpus(format!(
                    "unsupported checkpoint op {other}"
                )));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&workspace);
    let _ = std::fs::remove_dir_all(&store_root);
    Ok(json!({ "ops": ops }))
}
// Stage 3R R5 subjects: language-diagnostics / language-structure /
// language-definition (generic language intelligence parity).
// ---------------------------------------------------------------------------
//
// Executes each R5 scenario against the real siralos-core language
// modules (plus the generic URI mapping in siralos-adapters) and builds
// the canonical R5 observation object that the TypeScript oracle probe
// emits for the same inputs. The observation is semantic parity data,
// never a TypeScript object layout.

use siralos_adapters::language::uri::map_uri_to_workspace_relative;
use siralos_core::language::definition::{
    DefinitionLimits, RawDefinitionEntry, normalize_definition_locations,
};
use siralos_core::language::diagnostic::{
    Diagnostic, RawDiagnostic, RawDiagnosticCode,
    normalize_diagnostic_payload, normalize_diagnostic_set,
};
use siralos_core::language::limits::LANGUAGE_LIMITS;
use siralos_core::language::position::{RawPosition, RawRange};
use siralos_core::language::structure::{
    StructuralDeclaration, StructuralDocument, StructuralIssue,
    StructuralKind, StructureOptions, StructureStatus, SummaryOptions,
    build_structural_summary, normalize_structural_document,
};

/// One normalized diagnostic as a canonical record value.
fn diagnostic_value(diagnostic: &Diagnostic) -> Value {
    json!({
        "source": diagnostic.source,
        "severity": diagnostic.severity.as_str(),
        "path": diagnostic.path,
        "line": diagnostic.line,
        "column": diagnostic.column,
        "code": diagnostic.code,
        "message": diagnostic.message,
        "rawCategory": diagnostic.raw_category,
    })
}

/// Extract a raw 0-based position (None fields for malformed values,
/// mirroring the reference typeof/integer checks).
fn raw_position(value: &Value) -> RawPosition {
    let line = match value.get("line") {
        Some(Value::Number(number)) => {
            number.as_i64().filter(|line| *line >= 0)
        }
        _ => None,
    };
    let column = match value.get("character") {
        Some(Value::Number(number)) => {
            number.as_i64().filter(|column| *column >= 0)
        }
        _ => None,
    };
    RawPosition { line, column }
}

/// Extract a raw 0-based range; None when the value is not an object or
/// a position is absent.
fn raw_range(value: &Value) -> Option<RawRange> {
    let object = value.as_object()?;
    Some(RawRange {
        start: raw_position(object.get("start")?),
        end: raw_position(object.get("end")?),
    })
}

/// Parse one raw LSP-shaped diagnostic entry from the scenario input.
fn parse_raw_diagnostic(entry: &Value) -> Result<RawDiagnostic, HarnessError> {
    let severity = entry.get("severity").and_then(Value::as_i64);
    let code = match entry.get("code") {
        Some(Value::String(text)) => {
            Some(RawDiagnosticCode::Text(text.clone()))
        }
        Some(Value::Number(number)) => match number.as_i64() {
            Some(value) => Some(RawDiagnosticCode::Number(value)),
            None => Some(RawDiagnosticCode::Text(number.to_string())),
        },
        _ => None,
    };
    let message =
        entry.get("message").and_then(Value::as_str).map(str::to_owned);
    let source =
        entry.get("source").and_then(Value::as_str).map(str::to_owned);
    let range = entry.get("range").and_then(raw_range);
    Ok(RawDiagnostic { range, severity, code, message, source })
}

/// Canonical record for one language-diagnostics scenario.
fn language_diagnostics_record(input: &Value) -> Result<Value, HarnessError> {
    let fingerprint = scenario_string(input, "fingerprint")?;
    let root = scenario_string(input, "root")?;
    let source = scenario_string(input, "source")?;
    let run_max = scenario_u64(input, "runMax").map(|value| value as usize);
    let mut documents: Vec<Value> = Vec::new();
    let mut all_diagnostics: Vec<Diagnostic> = Vec::new();
    for document in scenario_array(input, "documents")? {
        let uri = scenario_string(document, "uri")?;
        let raw_entries = match document.get("diagnostics") {
            Some(Value::Array(entries)) => entries,
            _ => {
                // A non-array payload is rejected exactly like the
                // reference publish-diagnostics normalization.
                documents.push(json!({ "uri": uri, "status": "rejected" }));
                continue;
            }
        };
        let Some(path) = map_uri_to_workspace_relative(&uri, &root) else {
            // Out-of-workspace URIs are rejected, never guessed.
            documents.push(json!({ "uri": uri, "status": "rejected" }));
            continue;
        };
        let raw = raw_entries
            .iter()
            .map(parse_raw_diagnostic)
            .collect::<Result<Vec<_>, _>>()?;
        let per_set = scenario_u64(document, "max")
            .map(|value| value as usize)
            .unwrap_or(LANGUAGE_LIMITS.max_diagnostics_per_set);
        let limits = siralos_core::language::limits::LanguageLimits {
            max_diagnostics_per_set: per_set,
            ..LANGUAGE_LIMITS
        };
        let payload = normalize_diagnostic_payload(
            &raw,
            &source,
            &path,
            Some(&root),
            &limits,
        );
        let revision = match document.get("sha256") {
            Some(Value::String(sha256)) => {
                Some(compute_workspace_revision_handle(
                    &fingerprint,
                    &payload.path,
                    sha256,
                ))
            }
            _ => None,
        };
        let diagnostics = payload
            .diagnostics
            .iter()
            .map(diagnostic_value)
            .collect::<Vec<_>>();
        documents.push(json!({
            "uri": uri,
            "status": "normalized",
            "path": payload.path,
            "revision": revision,
            "diagnostics": diagnostics,
            "truncated": payload.truncated,
        }));
        all_diagnostics.extend(payload.diagnostics);
    }
    let run_bound = run_max.unwrap_or(LANGUAGE_LIMITS.max_diagnostics_per_run);
    let (aggregated, aggregated_truncated) =
        normalize_diagnostic_set(all_diagnostics, run_bound);
    let aggregate = json!({
        "diagnostics": aggregated.iter().map(diagnostic_value).collect::<Vec<_>>(),
        "truncated": aggregated_truncated,
    });
    Ok(json!({ "documents": documents, "aggregate": aggregate }))
}

/// Parse one generic structural declaration (language-neutral).
fn parse_declaration(value: &Value) -> StructuralDeclaration {
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .and_then(StructuralKind::parse)
        .unwrap_or(StructuralKind::Other);
    let name = value.get("name").and_then(Value::as_str).map(str::to_owned);
    let detail =
        value.get("detail").and_then(Value::as_str).map(str::to_owned);
    let line =
        value.get("line").and_then(Value::as_u64).filter(|line| *line >= 1);
    let attributes = parse_string_array(value.get("attributes"));
    let children = value
        .get("children")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().map(parse_declaration).collect())
        .unwrap_or_default();
    StructuralDeclaration { kind, name, detail, line, attributes, children }
}

/// Parse an opaque bounded string array (missing or non-array values
/// are empty; non-string entries are skipped).
fn parse_string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the generic structural observation facts. Status and
/// truncation are derived by normalization, never read from the
/// input (the input carries parser facts only).
fn parse_structure(value: &Value) -> StructuralDocument {
    let path =
        value.get("path").and_then(Value::as_str).unwrap_or("").to_owned();
    let declarations = value
        .get("declarations")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().map(parse_declaration).collect())
        .unwrap_or_default();
    let dependencies = parse_string_array(value.get("dependencies"));
    let issues = value
        .get("issues")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| StructuralIssue {
                    line: entry
                        .get("line")
                        .and_then(Value::as_u64)
                        .filter(|line| *line >= 1),
                    message: entry
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    StructuralDocument {
        path,
        revision: None,
        declarations,
        dependencies,
        status: StructureStatus::Complete,
        issues,
        truncated: false,
    }
}

/// Canonical record for one language-structure scenario.
fn language_structure_record(input: &Value) -> Result<Value, HarnessError> {
    let fingerprint = scenario_string(input, "fingerprint")?;
    let mut summaries: Vec<Value> = Vec::new();
    for document in scenario_array(input, "documents")? {
        let structure_value = document.get("structure").ok_or_else(|| {
            HarnessError::corpus(
                "language-structure document requires a structure object",
            )
        })?;
        let parsed = parse_structure(structure_value);
        let path = parsed.path.clone();
        let revision = match document.get("sha256") {
            Some(Value::String(sha256)) if !path.is_empty() => Some(
                compute_workspace_revision_handle(&fingerprint, &path, sha256),
            ),
            _ => None,
        };
        let mut normalized = normalize_structural_document(
            &path,
            parsed.declarations,
            parsed.dependencies,
            parsed.issues,
            &StructureOptions::default(),
        );
        normalized.revision = revision.clone();
        let options = SummaryOptions {
            max_bytes: scenario_u64(document, "maxBytes")
                .map(|value| value as usize),
            notable_declarations: scenario_u64(
                document,
                "notableDeclarations",
            )
            .map(|value| value as usize),
        };
        let summary = build_structural_summary(&normalized, &options);
        summaries.push(json!({
            "path": summary.path,
            "revision": summary.revision,
            "mode": "summary",
            "advisory": true,
            "truncated": summary.truncated,
            "bytes": summary.bytes,
            "text": summary.text,
        }));
    }
    Ok(json!({ "summaries": summaries }))
}

fn parse_raw_definition_entry(entry: &Value) -> RawDefinitionEntry {
    let uri = match entry.get("targetUri") {
        Some(Value::String(value)) => Some(value.clone()),
        _ => match entry.get("uri") {
            Some(Value::String(value)) => Some(value.clone()),
            _ => None,
        },
    };
    let range = match entry.get("targetRange") {
        Some(value) => raw_range(value),
        None => entry.get("range").and_then(raw_range),
    };
    RawDefinitionEntry { uri, range }
}

/// Canonical record for one language-definition scenario.
fn language_definition_record(input: &Value) -> Result<Value, HarnessError> {
    let root = scenario_string(input, "root")?;
    let mut queries: Vec<Value> = Vec::new();
    for query in scenario_array(input, "queries")? {
        let uri = scenario_string(query, "uri")?;
        let fallback = scenario_string(query, "path")?;
        let query_path =
            map_uri_to_workspace_relative(&uri, &root).unwrap_or(fallback);
        let entries = query
            .get("locations")
            .map(|locations| match locations {
                Value::Array(items) => items
                    .iter()
                    .map(parse_raw_definition_entry)
                    .collect::<Vec<_>>(),
                Value::Null => Vec::new(),
                _ => vec![parse_raw_definition_entry(locations)],
            })
            .unwrap_or_default();
        let result = normalize_definition_locations(
            &entries,
            &query_path,
            |target| map_uri_to_workspace_relative(target, &root),
            DefinitionLimits {
                max_locations: scenario_u64(query, "max")
                    .map(|value| value as usize)
                    .unwrap_or(LANGUAGE_LIMITS.max_definition_locations),
            },
        );
        let locations = result
            .locations
            .iter()
            .map(|location| {
                json!({
                    "path": location.path,
                    "range": {
                        "start": {
                            "line": location.range.start.line,
                            "column": location.range.start.column,
                        },
                        "end": {
                            "line": location.range.end.line,
                            "column": location.range.end.column,
                        },
                    },
                    "external": location.external,
                })
            })
            .collect::<Vec<_>>();
        queries.push(json!({
            "uri": uri,
            "path": result.path,
            "locations": locations,
            "truncated": result.truncated,
        }));
    }
    Ok(json!({ "queries": queries }))
}

// ---------------------------------------------------------------------------
// Stage 3R R6 subjects: domain-lifecycle and domain-capability.
// ---------------------------------------------------------------------------
//
// Executes each R6 scenario against the real siralos-core::domain
// lifecycle/capability semantics and builds the canonical R6
// observation object that the TypeScript oracle probe emits for the
// same inputs. The observations are semantic parity data, never a
// TypeScript object layout.

use siralos_core::domain::capability::{
    CapabilityRequest, GrantDecision, HostAuthority, decide_grant,
};
use siralos_core::domain::failure::DomainFailure;
use siralos_core::domain::lifecycle::{
    ActivationRequest, DomainLifecycle, RuntimeCheckResult,
    classify_workspace_file, workspace_domain_scan,
};
use siralos_core::domain::package::{
    DomainAbi, DomainPackage, PackageDigest, verify_package_digest,
};

/// Failure record: the stable code plus typed detail fields.
fn domain_failure_record(op: &str, failure: &DomainFailure) -> Value {
    let mut record = serde_json::Map::new();
    record.insert("op".to_owned(), json!(op));
    record.insert("ok".to_owned(), json!(false));
    record.insert("code".to_owned(), json!(failure.code()));
    if let DomainFailure::CapabilityDenied { missing }
    | DomainFailure::UndeclaredCapability { missing } = failure
    {
        record.insert(
            "missing".to_owned(),
            json!(missing.iter().map(|id| id.as_str()).collect::<Vec<_>>()),
        );
    }
    Value::Object(record)
}

/// Parse one package descriptor from a scenario value (typed failure,
/// matching the reference parse order: id, digest, abi, capabilities).
fn parse_scenario_package(
    value: &Value,
) -> Result<DomainPackage, DomainFailure> {
    let id = domain_string(value.get("id"))?;
    let digest = domain_string(value.get("digest"))?;
    let abi = domain_string(value.get("abi"))?;
    let capabilities =
        domain_string_array(value.get("requestedCapabilities"))?;
    DomainPackage::parse(&id, &digest, &abi, &capabilities)
}

/// Parse an activation request from a scenario value.
fn parse_scenario_request(
    value: &Value,
) -> Result<ActivationRequest, DomainFailure> {
    let package_id = domain_string(value.get("packageId"))?;
    let digest = domain_string(value.get("digest"))?;
    let abi = domain_string(value.get("abi"))?;
    let capabilities = domain_string_array(value.get("capabilities"))?;
    ActivationRequest::parse(&package_id, &digest, &abi, &capabilities)
}

/// One scenario string field (typed invalid-input failure).
fn domain_string(value: Option<&Value>) -> Result<String, DomainFailure> {
    value.and_then(Value::as_str).map(str::to_owned).ok_or_else(|| {
        DomainFailure::InvalidInput {
            reason: "missing string field".to_owned(),
        }
    })
}

/// One scenario string array field (typed invalid-input failure).
fn domain_string_array(
    value: Option<&Value>,
) -> Result<Vec<String>, DomainFailure> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry.as_str().map(str::to_owned).ok_or_else(|| {
                        DomainFailure::InvalidInput {
                            reason: "array entry is not a string".to_owned(),
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|| {
            Err(DomainFailure::InvalidInput {
                reason: "array field expected".to_owned(),
            })
        })
}

/// The canonical inspect observation for the current lifecycle state.
fn domain_inspect_value(lifecycle: &DomainLifecycle) -> Value {
    let package = lifecycle.installed_package();
    let active = lifecycle.active();
    json!({
        "op": "inspect",
        "state": lifecycle.state().as_str(),
        "available": lifecycle.available(),
        "enabled": lifecycle.enabled(),
        "active": active.is_some(),
        "package": package.map(|package| json!({
            "id": package.id().as_str(),
            "digest": package.digest().as_str(),
            "abi": package.abi().as_str(),
            "requestedCapabilities": package
                .requested_capabilities()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
        })),
        "activation": active.map(|active| json!({
            "sessionId": active.session_id(),
            "binding": {
                "packageId": active.binding().package_id().as_str(),
                "digest": active.binding().digest().as_str(),
                "abi": active.binding().abi().as_str(),
            },
            "grant": active
                .grant()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
        })),
    })
}

/// Execute one domain-lifecycle operation and return its canonical
/// observation.
fn run_domain_lifecycle_op(
    lifecycle: &mut DomainLifecycle,
    op: &str,
    entry: &Value,
    supported_abi: &DomainAbi,
    authority: &HostAuthority,
) -> Result<Value, HarnessError> {
    match op {
        "inspect" => Ok(domain_inspect_value(lifecycle)),
        "install" => {
            let package = match parse_scenario_package(object_field(
                entry, "package",
            )?) {
                Ok(package) => package,
                Err(failure) => {
                    return Ok(domain_failure_record("install", &failure));
                }
            };
            let computed = scenario_string(entry, "computedDigest")?;
            let computed_digest = match PackageDigest::parse(&computed) {
                Ok(digest) => digest,
                Err(failure) => {
                    return Ok(domain_failure_record("install", &failure));
                }
            };
            if let Err(failure) =
                verify_package_digest(package.digest(), &computed_digest)
            {
                return Ok(domain_failure_record("install", &failure));
            }
            match lifecycle.install(package) {
                Ok(()) => Ok(json!({
                    "op": "install",
                    "ok": true,
                    "state": lifecycle.state().as_str(),
                })),
                Err(failure) => Ok(domain_failure_record("install", &failure)),
            }
        }
        "uninstall" | "enable" | "disable" | "deactivate" => {
            let result = match op {
                "uninstall" => lifecycle.uninstall(),
                "enable" => lifecycle.enable(),
                "disable" => lifecycle.disable(),
                _ => lifecycle.deactivate(),
            };
            Ok(match result {
                Ok(()) => json!({ "op": op, "ok": true }),
                Err(failure) => domain_failure_record(op, &failure),
            })
        }
        "eligibility" => {
            let request = match parse_scenario_request(object_field(
                entry,
                "activation",
            )?) {
                Ok(request) => request,
                Err(failure) => {
                    return Ok(domain_failure_record("eligibility", &failure));
                }
            };
            let runtime = match RuntimeCheckResult::parse(&scenario_string(
                entry, "runtime",
            )?) {
                Ok(runtime) => runtime,
                Err(failure) => {
                    return Ok(domain_failure_record("eligibility", &failure));
                }
            };
            let eligibility = lifecycle.eligibility(
                &request,
                supported_abi,
                authority,
                &runtime,
            );
            Ok(json!({
                "op": "eligibility",
                "ready": eligibility.ready(),
                "reasons": eligibility
                    .reasons()
                    .iter()
                    .map(|reason| reason.code())
                    .collect::<Vec<_>>(),
            }))
        }
        "activate" => {
            let request = match parse_scenario_request(object_field(
                entry,
                "activation",
            )?) {
                Ok(request) => request,
                Err(failure) => {
                    return Ok(domain_failure_record("activate", &failure));
                }
            };
            let runtime = match RuntimeCheckResult::parse(&scenario_string(
                entry, "runtime",
            )?) {
                Ok(runtime) => runtime,
                Err(failure) => {
                    return Ok(domain_failure_record("activate", &failure));
                }
            };
            match lifecycle.activate(
                request,
                supported_abi,
                authority,
                runtime,
            ) {
                Ok(active) => Ok(json!({
                    "op": "activate",
                    "ok": true,
                    "sessionId": active.session_id(),
                    "binding": {
                        "packageId": active.binding().package_id().as_str(),
                        "digest": active.binding().digest().as_str(),
                        "abi": active.binding().abi().as_str(),
                    },
                    "grant": active
                        .grant()
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>(),
                })),
                Err(failure) => {
                    Ok(domain_failure_record("activate", &failure))
                }
            }
        }
        "workspaceScan" => {
            let files = scenario_string_array(entry, "files")?;
            let classified = files
                .iter()
                .map(|name| json!({ "name": name, "kind": classify_workspace_file(name) }))
                .collect::<Vec<_>>();
            let scan = workspace_domain_scan(&files);
            Ok(json!({
                "op": "workspaceScan",
                "files": classified,
                "candidates": scan.candidates,
                "installs": scan.installs,
                "enables": scan.enables,
                "activations": scan.activations,
                "downloads": scan.downloads,
                "recommendations": scan.recommendations,
            }))
        }
        _ => Ok(json!({ "op": op, "ok": false, "code": "INVALID_INPUT" })),
    }
}

/// Canonical record for one domain-lifecycle scenario.
fn domain_lifecycle_record(input: &Value) -> Result<Value, HarnessError> {
    let supported_abi =
        DomainAbi::parse(&scenario_string(input, "supportedAbi")?).map_err(
            |failure| {
                HarnessError::corpus(format!(
                    "domain input has an invalid supported ABI: {}",
                    failure.code()
                ))
            },
        )?;
    let authority =
        HostAuthority::parse(&scenario_string_array(input, "authority")?)
            .map_err(|failure| {
                HarnessError::corpus(format!(
                    "domain input has an invalid authority: {}",
                    failure.code()
                ))
            })?;
    let mut lifecycle = DomainLifecycle::new();
    let mut ops: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "ops")? {
        let op = scenario_string(entry, "op")?;
        let observation = run_domain_lifecycle_op(
            &mut lifecycle,
            &op,
            entry,
            &supported_abi,
            &authority,
        )?;
        ops.push(observation);
    }
    Ok(json!({ "ops": ops }))
}

/// Canonical record for one domain-capability scenario.
fn domain_capability_record(input: &Value) -> Result<Value, HarnessError> {
    let authority = match HostAuthority::parse(&scenario_string_array(
        input,
        "authority",
    )?) {
        Ok(authority) => authority,
        Err(failure) => {
            return Ok(json!({
                "authority": [],
                "ops": [{"op": "invalid", "ok": false, "code": failure.code()}],
            }));
        }
    };
    let mut ops: Vec<Value> = Vec::new();
    for entry in scenario_array(input, "ops")? {
        let op = scenario_string(entry, "op")?;
        match op.as_str() {
            "decide" => {
                let request = match CapabilityRequest::parse(
                    &scenario_string_array(entry, "request")?,
                ) {
                    Ok(request) => request,
                    Err(failure) => {
                        ops.push(json!({
                            "op": "decide",
                            "ok": false,
                            "code": failure.code(),
                        }));
                        continue;
                    }
                };
                match decide_grant(&request, &authority) {
                    GrantDecision::Granted(grant) => ops.push(json!({
                        "op": "decide",
                        "granted": true,
                        "grant": grant
                            .iter()
                            .map(|id| id.as_str())
                            .collect::<Vec<_>>(),
                    })),
                    GrantDecision::Denied { missing } => ops.push(json!({
                        "op": "decide",
                        "granted": false,
                        "missing": missing
                            .iter()
                            .map(|id| id.as_str())
                            .collect::<Vec<_>>(),
                    })),
                }
            }
            "inspectAuthority" => ops.push(json!({
                "op": "inspectAuthority",
                "authority": authority
                    .iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>(),
            })),
            _ => ops.push(json!({
                "op": op,
                "ok": false,
                "code": "INVALID_INPUT",
            })),
        }
    }
    Ok(json!({ "ops": ops }))
}

#[cfg(test)]
mod tests {
    use super::{
        HarnessErrorKind, PLATFORM_POSIX, PLATFORM_WINDOWS,
        canonical_records_text, canonical_scenario_digest,
        cargo_workspace_version, execute_bounded_child, load_corpus,
        platform_name, probe_state_dir_bytes, provider_turn_record,
        state_dir_record, validate_provider_turn_input, validate_scenario,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use super::Scenario;

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    struct TempCorpus(PathBuf);

    impl TempCorpus {
        fn copy() -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let source = root.join("tests/differential/corpus");
            let destination = std::env::temp_dir().join(format!(
                "siralos-rust-corpus-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&destination).expect("create temp corpus");
            for entry in std::fs::read_dir(source).expect("read corpus") {
                let entry = entry.expect("corpus entry");
                std::fs::copy(
                    entry.path(),
                    destination.join(entry.file_name()),
                )
                .expect("copy corpus entry");
            }
            Self(destination)
        }
    }

    impl Drop for TempCorpus {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove temp corpus");
        }
    }

    fn scenario() -> Scenario {
        Scenario {
            id: "state-dir.set.posix".to_owned(),
            subject: "state-dir".to_owned(),
            platforms: vec!["posix".to_owned()],
            parity: "required".to_owned(),
            env: BTreeMap::from([(
                "HOME".to_owned(),
                "/fixture/home".to_owned(),
            )]),
            input: None,
        }
    }

    #[test]
    fn platform_name_is_one_of_the_contract_values() {
        assert!(matches!(platform_name(), PLATFORM_WINDOWS | PLATFORM_POSIX));
    }

    #[test]
    fn state_dir_record_hashes_raw_ok_bytes() {
        let record = state_dir_record(
            "state-dir.set.windows",
            b"C:\\fixture\\home\\.siralos",
        );
        assert_eq!(record["outcome"], "COMPLETED");
        assert_eq!(
            record["result"]["stateDirSha256"].as_str().unwrap().len(),
            64
        );
    }

    #[test]
    fn state_dir_record_marks_only_the_exact_error_marker() {
        let record = state_dir_record("state-dir.unset.windows", b"ERR");
        assert_eq!(record["outcome"], "PRODUCT_FAILURE");
        assert_eq!(record["error"]["category"], "NO_HOME_DIRECTORY");

        let non_utf8 = state_dir_record("state-dir.set.posix", &[0xff]);
        assert_eq!(non_utf8["outcome"], "COMPLETED");
    }

    #[test]
    fn records_are_canonical_with_one_trailing_newline() {
        let records = vec![
            json!({"scenarioId": "b", "kind": "ok"}),
            json!({"scenarioId": "a", "kind": "ok"}),
        ];
        assert_eq!(
            canonical_records_text(records),
            "{\"records\":[{\"kind\":\"ok\",\"scenarioId\":\"b\"},{\"kind\":\"ok\",\"scenarioId\":\"a\"}],\"schemaVersion\":1}\n"
        );
    }

    #[test]
    fn scenario_validation_rejects_unknown_environment_authority() {
        let mut input = scenario();
        input.env.insert("NODE_OPTIONS".to_owned(), "--require=x".to_owned());
        assert!(
            validate_scenario(&input, "state-dir.set.posix.json").is_err()
        );
    }

    #[test]
    fn scenario_validation_rejects_id_file_mismatch() {
        assert!(validate_scenario(&scenario(), "other.json").is_err());
    }

    #[test]
    fn scenario_validation_rejects_required_os_home_fallback() {
        let mut input = scenario();
        input.env.insert("HOME".to_owned(), String::new());
        let error = validate_scenario(&input, "state-dir.set.posix.json")
            .expect_err("required fallback is uncontrolled");
        assert!(error.to_string().contains("does not fully declare"));
    }

    #[test]
    fn scenario_digest_matches_the_javascript_canonical_order() {
        assert_eq!(
            canonical_scenario_digest(&scenario()).expect("canonical"),
            "50508c3bb501926a100bbcbd9567bac8add9b125a4bd234da11cafbad84202ef"
        );
    }

    #[test]
    fn strict_loader_accepts_the_checked_in_digest_bound_corpus() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let loaded = load_corpus(
            &root.join("tests/differential/corpus"),
            platform_name(),
        )
        .expect("checked-in corpus");
        assert_eq!(loaded.len(), 133);
    }

    #[test]
    fn strict_loader_rejects_digest_tampering() {
        let corpus = TempCorpus::copy();
        let path = corpus.0.join("state-dir.set.posix.json");
        let text = std::fs::read_to_string(&path).expect("scenario");
        std::fs::write(&path, text.replace("/fixture/home", "/tampered"))
            .expect("tamper scenario");
        let error = load_corpus(&corpus.0, PLATFORM_POSIX)
            .err()
            .expect("tampering rejected");
        assert_eq!(error.kind(), HarnessErrorKind::Corpus);
        assert_eq!(error.code(), "CONTENT_MISMATCH");
        assert!(error.to_string().contains("manifest digest"));
    }

    #[test]
    fn strict_loader_distinguishes_corpus_digest_failures() {
        for (replacement, expected_code) in [
            (None, "MISSING_DIGEST"),
            (Some("invalid"), "MALFORMED_DIGEST"),
            (
                Some(
                    "0000000000000000000000000000000000000000000000000000000000000000",
                ),
                "CONTENT_MISMATCH",
            ),
        ] {
            let corpus = TempCorpus::copy();
            let path = corpus.0.join("manifest.json");
            let text = std::fs::read_to_string(&path).expect("manifest");
            let mut manifest: serde_json::Value =
                serde_json::from_str(&text).expect("valid manifest JSON");
            let object = manifest.as_object_mut().expect("manifest object");
            if let Some(replacement) = replacement {
                object.insert(
                    "corpusSha256".to_owned(),
                    serde_json::Value::String(replacement.to_owned()),
                );
            } else {
                object.remove("corpusSha256");
            }
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&manifest)
                    .expect("serialize manifest"),
            )
            .expect("alter manifest");
            let error = load_corpus(&corpus.0, PLATFORM_POSIX)
                .err()
                .expect("invalid corpus digest rejected");
            assert_eq!(error.kind(), HarnessErrorKind::Corpus);
            assert_eq!(error.code(), expected_code);
        }
    }

    #[test]
    fn strict_loader_rejects_unknown_fields() {
        let corpus = TempCorpus::copy();
        let path = corpus.0.join("state-dir.set.posix.json");
        let text = std::fs::read_to_string(&path).expect("scenario");
        std::fs::write(
            &path,
            text.replace("\n}", ",\n  \"unexpected\": true\n}"),
        )
        .expect("alter scenario");
        let error = load_corpus(&corpus.0, PLATFORM_POSIX)
            .err()
            .expect("unknown field rejected");
        assert_eq!(error.kind(), HarnessErrorKind::Corpus);
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn strict_loader_rejects_manifest_path_traversal() {
        let corpus = TempCorpus::copy();
        let path = corpus.0.join("manifest.json");
        let text = std::fs::read_to_string(&path).expect("manifest");
        std::fs::write(
            &path,
            text.replace(
                "state-dir.set.windows.json",
                "../state-dir.set.windows.json",
            ),
        )
        .expect("alter manifest");
        let error = load_corpus(&corpus.0, PLATFORM_POSIX)
            .err()
            .expect("traversal rejected");
        assert_eq!(error.kind(), HarnessErrorKind::Corpus);
        assert!(error.to_string().contains("invalid file name"));
    }

    #[cfg(unix)]
    #[test]
    fn strict_loader_rejects_a_symlinked_corpus_root() {
        use std::os::unix::fs::symlink;

        let corpus = TempCorpus::copy();
        let alias = corpus.0.with_extension("alias");
        symlink(&corpus.0, &alias).expect("create corpus alias");
        let error = load_corpus(&alias, PLATFORM_POSIX)
            .err()
            .expect("symlink root rejected");
        std::fs::remove_file(alias).expect("remove corpus alias");
        assert_eq!(error.kind(), HarnessErrorKind::Corpus);
        assert!(error.to_string().contains("may not be a symlink"));
    }

    #[test]
    fn child_nonzero_exit_is_a_typed_harness_error() {
        let executable = std::env::current_exe().expect("test executable");
        let mut command = Command::new(executable);
        command.arg("--definitely-not-a-test-runner-option");
        let error =
            execute_bounded_child(&mut command, Duration::from_secs(2))
                .expect_err("invalid invocation exits nonzero");
        assert_eq!(error.kind(), HarnessErrorKind::ProbeExit);
    }

    #[test]
    fn child_timeout_is_a_typed_harness_error() {
        let executable = std::env::current_exe().expect("test executable");
        let mut command = Command::new(executable);
        command.args([
            "--ignored",
            "--exact",
            "harness::tests::timeout_child",
        ]);
        let error =
            execute_bounded_child(&mut command, Duration::from_millis(50))
                .expect_err("sleeping child times out");
        assert_eq!(error.kind(), HarnessErrorKind::ProbeTimeout);
    }

    #[test]
    #[ignore = "executed only as a bounded child by child_timeout_is_a_typed_harness_error"]
    fn timeout_child() {
        std::thread::sleep(Duration::from_secs(5));
        print!("late");
    }

    #[test]
    fn probe_reports_a_nonempty_outcome() {
        assert!(!probe_state_dir_bytes().expect("probe bytes").is_empty());
    }

    #[test]
    fn provider_turn_record_runs_turn_and_detach_cases() {
        let input = json!({
            "cases": [
                {
                    "turn": {
                        "provider": {"kind": "fake"},
                        "messages": [{"type": "user_message", "content": "hello"}],
                        "tools": []
                    }
                },
                {
                    "detach": {
                        "value": {"status": "success", "output": 1, "summary": "s"},
                        "maxBytes": 1024,
                        "actor": "test.tool"
                    }
                }
            ]
        });
        let record = provider_turn_record(&input).expect("record");
        assert_eq!(record["cases"][0]["turn"]["kind"], "turn");
        assert_eq!(
            record["cases"][0]["turn"]["assistantText"],
            "Siralos received: hello"
        );
        assert_eq!(record["cases"][1]["detach"]["ok"], true);
        assert_eq!(
            record["cases"][1]["detach"]["result"]["status"],
            "success"
        );
    }

    #[test]
    fn provider_turn_repeat_markers_materialize() {
        let input = json!({
            "cases": [
                {
                    "turn": {
                        "provider": {"kind": "fake"},
                        "messages": [{
                            "type": "user_message",
                            "content": {"$repeat": {"character": "a", "count": 5}}
                        }],
                        "tools": []
                    }
                }
            ]
        });
        let record = provider_turn_record(&input).expect("record");
        assert_eq!(
            record["cases"][0]["turn"]["assistantText"],
            "Siralos received: aaaaa"
        );
    }

    #[test]
    fn provider_turn_failure_records_typed_category_and_message() {
        let input = json!({
            "cases": [
                {
                    "turn": {
                        "provider": {"kind": "scripted", "events": [
                            {"type": "text_delta", "text": {"$repeat": {"character": "a", "count": 65537}}}
                        ]},
                        "messages": [{"type": "user_message", "content": "hello"}],
                        "tools": []
                    }
                }
            ]
        });
        let record = provider_turn_record(&input).expect("record");
        let turn = &record["cases"][0]["turn"];
        assert_eq!(turn["kind"], "failed");
        assert_eq!(turn["failure"], "LIMIT_ASSISTANT_TEXT_BYTES");
        assert_eq!(
            turn["message"],
            "The provider exceeded the assistant-text byte limit limit; the response was rejected."
        );
    }

    #[test]
    fn provider_turn_cancellation_script_produces_cancelled() {
        let input = json!({
            "cases": [
                {
                    "turn": {
                        "provider": {"kind": "scripted", "events": [
                            {"type": "text_delta", "text": "one"},
                            {"type": "completed"}
                        ]},
                        "messages": [{"type": "user_message", "content": "hello"}],
                        "tools": [],
                        "cancelAfterEvents": 1
                    }
                }
            ]
        });
        let record = provider_turn_record(&input).expect("record");
        assert_eq!(record["cases"][0]["turn"]["kind"], "cancelled");
    }

    #[test]
    fn provider_turn_input_validation_is_strict() {
        let input = json!({ "cases": [], "extra": true });
        assert!(validate_provider_turn_input(&input).is_err());
        let input = json!({
            "cases": [{
                "turn": {"provider": {"kind": "fake"}, "messages": [], "tools": []},
                "detach": {"value": 1, "maxBytes": 10, "actor": "a"}
            }]
        });
        assert!(validate_provider_turn_input(&input).is_err());
        let input = json!({
            "cases": [{
                "turn": {
                    "provider": {"kind": "fake"},
                    "messages": [],
                    "tools": [],
                    "unexpected": 1
                }
            }]
        });
        assert!(validate_provider_turn_input(&input).is_err());
        let input = json!({
            "cases": [{
                "turn": {
                    "provider": {"kind": "openai"},
                    "messages": [],
                    "tools": []
                }
            }]
        });
        assert!(validate_provider_turn_input(&input).is_err());
        let input = json!({
            "cases": [{
                "turn": {
                    "provider": {"kind": "scripted"},
                    "messages": [],
                    "tools": []
                }
            }]
        });
        assert!(validate_provider_turn_input(&input).is_err());
        let input = json!({
            "cases": [{
                "turn": {
                    "provider": {"kind": "fake"},
                    "messages": [{"type": "system_message", "content": "x"}],
                    "tools": []
                }
            }]
        });
        assert!(validate_provider_turn_input(&input).is_err());
        let input = json!({
            "cases": [{
                "turn": {
                    "provider": {"kind": "fake"},
                    "messages": [{"type": "user_message", "content": "hello"}],
                    "tools": []
                }
            }]
        });
        assert!(validate_provider_turn_input(&input).is_ok());
        let input = json!({
            "cases": [{
                "detach": {
                    "value": {"status": "failed", "message": "boom"},
                    "maxBytes": 128,
                    "actor": "test.tool"
                }
            }]
        });
        assert!(validate_provider_turn_input(&input).is_ok());
    }

    #[test]
    fn provider_turn_scripted_raw_events_use_the_validation_seam() {
        let input = json!({
            "cases": [
                {
                    "turn": {
                        "provider": {"kind": "scripted", "events": [
                            {"type": "unexpected", "callId": "call-1", "toolName": "workspace.read", "input": {"path": "README.md"}}
                        ]},
                        "messages": [{"type": "user_message", "content": "hello"}],
                        "tools": []
                    }
                }
            ]
        });
        let record = provider_turn_record(&input).expect("record");
        let turn = &record["cases"][0]["turn"];
        assert_eq!(turn["kind"], "failed");
        assert_eq!(turn["failure"], "UNKNOWN_EVENT_TYPE");
        assert_eq!(
            turn["message"],
            "The provider emitted an unknown event type; the response was rejected."
        );
    }

    #[test]
    fn workspace_version_matches_the_workspace_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(
            cargo_workspace_version(&root).expect("workspace version"),
            "0.0.0"
        );
    }
}

// ---------------------------------------------------------------------------
// Stage 3R R7.3 subject: context-projection.
// ---------------------------------------------------------------------------

fn context_projection_record(input: &Value) -> Result<Value, HarnessError> {
    let cases = scenario_array(input, "cases")?;
    let mut records = Vec::new();
    for case in cases {
        let case = materialize_value(case)?;
        let kind = scenario_string(&case, "kind")?;
        let entry = case.get("input").ok_or_else(|| {
            HarnessError::corpus("context-projection case requires an input")
        })?;
        let record = match kind.as_str() {
            "estimate" => run_context_estimate_case(entry)?,
            "pressure" => run_context_pressure_case(entry)?,
            "trim" => run_context_trim_case(entry)?,
            "segments" => run_context_segments_case(entry)?,
            "tool-visibility" => run_context_tool_visibility_case(entry)?,
            "evidence" => run_context_evidence_case(entry)?,
            "fingerprints" => run_context_fingerprints_case(entry)?,
            "pipeline" => run_context_pipeline_case(entry)?,
            "unsupported-tool-calling" => run_context_unsupported_case(entry)?,
            other => {
                return Err(HarnessError::corpus(format!(
                    "unknown context-projection kind {other}"
                )));
            }
        };
        records.push(json!({ "kind": kind, "result": record }));
    }
    Ok(json!({ "cases": records }))
}

fn run_context_estimate_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::estimator::{
        estimate_conversation_item_tokens, estimate_tokens,
    };
    let input = materialize_value(input)?;
    let texts = input
        .get("texts")
        .and_then(Value::as_array)
        .ok_or_else(|| HarnessError::corpus("estimate requires texts"))?;
    let mut text_estimates = Vec::new();
    for text in texts {
        let text = text.as_str().ok_or_else(|| {
            HarnessError::corpus("estimate texts must be strings")
        })?;
        let bytes = text.len();
        let tokens = estimate_tokens(text);
        text_estimates
            .push(json!({ "text": text, "bytes": bytes, "tokens": tokens }));
    }
    let mut item_estimates = Vec::new();
    if let Some(items) =
        input.get("conversationItems").and_then(Value::as_array)
    {
        for item in items {
            let ci = parse_conversation_items(std::slice::from_ref(item))?
                .into_iter()
                .next()
                .unwrap();
            let est = estimate_conversation_item_tokens(&ci);
            item_estimates.push(json!({ "item": item, "bytes": est.bytes, "tokens": est.tokens }));
        }
    }
    Ok(
        json!({ "textEstimates": text_estimates, "itemEstimates": item_estimates }),
    )
}

fn ratio_value(ratio: f64) -> Value {
    if ratio.fract() == 0.0
        && ratio.is_finite()
        && ratio >= i64::MIN as f64
        && ratio <= i64::MAX as f64
    {
        Value::Number(serde_json::Number::from(ratio as i64))
    } else if let Some(n) = serde_json::Number::from_f64(ratio) {
        Value::Number(n)
    } else {
        Value::Number(serde_json::Number::from(0))
    }
}

fn run_context_pressure_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::pressure::{
        PressureLimits, classify_pressure,
    };
    let input = materialize_value(input)?;
    let cases = input
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| HarnessError::corpus("pressure requires cases"))?;
    let limits =
        if let Some(limits) = input.get("limits").and_then(Value::as_object) {
            PressureLimits {
                warn_ratio: limits
                    .get("warnRatio")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.70),
                auto_ratio: limits
                    .get("autoRatio")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.85),
                hard_ratio: limits
                    .get("hardRatio")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            }
        } else {
            PressureLimits::default()
        };
    let mut results = Vec::new();
    for entry in cases {
        let estimated = entry
            .get("estimatedTokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                HarnessError::corpus("pressure case requires estimatedTokens")
            })? as usize;
        let working = entry
            .get("workingMaximum")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                HarnessError::corpus("pressure case requires workingMaximum")
            })?;
        let p = classify_pressure(estimated, working, limits);
        results.push(json!({ "estimatedTokens": p.estimated_tokens, "workingMaximum": p.working_maximum, "ratio": ratio_value(p.ratio), "state": p.state.as_str() }));
    }
    Ok(json!({ "results": results }))
}

fn run_context_trim_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::estimator::estimate_conversation_tokens;
    use siralos_core::projection::trim::trim_conversation_preserving_pairs;
    let input = materialize_value(input)?;
    let max_tokens = input
        .get("maxTokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| HarnessError::corpus("trim requires maxTokens"))?
        as usize;
    let messages =
        parse_conversation_items(scenario_array(&input, "messages")?)?;
    let original_tokens = estimate_conversation_tokens(&messages);
    let trimmed = trim_conversation_preserving_pairs(&messages, max_tokens);
    Ok(json!({
        "originalTokens": original_tokens,
        "maxTokens": max_tokens,
        "kept": trimmed.items.iter().map(conversation_item_value).collect::<Vec<_>>(),
        "droppedItems": trimmed.dropped_items,
        "estimatedTokens": trimmed.estimated_tokens,
    }))
}

fn run_context_segments_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::segments::{
        SegmentInput, Stability, project_segments, serialize_prefix,
    };
    let input = materialize_value(input)?;
    let segments = scenario_array(&input, "segments")?;
    let mut inputs = Vec::new();
    for seg in segments {
        let obj = seg
            .as_object()
            .ok_or_else(|| HarnessError::corpus("segments must be objects"))?;
        let stability = match obj.get("stability").and_then(Value::as_str) {
            Some("stable") => Stability::Stable,
            Some("contextual") => Stability::Contextual,
            Some("volatile") => Stability::Volatile,
            _ => {
                return Err(HarnessError::corpus(
                    "segments stability must be stable/contextual/volatile",
                ));
            }
        };
        inputs.push(SegmentInput {
            id: scenario_string(seg, "id")?,
            stability,
            title: scenario_string(seg, "title")?,
            content: seg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        });
    }
    let proj = project_segments(inputs);
    let prefix = serialize_prefix(&proj);
    Ok(json!({
        "stableSegments": proj.stable_segments.iter().map(segment_value).collect::<Vec<_>>(),
        "contextualSegments": proj.contextual_segments.iter().map(segment_value).collect::<Vec<_>>(),
        "volatileSegments": proj.volatile_segments.iter().map(segment_value).collect::<Vec<_>>(),
        "stableFingerprint": proj.stable_fingerprint,
        "stableBytes": proj.stable_bytes,
        "stablePrefixBytes": proj.stable_prefix_bytes,
        "totalBytes": proj.total_bytes,
        "estimatedTokens": proj.estimated_tokens,
        "systemPrefix": prefix,
        "systemPrefixBytes": prefix.len(),
    }))
}

fn segment_value(
    seg: &siralos_core::projection::segments::ContextSegment,
) -> Value {
    json!({ "id": seg.id, "stability": seg.stability.as_str(), "title": seg.title, "content": seg.content, "bytes": seg.bytes, "estimatedTokens": seg.estimated_tokens })
}

fn run_context_tool_visibility_case(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::projection::visibility::{
        ProjectionMode, ToolProjectionInput, project_tools,
    };
    use siralos_core::tool::capability::CapabilityId;
    use siralos_core::tool::permission::{
        PermissionPolicy, PermissionRule, PolicyRule,
    };
    let input = materialize_value(input)?;
    let mode = ProjectionMode::parse(input.get("mode").and_then(Value::as_str).unwrap_or("generic"))
        .ok_or_else(|| HarnessError::corpus("tool-visibility mode must be generic/development/review/inspection/planning"))?;
    let registered =
        parse_registered_tools(scenario_array(&input, "registeredTools")?)?;
    let policy = if let Some(rules) =
        input.get("policyRules").and_then(Value::as_array)
    {
        let mut policy_rules = Vec::new();
        for rule in rules {
            let cap = scenario_string(rule, "capability")?;
            let decision = scenario_string(rule, "decision")?;
            let r = match decision.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                "deny" => PermissionRule::Deny,
                _ => {
                    return Err(HarnessError::corpus(
                        "policy decision must be allow/ask/deny",
                    ));
                }
            };
            policy_rules.push(PolicyRule {
                capability: CapabilityId::parse(&cap).map_err(|e| {
                    HarnessError::corpus(format!(
                        "invalid capability {cap}: {e}"
                    ))
                })?,
                rule: r,
            });
        }
        PermissionPolicy::from_rules(policy_rules)
    } else {
        PermissionPolicy::default()
    };
    let allowed_names: Option<Vec<String>> =
        input.get("allowedToolNames").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        });
    let proj = project_tools(ToolProjectionInput {
        registered_tools: &registered,
        policy: &policy,
        allowed_tool_names: allowed_names.as_deref(),
        mode,
    });
    Ok(json!({
        "tools": proj.tools.iter().map(|t| json!({ "name": t.name, "visibility": t.visibility.as_str() })).collect::<Vec<_>>(),
        "counts": { "available": proj.counts.available, "gated": proj.counts.gated, "hidden": proj.counts.hidden },
        "fingerprint": proj.fingerprint,
        "requestTools": proj.request_tools.iter().map(|t| json!({ "description": t.description, "name": t.name })).collect::<Vec<_>>(),
        "approvedNames": proj.approved_names,
    }))
}

fn parse_registered_tools(
    tools: &[Value],
) -> Result<Vec<siralos_core::tool::registry::RegisteredToolInfo>, HarnessError>
{
    let mut out = Vec::new();
    for tool in tools {
        let name = scenario_string(tool, "name")?;
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let input_schema = tool
            .get("inputSchema")
            .cloned()
            .unwrap_or(json!({"type":"object"}));
        let capability_str = scenario_string(tool, "capability")?;
        let capability = siralos_core::tool::capability::CapabilityId::parse(
            &capability_str,
        )
        .map_err(|e| {
            HarnessError::corpus(format!(
                "invalid capability {capability_str}: {e}"
            ))
        })?;
        out.push(siralos_core::tool::registry::RegisteredToolInfo {
            definition: siralos_core::provider::ToolDefinition {
                name,
                description,
                input_schema,
            },
            capability,
        });
    }
    Ok(out)
}

fn run_context_evidence_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::evidence::{
        EvidenceProjectorOptions, project_for_model,
    };
    let input = materialize_value(input)?;
    let options = EvidenceProjectorOptions {
        secrets: input
            .get("secrets")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        max_total_bytes: input
            .get("maxTotalBytes")
            .and_then(Value::as_u64)
            .unwrap_or(32768) as usize,
        max_line_bytes: input
            .get("maxLineBytes")
            .and_then(Value::as_u64)
            .unwrap_or(1024) as usize,
    };
    let raw_text =
        input.get("rawText").and_then(Value::as_str).unwrap_or("").to_owned();
    let view = project_for_model(None, None, &raw_text, &options);
    let mut result = json!({
        "text": view.text,
        "truncated": view.truncated,
        "shownBytes": view.shown_bytes,
        "originalBytes": view.original_bytes,
        "transformations": view.transformations,
        "toolResultView": Value::Null,
    });
    if let Some(tr) = input.get("toolResult").and_then(Value::as_object) {
        let status =
            tr.get("status").and_then(Value::as_str).unwrap_or("failed");
        let source_text = if status == "success" {
            tr.get("summary").and_then(Value::as_str).unwrap_or("")
        } else {
            tr.get("message").and_then(Value::as_str).unwrap_or("")
        };
        let v2 = project_for_model(None, None, source_text, &options);
        if status == "success" {
            result["toolResultView"] = json!({ "status": status, "summary": v2.text, "truncated": v2.truncated, "transformations": v2.transformations, "shownBytes": v2.shown_bytes, "originalBytes": v2.original_bytes });
        } else {
            result["toolResultView"] = json!({ "status": status, "message": v2.text, "truncated": v2.truncated, "transformations": v2.transformations });
        }
    }
    Ok(result)
}

fn run_context_fingerprints_case(
    input: &Value,
) -> Result<Value, HarnessError> {
    use siralos_core::projection::segments::{
        SegmentInput, Stability, project_segments,
    };
    let input = materialize_value(input)?;
    let base_segments = scenario_array(&input, "baseSegments")?;
    let variant_segments = scenario_array(&input, "variantSegments")?;
    let parse = |segments: &[Value]| -> Result<
        siralos_core::projection::segments::ContextProjection,
        HarnessError,
    > {
        let mut inputs = Vec::new();
        for seg in segments {
            let stability = match seg.get("stability").and_then(Value::as_str)
            {
                Some("stable") => Stability::Stable,
                Some("contextual") => Stability::Contextual,
                Some("volatile") => Stability::Volatile,
                _ => {
                    return Err(HarnessError::corpus(
                        "stability must be stable/contextual/volatile",
                    ));
                }
            };
            inputs.push(SegmentInput {
                id: scenario_string(seg, "id")?,
                stability,
                title: scenario_string(seg, "title")?,
                content: seg
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        Ok(project_segments(inputs))
    };
    let base = parse(base_segments)?;
    let variant = parse(variant_segments)?;
    Ok(json!({
        "stableFingerprint": base.stable_fingerprint,
        "variantStableFingerprint": variant.stable_fingerprint,
        "stableFingerprintUnchanged": base.stable_fingerprint == variant.stable_fingerprint,
        "stableBytesUnchanged": base.stable_bytes == variant.stable_bytes,
        "stablePrefixBytesUnchanged": base.stable_prefix_bytes == variant.stable_prefix_bytes,
    }))
}

fn run_context_pipeline_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::{
        ProjectionInput, ProjectionService,
        capacity::ContextCapacity,
        evidence::EvidenceProjectorOptions,
        pressure::PressureLimits,
        segments::{SegmentInput, Stability},
        visibility::ProjectionMode,
    };
    let input = materialize_value(input)?;
    let mode = ProjectionMode::parse(
        input.get("mode").and_then(Value::as_str).unwrap_or("generic"),
    )
    .ok_or_else(|| HarnessError::corpus("pipeline mode invalid"))?;
    let working_maximum =
        input.get("workingMaximum").and_then(Value::as_i64).unwrap_or(32768);
    let capacity = ContextCapacity::with_working_maximum(working_maximum);
    let messages =
        if let Some(arr) = input.get("messages").and_then(Value::as_array) {
            parse_conversation_items(arr)?
        } else {
            Vec::new()
        };
    let registered = if let Some(arr) =
        input.get("registeredTools").and_then(Value::as_array)
    {
        parse_registered_tools(arr)?
    } else {
        Vec::new()
    };
    let segments = if let Some(arr) =
        input.get("segments").and_then(Value::as_array)
    {
        let mut v = Vec::new();
        for seg in arr {
            let stability = match seg.get("stability").and_then(Value::as_str)
            {
                Some("stable") => Stability::Stable,
                Some("contextual") => Stability::Contextual,
                Some("volatile") => Stability::Volatile,
                _ => Stability::Contextual,
            };
            v.push(SegmentInput {
                id: scenario_string(seg, "id")?,
                stability,
                title: seg
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(
                        seg.get("id").and_then(Value::as_str).unwrap_or("T"),
                    )
                    .to_owned(),
                content: seg
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        v
    } else {
        // No explicit segments: pipeline without segments is the service-composition path.
        // The oracle service's default stableInstructions for this fixture is "You are Siralos."
        // synthesized as one stable segment with the same title/content for byte-identical parity.
        vec![SegmentInput {
            id: "siralos-core-instructions".to_owned(),
            stability: Stability::Stable,
            title: "Siralos instructions".to_owned(),
            content: "You are Siralos.".to_owned(),
        }]
    };
    let policy = if let Some(rules) =
        input.get("policyRules").and_then(Value::as_array)
    {
        use siralos_core::tool::capability::CapabilityId;
        use siralos_core::tool::permission::{
            PermissionPolicy, PermissionRule, PolicyRule,
        };
        let mut prs = Vec::new();
        for rule in rules {
            let cap = scenario_string(rule, "capability")?;
            let dec = scenario_string(rule, "decision")?;
            let r = match dec.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                _ => PermissionRule::Deny,
            };
            prs.push(PolicyRule {
                capability: CapabilityId::parse(&cap)
                    .map_err(|e| HarnessError::corpus(format!("{e}")))?,
                rule: r,
            });
        }
        PermissionPolicy::from_rules(prs)
    } else {
        siralos_core::tool::permission::PermissionPolicy::default()
    };
    let allowed_names: Option<Vec<String>> =
        input.get("allowedToolNames").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                .collect()
        });
    let evidence_options = EvidenceProjectorOptions {
        secrets: input
            .get("secrets")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        max_total_bytes: input
            .get("maxTotalBytes")
            .and_then(Value::as_u64)
            .unwrap_or(32768) as usize,
        max_line_bytes: input
            .get("maxLineBytes")
            .and_then(Value::as_u64)
            .unwrap_or(1024) as usize,
    };
    let provider_tool_calling = input
        .get("providerToolCalling")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut service = ProjectionService::new();
    let req = service.project(ProjectionInput {
        mode,
        messages: &messages,
        registered_tools: &registered,
        provider_tool_calling,
        capacity,
        pressure_limits: PressureLimits::default(),
        segments,
        evidence_options,
        allowed_tool_names: allowed_names,
        policy: &policy,
        task_revision: None,
    });
    Ok(json!({
        "blocked": req.blocked.as_ref().map(|b| json!({ "type": b.kind(), "reason": b.message() })),
        "contextFingerprint": req.context_projection.stable_fingerprint,
        "estimatedTokens": req.estimated_tokens,
        "pressure": { "estimatedTokens": req.pressure.estimated_tokens, "workingMaximum": req.pressure.working_maximum, "state": req.pressure.state.as_str(), "ratio": ratio_value(req.pressure.ratio) },
        "providerCalled": req.blocked.is_none(),
        "systemPrefixBytes": req.system.as_deref().map(|s| s.len()).unwrap_or(0),
        "toolProjection": { "counts": { "available": req.tool_projection.counts.available, "gated": req.tool_projection.counts.gated, "hidden": req.tool_projection.counts.hidden }, "fingerprint": req.tool_projection.fingerprint },
        "workingMaximum": req.pressure.working_maximum,
    }))
}

fn run_context_unsupported_case(input: &Value) -> Result<Value, HarnessError> {
    use siralos_core::projection::{
        ProjectionInput, ProjectionService, capacity::ContextCapacity,
        evidence::EvidenceProjectorOptions, pressure::PressureLimits,
        visibility::ProjectionMode,
    };
    let input = materialize_value(input)?;
    let mode = ProjectionMode::parse(
        input.get("mode").and_then(Value::as_str).unwrap_or("development"),
    )
    .ok_or_else(|| HarnessError::corpus("unsupported mode invalid"))?;
    let registered = if let Some(arr) =
        input.get("registeredTools").and_then(Value::as_array)
    {
        parse_registered_tools(arr)?
    } else {
        Vec::new()
    };
    let messages =
        if let Some(arr) = input.get("messages").and_then(Value::as_array) {
            parse_conversation_items(arr)?
        } else {
            Vec::new()
        };
    let policy = if let Some(rules) =
        input.get("policyRules").and_then(Value::as_array)
    {
        use siralos_core::tool::capability::CapabilityId;
        use siralos_core::tool::permission::{
            PermissionPolicy, PermissionRule, PolicyRule,
        };
        let mut prs = Vec::new();
        for rule in rules {
            let cap = scenario_string(rule, "capability")?;
            let dec = scenario_string(rule, "decision")?;
            let r = match dec.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                _ => PermissionRule::Deny,
            };
            prs.push(PolicyRule {
                capability: CapabilityId::parse(&cap)
                    .map_err(|e| HarnessError::corpus(format!("{e}")))?,
                rule: r,
            });
        }
        PermissionPolicy::from_rules(prs)
    } else {
        siralos_core::tool::permission::PermissionPolicy::default()
    };
    let segments = if let Some(arr) =
        input.get("segments").and_then(Value::as_array)
    {
        let mut v = Vec::new();
        for seg in arr {
            let stability = match seg.get("stability").and_then(Value::as_str)
            {
                Some("stable") => {
                    siralos_core::projection::segments::Stability::Stable
                }
                Some("contextual") => {
                    siralos_core::projection::segments::Stability::Contextual
                }
                Some("volatile") => {
                    siralos_core::projection::segments::Stability::Volatile
                }
                _ => siralos_core::projection::segments::Stability::Stable,
            };
            v.push(siralos_core::projection::segments::SegmentInput {
                id: scenario_string(seg, "id")?,
                stability,
                title: seg
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Instructions")
                    .to_owned(),
                content: seg
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            });
        }
        v
    } else {
        Vec::new()
    };
    let mut service = ProjectionService::new();
    let req = service.project(ProjectionInput {
        mode,
        messages: &messages,
        registered_tools: &registered,
        provider_tool_calling: false,
        capacity: ContextCapacity::default(),
        pressure_limits: PressureLimits::default(),
        segments,
        evidence_options: EvidenceProjectorOptions::default(),
        allowed_tool_names: None,
        policy: &policy,
        task_revision: None,
    });
    Ok(json!({
        "mode": req.mode.as_str(),
        "blocked": req.blocked.as_ref().map(|b| json!({ "type": b.kind(), "reason": b.message() })),
        "toolCounts": { "available": req.tool_projection.counts.available, "gated": req.tool_projection.counts.gated, "hidden": req.tool_projection.counts.hidden },
        "fingerprint": req.tool_projection.fingerprint,
        "requestTools": req.tool_projection.request_tools.iter().map(|t| Value::String(t.name.clone())).collect::<Vec<_>>(),
        "estimatedTokens": req.estimated_tokens,
        "providerCalled": false,
    }))
}

fn conversation_item_value(
    item: &siralos_core::provider::ConversationItem,
) -> Value {
    match item {
        siralos_core::provider::ConversationItem::UserMessage { content } => {
            json!({ "type": "user_message", "content": content })
        }
        siralos_core::provider::ConversationItem::AssistantMessage {
            content,
        } => json!({ "type": "assistant_message", "content": content }),
        siralos_core::provider::ConversationItem::AssistantToolCall {
            call_id,
            tool_name,
            input,
        } => {
            if let Some(v) = input.value() {
                json!({ "type": "assistant_tool_call", "callId": call_id, "toolName": tool_name, "input": v })
            } else {
                json!({ "type": "assistant_tool_call", "callId": call_id, "toolName": tool_name })
            }
        }
        siralos_core::provider::ConversationItem::ToolResult {
            call_id,
            tool_name,
            result,
        } => {
            json!({ "type": "tool_result", "callId": call_id, "toolName": tool_name, "result": tool_execution_result_value(result) })
        }
    }
}

// ---------------------------------------------------------------------------
