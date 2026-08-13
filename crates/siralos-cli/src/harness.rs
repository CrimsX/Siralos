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
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Scenario platform value for Windows hosts.
pub const PLATFORM_WINDOWS: &str = "windows";

/// Scenario platform value for POSIX hosts.
pub const PLATFORM_POSIX: &str = "posix";

const SUBJECT_STATE_DIR: &str = "state-dir";
const SUBJECT_VERSION_IDENTITY: &str = "version-identity";
const CORPUS_SCHEMA_VERSION: u64 = 1;
const CORPUS_VERSION: u64 = 3;
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
    detail: String,
}

impl HarnessError {
    /// Stable machine-branchable error category.
    pub fn kind(&self) -> HarnessErrorKind {
        self.kind
    }

    fn new(kind: HarnessErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u64,
    #[serde(rename = "corpusVersion")]
    corpus_version: u64,
    scenarios: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    file: String,
    sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    id: String,
    subject: String,
    platforms: Vec<String>,
    parity: String,
    env: BTreeMap<String, String>,
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
        SUBJECT_STATE_DIR | SUBJECT_VERSION_IDENTITY
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
        return Err(HarnessError::corpus(format!(
            "unsupported corpus schemaVersion {}",
            manifest.schema_version
        )));
    }
    if manifest.corpus_version != CORPUS_VERSION {
        return Err(HarnessError::corpus(format!(
            "unsupported corpusVersion {}",
            manifest.corpus_version
        )));
    }
    if manifest.scenarios.is_empty()
        || manifest.scenarios.len() > MAX_SCENARIOS
    {
        return Err(HarnessError::corpus(format!(
            "corpus must contain 1-{MAX_SCENARIOS} scenarios"
        )));
    }

    let mut files = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut loaded = Vec::with_capacity(manifest.scenarios.len());
    for entry in manifest.scenarios {
        if !valid_file_name(&entry.file) || !valid_sha256(&entry.sha256) {
            return Err(HarnessError::corpus(
                "manifest entry has an invalid file name or digest",
            ));
        }
        if !files.insert(entry.file.clone()) {
            return Err(HarnessError::corpus(format!(
                "corpus contains duplicate file {}",
                entry.file
            )));
        }
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
        if canonical_scenario_digest(&scenario)? != entry.sha256 {
            return Err(HarnessError::corpus(format!(
                "scenario {} does not match its manifest digest",
                entry.file
            )));
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
    format!("{:x}", hasher.finalize())
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
            "kind": "error",
            "stateDirSha256": null,
        })
    } else {
        json!({
            "scenarioId": scenario_id,
            "subject": SUBJECT_STATE_DIR,
            "kind": "ok",
            "stateDirSha256": sha256_hex(stdout),
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

fn run_scenario(
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
        _ => unreachable!("subject was validated while loading the corpus"),
    }
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
) -> Result<String, HarnessError> {
    let scenarios = load_corpus(corpus_dir, platform_name())?;
    let mut records = Vec::new();
    for loaded in scenarios {
        if loaded.applicable {
            records.push(run_scenario(&loaded.scenario, root)?);
        }
    }
    Ok(canonical_records_text(records))
}

fn canonical_records_text(records: Vec<Value>) -> String {
    let text = serde_json::to_string(&Value::Array(records))
        .expect("outcome records are constructed from serializable values");
    format!("{text}\n")
}

#[cfg(test)]
mod tests {
    use super::{
        HarnessErrorKind, PLATFORM_POSIX, PLATFORM_WINDOWS,
        canonical_records_text, canonical_scenario_digest,
        cargo_workspace_version, execute_bounded_child, load_corpus,
        platform_name, probe_state_dir_bytes, state_dir_record,
        validate_scenario,
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
        assert_eq!(record["kind"], "ok");
        assert_eq!(record["stateDirSha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn state_dir_record_marks_only_the_exact_error_marker() {
        let record = state_dir_record("state-dir.unset.windows", b"ERR");
        assert_eq!(record["kind"], "error");
        assert!(record["stateDirSha256"].is_null());

        let non_utf8 = state_dir_record("state-dir.set.posix", &[0xff]);
        assert_eq!(non_utf8["kind"], "ok");
    }

    #[test]
    fn records_are_canonical_with_one_trailing_newline() {
        let records = vec![
            json!({"scenarioId": "b", "kind": "ok"}),
            json!({"scenarioId": "a", "kind": "ok"}),
        ];
        assert_eq!(
            canonical_records_text(records),
            "[{\"kind\":\"ok\",\"scenarioId\":\"b\"},{\"kind\":\"ok\",\"scenarioId\":\"a\"}]\n"
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
        assert_eq!(loaded.len(), 7);
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
        assert!(error.to_string().contains("manifest digest"));
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
    fn workspace_version_matches_the_workspace_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(
            cargo_workspace_version(&root).expect("workspace version"),
            "0.0.0"
        );
    }
}
