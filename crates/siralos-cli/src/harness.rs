//! Differential behavioral harness — candidate runner (ADR 0033).
//!
//! Reads the scenario corpus, executes each applicable scenario against
//! the Siralos Rust candidate, and emits canonical outcome records in
//! the same deterministic format as the TypeScript oracle runner:
//! sorted-key compact JSON (`serde_json` over `BTreeMap`-backed values,
//! byte-identical to `canonicalizeJson` in `@siralos/core`).
//!
//! Environment-sensitive scenarios run in a probe subprocess with a
//! scrubbed environment (exactly the scenario's fixtures), mirroring the
//! oracle side, so both sides exercise their real environment-reading
//! code path.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Scenario `platforms` value for Windows hosts.
pub const PLATFORM_WINDOWS: &str = "windows";

/// Scenario `platforms` value for POSIX hosts.
pub const PLATFORM_POSIX: &str = "posix";

/// Subject id for state-dir resolution scenarios.
pub const SUBJECT_STATE_DIR: &str = "state-dir";

/// Subject id for product version identity scenarios.
pub const SUBJECT_VERSION_IDENTITY: &str = "version-identity";

/// The platform name of the current host, as scenario `platforms` use it.
pub fn platform_name() -> &'static str {
    if cfg!(windows) { PLATFORM_WINDOWS } else { PLATFORM_POSIX }
}

/// A harness failure (bad corpus, I/O error, unknown subject). Distinct
/// from scenario outcomes: this is an exit-2 condition, never a parity
/// deviation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError {
    detail: String,
}

impl HarnessError {
    fn new(detail: impl Into<String>) -> Self {
        Self { detail: detail.into() }
    }
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for HarnessError {}

/// The corpus manifest: scenario file names with content digests.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Corpus schema version (1).
    #[serde(rename = "schemaVersion")]
    pub schema_version: u64,
    /// Corpus content version; bumped when scenarios change.
    #[serde(rename = "corpusVersion")]
    pub corpus_version: u64,
    /// Scenario file entries in deterministic order.
    pub scenarios: Vec<ManifestEntry>,
}

/// One corpus manifest entry.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestEntry {
    /// Scenario file name relative to the corpus directory.
    pub file: String,
    /// SHA-256 of the scenario's canonical serialization.
    pub sha256: String,
}

/// One typed scenario: inputs only, never expected outputs.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    /// Stable scenario id, unique within the corpus.
    pub id: String,
    /// Subject id (`state-dir`, `version-identity`).
    pub subject: String,
    /// Platforms the scenario runs on (`windows`, `posix`, `*`).
    pub platforms: Vec<String>,
    /// `required` scenarios gate parity; `informational` are recorded
    /// but never fail the gate.
    pub parity: String,
    /// Environment fixtures for probe subprocesses (scrubbed env).
    pub env: BTreeMap<String, String>,
}

/// A scenario together with whether it applies to the current platform.
pub struct LoadedScenario {
    /// The parsed scenario.
    pub scenario: Scenario,
    /// Whether the scenario runs on the current platform.
    pub applicable: bool,
}

/// Load and validate the corpus manifest and scenario files.
pub fn load_corpus(
    corpus_dir: &Path,
    platform: &str,
) -> Result<Vec<LoadedScenario>, HarnessError> {
    let manifest_text = std::fs::read_to_string(
        corpus_dir.join("manifest.json"),
    )
    .map_err(|error| {
        HarnessError::new(format!("cannot read corpus manifest: {error}"))
    })?;
    let manifest: Manifest =
        serde_json::from_str(&manifest_text).map_err(|error| {
            HarnessError::new(format!("malformed corpus manifest: {error}"))
        })?;
    if manifest.schema_version != 1 {
        return Err(HarnessError::new(format!(
            "unsupported corpus schemaVersion {}",
            manifest.schema_version
        )));
    }
    let mut loaded = Vec::with_capacity(manifest.scenarios.len());
    for entry in &manifest.scenarios {
        let text = std::fs::read_to_string(corpus_dir.join(&entry.file))
            .map_err(|error| {
                HarnessError::new(format!(
                    "cannot read {}: {error}",
                    entry.file
                ))
            })?;
        let scenario: Scenario =
            serde_json::from_str(&text).map_err(|error| {
                HarnessError::new(format!(
                    "malformed scenario {}: {error}",
                    entry.file
                ))
            })?;
        if scenario.parity != "required" && scenario.parity != "informational"
        {
            return Err(HarnessError::new(format!(
                "scenario {} has invalid parity {}",
                scenario.id, scenario.parity
            )));
        }
        let applicable =
            scenario.platforms.iter().any(|p| p == "*" || p == platform);
        loaded.push(LoadedScenario { scenario, applicable });
    }
    Ok(loaded)
}

/// Lowercase hex SHA-256 of UTF-8 text.
fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// The probe subprocess's output: the resolved state directory, or the
/// literal marker `ERR` when resolution failed. Must stay byte-identical
/// with the oracle probe's marker.
const PROBE_ERROR_MARKER: &str = "ERR";

/// The candidate's state-dir resolution result as the probe would print
/// it. Read-only; reads the process environment.
pub fn probe_state_dir() -> String {
    match siralos_adapters::paths::state_dir() {
        Ok(path) => path.display().to_string(),
        Err(_) => PROBE_ERROR_MARKER.to_string(),
    }
}

/// Spawn this executable's `probe-state-dir` subcommand with exactly the
/// scenario's environment and return its stdout bytes.
fn spawn_state_dir_probe(
    env: &BTreeMap<String, String>,
) -> Result<Vec<u8>, HarnessError> {
    let executable = std::env::current_exe().map_err(|error| {
        HarnessError::new(format!(
            "cannot resolve current executable: {error}"
        ))
    })?;
    let output = Command::new(executable)
        .arg("probe-state-dir")
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|error| {
            HarnessError::new(format!("probe subprocess failed: {error}"))
        })?;
    Ok(output.stdout)
}

/// Build the canonical record for a state-dir scenario.
fn state_dir_record(scenario_id: &str, stdout: &[u8]) -> Value {
    let text = String::from_utf8_lossy(stdout);
    if text == PROBE_ERROR_MARKER {
        json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_STATE_DIR,
            "kind": "error",
            "stateDirSha256": null,
        })
    } else {
        json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_STATE_DIR,
            "kind": "ok",
            "stateDirSha256": sha256_hex(&text),
        })
    }
}

/// The candidate's product version from the workspace manifest.
fn cargo_workspace_version(root: &Path) -> Result<String, HarnessError> {
    let text =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(|error| {
            HarnessError::new(format!("cannot read Cargo.toml: {error}"))
        })?;
    let value: toml::Value = toml::from_str(&text).map_err(|error| {
        HarnessError::new(format!("malformed Cargo.toml: {error}"))
    })?;
    value
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(|version| version.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            HarnessError::new(
                "Cargo.toml declares no [workspace.package] version",
            )
        })
}

/// Build the canonical record for a version-identity scenario.
fn version_identity_record(
    scenario_id: &str,
    version: Result<String, HarnessError>,
) -> Value {
    match version {
        Ok(version) => json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_VERSION_IDENTITY,
            "kind": "ok",
            "version": version,
        }),
        Err(_) => json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_VERSION_IDENTITY,
            "kind": "error",
            "version": null,
        }),
    }
}

/// Execute one scenario and produce its canonical record.
pub fn run_scenario(
    scenario: &Scenario,
    root: &Path,
) -> Result<Value, HarnessError> {
    match scenario.subject.as_str() {
        SUBJECT_STATE_DIR => {
            let stdout = spawn_state_dir_probe(&scenario.env)?;
            Ok(state_dir_record(&scenario.id, &stdout))
        }
        SUBJECT_VERSION_IDENTITY => Ok(version_identity_record(
            &scenario.id,
            cargo_workspace_version(root),
        )),
        other => Err(HarnessError::new(format!("unknown subject {other}"))),
    }
}

/// Run every applicable scenario and return the canonical records text.
pub fn run_corpus(
    corpus_dir: &Path,
    root: &Path,
) -> Result<String, HarnessError> {
    let platform = platform_name();
    let scenarios = load_corpus(corpus_dir, platform)?;
    let mut records: Vec<Value> = Vec::new();
    for loaded in scenarios {
        if !loaded.applicable {
            continue;
        }
        records.push(run_scenario(&loaded.scenario, root)?);
    }
    Ok(canonical_records_text(records))
}

/// Serialize records canonically: sorted keys, compact, one array.
pub fn canonical_records_text(records: Vec<Value>) -> String {
    serde_json::to_string(&Value::Array(records))
        .expect("records are always serializable")
}

#[cfg(test)]
mod tests {
    use super::{
        PLATFORM_POSIX, PLATFORM_WINDOWS, canonical_records_text,
        cargo_workspace_version, platform_name, probe_state_dir, run_corpus,
        state_dir_record,
    };
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn platform_name_is_one_of_the_contract_values() {
        let name = platform_name();
        assert!(name == PLATFORM_WINDOWS || name == PLATFORM_POSIX);
    }

    #[test]
    fn state_dir_record_hashes_ok_outputs() {
        let record = state_dir_record(
            "state-dir.set.windows",
            b"C:\\fixture\\home\\.siralos",
        );
        assert_eq!(record["kind"], "ok");
        assert_eq!(record["stateDirSha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn state_dir_record_marks_errors_with_null_hash() {
        let record = state_dir_record("state-dir.unset.windows", b"ERR");
        assert_eq!(record["kind"], "error");
        assert!(record["stateDirSha256"].is_null());
    }

    #[test]
    fn records_serialize_with_sorted_keys_and_no_whitespace() {
        // Within each record, keys are sorted; the array preserves the
        // provided (manifest) order, which both runners share.
        let records = vec![
            json!({"scenarioId": "b", "kind": "ok"}),
            json!({"scenarioId": "a", "kind": "ok"}),
        ];
        let text = canonical_records_text(records);
        assert_eq!(
            text,
            r#"[{"kind":"ok","scenarioId":"b"},{"kind":"ok","scenarioId":"a"}]"#
        );
    }

    #[test]
    fn probe_reports_a_state_dir_path_ending_in_the_canonical_name() {
        // Test environments always have a home directory (CI and
        // developer machines); the probe output must be the resolved
        // state dir, never the error marker or an empty string.
        let output = probe_state_dir();
        assert!(!output.is_empty());
        assert_ne!(output, "ERR");
        assert!(output.ends_with(".siralos"));
    }

    #[test]
    fn workspace_version_extraction_matches_the_workspace_manifest() {
        // The real repo Cargo.toml is the fixture: its version must be
        // parseable and equal to the package.json version audited by the
        // oracle (the differential gate's version-identity subject).
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let version = cargo_workspace_version(&root)
            .expect("workspace version is extractable");
        assert_eq!(version, "0.0.0");
    }

    #[test]
    fn corpus_run_is_replay_stable() {
        // Determinism replay (assurance contract Part 11): repeated runs
        // over the same corpus produce byte-identical canonical records.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let corpus = repo.join("tests/differential/corpus");
        let first = run_corpus(&corpus, &repo).expect("corpus runs");
        for _ in 0..3 {
            assert_eq!(
                run_corpus(&corpus, &repo).expect("corpus runs"),
                first
            );
        }
    }
}
