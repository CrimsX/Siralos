//! Bounded persisted recordings store (decision 78 B1).
//!
//! On-disk shape is `{ "version": 1, "digest": "<hex>", "recordings": [...] }`
//! where each recording carries the canonical identity fields plus `body`.
//! The file is runtime DATA at `.siralos/replay-store.json`; every byte is
//! untrusted, bounded to 2 MiB serialized (with a 2 MiB aggregate body
//! budget), and digest-verified on load. Writes are
//! atomic over the established lockfile pattern (temp file + rename) and are
//! refused whole-sale when any body matches a credential shape.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use siralos_core::determinism::{
    REPLAY_STORE_MAX_RECORDINGS, REPLAY_STORE_MAX_TOTAL_BODY_BYTES,
    ReplayRecording, ReplayStore, ReplayStoreBoundsError,
    compute_replay_store_digest, validate_replay_store_bounds,
};

use crate::workspace::fs::{
    BoundedFileRead, MUTATION_TEMP_PREFIX, read_complete_file_bounded,
};

const REPLAY_STORE_VERSION: u64 = 1;
const REPLAY_STORE_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

const AWS_SAMPLE_KEY: &str = "AKIAIOSFODNN7EXAMPLE";

/// Hand-rolled credential-shape scanner mirroring `check:secrets` surface
/// patterns a-d. Returns the first matching pattern name, if any.
///
/// Patterns:
/// - `openai-key-shape`: `sk-` + >=16 of `[A-Za-z0-9_-]`
/// - `aws-access-key-shape`: `AKIA` + 16 `[0-9A-Z]` with exact allowlist
/// - `bearer-token-shape`: case-insensitive `Bearer` + whitespace + >=8 `[A-Za-z0-9._-]`
/// - `credential-assignment-shape`: case-insensitive word-boundary
///   `(api[_-]?key|secret|token|credential)\s*=\s*"(?!\$\{|env:)[^"]{8,}"`
fn body_credential_shape(body: &str) -> Option<&'static str> {
    if contains_openai_key_shape(body) {
        return Some("openai-key-shape");
    }
    if contains_aws_access_key_shape(body) {
        return Some("aws-access-key-shape");
    }
    if contains_bearer_token_shape(body) {
        return Some("bearer-token-shape");
    }
    if contains_credential_assignment_shape(body) {
        return Some("credential-assignment-shape");
    }
    if contains_literal_key_value(body) {
        return Some("literal-key-value-shape");
    }
    None
}

fn is_openai_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn contains_openai_key_shape(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"sk-") {
            let mut count = 0;
            let mut cursor = index + 3;
            while cursor < bytes.len() && is_openai_char(bytes[cursor] as char)
            {
                count += 1;
                cursor += 1;
            }
            if count >= 16 {
                return true;
            }
            index = cursor.max(index + 1);
        } else {
            index += 1;
        }
    }
    false
}

fn is_aws_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

fn contains_aws_access_key_shape(body: &str) -> bool {
    let bytes = body.as_bytes();
    let prefix = b"AKIA";
    if bytes.len() < 4 + 16 {
        return false;
    }
    for i in 0..=bytes.len().saturating_sub(20) {
        if bytes.get(i..i + 4) != Some(prefix) {
            continue;
        }
        let mut ok = true;
        for &b in &bytes[i + 4..i + 20] {
            let c = b as char;
            if !is_aws_char(c) {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        let token = bytes.get(i..i + 20);
        if token == Some(AWS_SAMPLE_KEY.as_bytes()) {
            continue;
        }
        return true;
    }
    false
}

fn is_bearer_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '.' | '_' | '-' | '+' | '/' | '=' | '~')
}

fn contains_bearer_token_shape(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let orig_bytes = body.as_bytes();
    let prefix = b"bearer";
    if bytes.len() < 6 {
        return false;
    }
    for i in 0..=bytes.len().saturating_sub(6) {
        if &bytes[i..i + 6] != prefix {
            continue;
        }
        let after = i + 6;
        if after >= bytes.len() {
            continue;
        }
        // Need at least one whitespace after "bearer"
        if !(orig_bytes[after] as char).is_ascii_whitespace() {
            continue;
        }
        let mut pos = after;
        while pos < bytes.len()
            && (orig_bytes[pos] as char).is_ascii_whitespace()
        {
            pos += 1;
        }
        let mut count = 0;
        for &b in &orig_bytes[pos..] {
            let c = b as char;
            if is_bearer_token_char(c) {
                count += 1;
            } else {
                break;
            }
        }
        if count >= 8 {
            return true;
        }
    }
    false
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn json_sensitive_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => {
            text.len() >= 8
                && !text.starts_with("${")
                && !text.starts_with("env:")
        }
        serde_json::Value::Array(values) => {
            values.iter().any(json_sensitive_value)
        }
        serde_json::Value::Object(object) => {
            object.values().any(json_sensitive_value)
        }
        _ => false,
    }
}

fn json_credential_shape(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.iter().any(|(key, value)| {
                let lower = key.to_ascii_lowercase();
                let sensitive_key = [
                    "api_key",
                    "api-key",
                    "apikey",
                    "secret",
                    "token",
                    "credential",
                    "password",
                    "passwd",
                    "private_key",
                    "private-key",
                    "access_token",
                    "access-token",
                ]
                .iter()
                .any(|keyword| lower.contains(keyword));
                (sensitive_key && json_sensitive_value(value))
                    || json_credential_shape(value)
            })
        }
        serde_json::Value::Array(values) => {
            values.iter().any(json_credential_shape)
        }
        serde_json::Value::String(text) => {
            // A tool `arguments` field carries a serialized JSON object as a
            // string, so the structural scan has to continue into it rather
            // than stopping at the raw assignment shape.
            let trimmed = text.trim_start();
            if (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(nested) =
                    serde_json::from_str::<serde_json::Value>(trimmed)
                && !matches!(nested, serde_json::Value::String(_))
            {
                return json_credential_shape(&nested);
            }
            raw_credential_assignment_shape(text)
        }
        _ => false,
    }
}

fn contains_literal_key_value(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut index = 0usize;
    while index + 4 <= bytes.len() {
        if &bytes[index..index + 4] == b"key:" {
            let suffix = &body[index + 4..];
            if suffix.chars().any(|character| {
                !character.is_whitespace() && !character.is_control()
            }) {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn contains_credential_assignment_shape(body: &str) -> bool {
    if serde_json::from_str::<serde_json::Value>(body)
        .map(|value| json_credential_shape(&value))
        .unwrap_or(false)
    {
        return true;
    }
    raw_credential_assignment_shape(body)
}

fn raw_credential_assignment_shape(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let orig = body.as_bytes();
    let keywords = [
        "api_key",
        "api-key",
        "apikey",
        "secret",
        "token",
        "credential",
        "password",
        "passwd",
        "private_key",
        "private-key",
        "access_token",
        "access-token",
    ];
    let mut i = 0usize;
    while i < bytes.len() {
        let boundary = i == 0 || !is_word_char(orig[i - 1] as char);
        if !boundary {
            i += 1;
            continue;
        }
        let matched_len = keywords
            .iter()
            .filter(|keyword| {
                bytes.len() >= i + keyword.len()
                    && &bytes[i..i + keyword.len()] == keyword.as_bytes()
            })
            .map(|keyword| keyword.len())
            .max();
        let Some(keyword_len) = matched_len else {
            i += 1;
            continue;
        };
        let mut pos = i + keyword_len;
        while pos < bytes.len() && (orig[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || orig[pos] != b'=' {
            i += 1;
            continue;
        }
        pos += 1;
        while pos < bytes.len() && (orig[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || orig[pos] != b'"' {
            i += 1;
            continue;
        }
        pos += 1;
        if pos >= bytes.len()
            || bytes.get(pos..pos.saturating_add(2)) == Some(b"${")
            || bytes.get(pos..pos.saturating_add(4)) == Some(b"env:")
        {
            i += 1;
            continue;
        }
        let search_end = orig.len().min(pos.saturating_add(64 * 1024));
        let Some(end) = orig[pos..search_end]
            .iter()
            .position(|byte| *byte == b'"')
            .map(|offset| pos + offset)
        else {
            return true;
        };
        if end - pos >= 8 {
            return true;
        }
        i = end.saturating_add(1);
    }
    false
}

/// Typed failure writing the replay store. Never contains body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStoreWriteError {
    /// A body matched a credential shape; the whole write was refused.
    CredentialShape {
        /// Index of the offending recording.
        index: usize,
        /// Pattern name that matched.
        pattern: &'static str,
    },
    /// A recording identity/body was malformed or inconsistent.
    InvalidRecording {
        /// Index of the offending recording.
        index: usize,
    },
    /// Bounded-cache violation.
    Bounds(ReplayStoreBoundsError),
    /// Canonical serialization exceeded the bounded file contract.
    SerializedBytesExceeded {
        /// Actual serialized size.
        actual: usize,
        /// Maximum accepted size.
        maximum: usize,
    },
    /// I/O failure (path-free, body-free).
    Io(String),
}

impl std::fmt::Display for ReplayStoreWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CredentialShape { index, pattern } => write!(
                f,
                "replay store write refused: body at index {index} matches credential shape {pattern}"
            ),
            Self::InvalidRecording { index } => write!(
                f,
                "replay store write refused: recording {index} has invalid identity or body"
            ),
            Self::Bounds(err) => write!(f, "replay store bounds: {err}"),
            Self::SerializedBytesExceeded { actual, maximum } => write!(
                f,
                "replay store serialized bytes exceed the bound ({actual} > {maximum})"
            ),
            Self::Io(message) => write!(f, "replay store I/O: {message}"),
        }
    }
}

impl std::error::Error for ReplayStoreWriteError {}

/// Typed failure loading the replay store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStoreLoadError {
    /// No store file exists.
    NotFound,
    /// Malformed store (capless, no content echoed).
    Malformed(String),
    /// Digest mismatch — untrusted.
    UntrustedDigest,
    /// Bounded-cache violation.
    Bounds(ReplayStoreBoundsError),
    /// I/O failure.
    Io(String),
}

impl std::fmt::Display for ReplayStoreLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "replay store not found"),
            Self::Malformed(reason) => {
                write!(f, "replay store malformed: {reason}")
            }
            Self::UntrustedDigest => {
                write!(f, "replay store untrusted: digest mismatch")
            }
            Self::Bounds(err) => write!(f, "replay store bounds: {err}"),
            Self::Io(message) => write!(f, "replay store I/O: {message}"),
        }
    }
}

impl std::error::Error for ReplayStoreLoadError {}

/// Create a parent path one component at a time and refuse links or
/// non-directory components. `create_dir_all` alone follows a pre-existing
/// symlink, which would let a replay store escape its workspace root.
fn ensure_private_parent(parent: &Path) -> Result<(), ReplayStoreWriteError> {
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let mut current = PathBuf::new();
    for component in parent.components() {
        // A Windows drive prefix/root is not a filesystem object to probe;
        // probing `C:` can fail with ERROR_INVALID_FUNCTION. Only ordinary
        // directory components are link-checked and created.
        match component {
            std::path::Component::Prefix(prefix) => {
                current.push(prefix.as_os_str());
                continue;
            }
            std::path::Component::RootDir => {
                current.push(std::path::MAIN_SEPARATOR_STR);
                continue;
            }
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                return Err(ReplayStoreWriteError::Io(
                    "store parent must not contain traversal".to_owned(),
                ));
            }
            std::path::Component::Normal(name) => current.push(name),
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(ReplayStoreWriteError::Io(
                        "store parent must be a real directory".to_owned(),
                    ));
                }
                #[cfg(unix)]
                if current == parent {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        std::fs::set_permissions(
                            &current,
                            std::fs::Permissions::from_mode(0o700),
                        )
                        .map_err(|_error| {
                            ReplayStoreWriteError::Io(
                                "store parent permissions could not be restricted"
                                    .to_owned(),
                            )
                        })?;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|_error| {
                    ReplayStoreWriteError::Io(
                        "store parent could not be created".to_owned(),
                    )
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        &current,
                        std::fs::Permissions::from_mode(0o700),
                    )
                    .map_err(|_error| {
                        ReplayStoreWriteError::Io(
                            "store parent permissions could not be restricted"
                                .to_owned(),
                        )
                    })?;
                }
            }
            Err(_error) => {
                return Err(ReplayStoreWriteError::Io(
                    "store parent is unavailable".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

/// Identity evidence for the directory that will contain a replay store.
///
/// A pathname check alone is not enough when a store is read and later
/// replaced: the same path can name a different directory between the two
/// operations.  Retain the canonical directory identity alongside the
/// operation and recheck it before every filesystem mutation.  On Unix the
/// device/inode pair is the strongest portable evidence available through
/// `std`; other targets retain the canonical directory name and still refuse
/// links/non-directories at each check.
#[derive(Clone, Eq, PartialEq)]
struct ParentIdentity {
    canonical: PathBuf,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn capture_parent_identity(
    parent: &Path,
) -> Result<ParentIdentity, &'static str> {
    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| "store parent is unavailable")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("store parent must be a real directory");
    }
    let canonical = std::fs::canonicalize(parent)
        .map_err(|_| "store parent is unavailable")?;
    Ok(ParentIdentity {
        canonical,
        #[cfg(unix)]
        device: {
            use std::os::unix::fs::MetadataExt;
            metadata.dev()
        },
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt;
            metadata.ino()
        },
    })
}

fn verify_parent_identity(
    parent: &Path,
    expected: &ParentIdentity,
) -> Result<(), &'static str> {
    if capture_parent_identity(parent)? == *expected {
        Ok(())
    } else {
        Err("store parent identity changed")
    }
}

fn check_private_parent_for_load(
    parent: &Path,
) -> Result<(), ReplayStoreLoadError> {
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    // The designated store parent is a real directory. Older replay-store
    // versions were commonly created beneath a normal 0755 workspace
    // directory, so read-only/execute legacy modes remain loadable, but a
    // group/world-writable parent is refused because an unkeyed digest cannot
    // protect against local replacement. Writers still tighten the directory
    // on the next successful write; link/non-directory refusal remains
    // fail-closed.
    let metadata = std::fs::symlink_metadata(parent).map_err(|_| {
        ReplayStoreLoadError::Io("store parent is unavailable".to_owned())
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ReplayStoreLoadError::Malformed(
            "store parent must be a real directory".to_owned(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(ReplayStoreLoadError::Io(
                "store parent is group/world writable".to_owned(),
            ));
        }
    }
    Ok(())
}

fn is_persistable_replay_recording(recording: &ReplayRecording) -> bool {
    recording
        .identity
        .status
        .is_some_and(|status| (200..300).contains(&status))
        && !recording.body.is_empty()
}

/// Write the bounded replay store atomically.
///
/// Scans every body eligible for bounded retention first; any credential
/// shape match refuses the whole write with the offending index + pattern.
/// Identity-only/non-successful
/// terminal records are metadata-only and are not persisted. Then
/// deterministic oldest-first eviction keeps the last
/// `REPLAY_STORE_MAX_RECORDINGS` that fit the 2 MiB total-bytes cap,
/// evicting from the front. Then atomic write (temp file + rename, same
/// conventions as `crates/siralos-adapters/src/lockfile.rs`) of the canonical
/// JSON with the recomputed digest. Returns the persisted count.
pub fn write_replay_store(
    path: &Path,
    recordings: &[ReplayRecording],
) -> Result<usize, ReplayStoreWriteError> {
    // 1. Bound the candidate work before inspecting provider-controlled body
    // text. The public function accepts a slice for compatibility, but the
    // persisted store can retain at most 64 records; scanning an unbounded
    // caller vector would let a large body list burn CPU and allocations
    // before the suffix cap takes effect. Select the newest persistable
    // current-session suffix first, then apply the expensive credential and
    // structural checks only to records that can actually be retained.
    let mut current_candidates: Vec<(usize, &ReplayRecording)> = recordings
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, recording)| is_persistable_replay_recording(recording))
        .take(REPLAY_STORE_MAX_RECORDINGS)
        .collect();
    current_candidates.reverse();
    let has_persistable_input = !current_candidates.is_empty();
    for (index, recording) in &current_candidates {
        if let Some(pattern) = body_credential_shape(&recording.body) {
            return Err(ReplayStoreWriteError::CredentialShape {
                index: *index,
                pattern,
            });
        }
        if recording.body.len() > crate::provider::MAX_RESPONSE_BYTES {
            return Err(ReplayStoreWriteError::Bounds(
                ReplayStoreBoundsError::TotalBodyBytesExceeded {
                    total: recording.body.len(),
                },
            ));
        }
        if crate::provider::replay::validate_replay_recording(recording, None)
            .is_err()
        {
            return Err(ReplayStoreWriteError::InvalidRecording {
                index: *index,
            });
        }
    }
    // Prepare the designated private store directory before reading an
    // existing cache. This tightens a newly created ordinary `.siralos`
    // directory rather than rejecting it on Unix, while still refusing links
    // and non-directories.
    let parent = path
        .parent()
        .filter(|candidate| !candidate.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_private_parent(parent)?;
    let parent_identity = capture_parent_identity(parent)
        .map_err(|message| ReplayStoreWriteError::Io(message.to_owned()))?;
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreWriteError::Io(message.to_owned()))?;
    // Preserve recordings from prior sessions before applying the bounded
    // cache policy. The store is a cross-session cache, not a per-flush log.
    let existing = match load_replay_store(path) {
        Ok(store) => store.recordings,
        Err(ReplayStoreLoadError::NotFound) => Vec::new(),
        Err(error) => {
            return Err(ReplayStoreWriteError::Io(match error {
                ReplayStoreLoadError::Malformed(_)
                | ReplayStoreLoadError::UntrustedDigest
                | ReplayStoreLoadError::Bounds(_) => {
                    "existing store is not trusted".to_owned()
                }
                ReplayStoreLoadError::NotFound => "store not found".to_owned(),
                ReplayStoreLoadError::Io(_) => {
                    "store is unavailable".to_owned()
                }
            }));
        }
    };
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreWriteError::Io(message.to_owned()))?;
    // Borrow both sources while selecting the newest bounded suffix. Walk
    // current-session entries first (newest to oldest), then the prior cache,
    // and restore the retained suffix to the store's oldest-first invariant.
    // Only the bounded retained suffix is cloned; the caller's full slice is
    // never copied before eviction.
    let mut kept: Vec<ReplayRecording> =
        Vec::with_capacity(REPLAY_STORE_MAX_RECORDINGS);
    let mut total = 0usize;
    for recording in current_candidates
        .iter()
        .rev()
        .map(|(_, recording)| *recording)
        .chain(existing.iter().rev())
    {
        if kept.len() >= REPLAY_STORE_MAX_RECORDINGS {
            break;
        }
        let next_total = total.saturating_add(recording.body.len());
        if next_total > REPLAY_STORE_MAX_TOTAL_BODY_BYTES {
            break;
        }
        total = next_total;
        kept.push(recording.clone());
    }
    kept.reverse();
    if kept.is_empty() && has_persistable_input {
        return Err(ReplayStoreWriteError::Bounds(
            ReplayStoreBoundsError::TotalBodyBytesExceeded {
                total: recordings
                    .last()
                    .map_or(0, |recording| recording.body.len()),
            },
        ));
    }
    // Validate final kept set (defensive: also checks count).
    if let Err(err) = validate_replay_store_bounds(&kept) {
        return Err(ReplayStoreWriteError::Bounds(err));
    }

    // Canonicalize request digests before both digest calculation and JSON
    // emission. A caller may supply uppercase hex; writing the original case
    // in the digest but lowercase in the document would make every store fail
    // its own integrity check on reload.
    for recording in &mut kept {
        recording.identity.body_sha256 =
            recording.identity.body_sha256.to_ascii_lowercase();
        if let Some(request_sha256) = recording.request_sha256.as_mut() {
            *request_sha256 = request_sha256.to_ascii_lowercase();
        }
    }

    // 3. Recompute digest.
    let digest = compute_replay_store_digest(&kept);

    // 4. Build canonical JSON (with optional usage fields when present).
    let has_usage = kept.iter().any(|r| {
        r.identity.input_tokens.is_some()
            || r.identity.output_tokens.is_some()
            || r.identity.cached_tokens.is_some()
    });
    let has_request_binding = kept.iter().any(|r| r.request_sha256.is_some());
    let recordings_json: Vec<Value> = kept
        .iter()
        .map(|recording| {
            let mut value = if has_usage {
                json!({
                    "providerId": recording.identity.provider_id,
                    "model": recording.identity.model,
                    "status": match recording.identity.status {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "bodySha256": recording.identity.body_sha256,
                    "bodyBytes": recording.identity.body_bytes,
                    "observedAtMs": match recording.identity.observed_at_ms {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "inputTokens": match recording.identity.input_tokens {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "outputTokens": match recording.identity.output_tokens {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "cachedTokens": match recording.identity.cached_tokens {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "body": recording.body,
                })
            } else {
                json!({
                    "providerId": recording.identity.provider_id,
                    "model": recording.identity.model,
                    "status": match recording.identity.status {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "bodySha256": recording.identity.body_sha256,
                    "bodyBytes": recording.identity.body_bytes,
                    "observedAtMs": match recording.identity.observed_at_ms {
                        Some(value) => json!(value),
                        None => Value::Null,
                    },
                    "body": recording.body,
                })
            };
            if has_request_binding {
                value
                    .as_object_mut()
                    .expect("replay entry is an object")
                    .insert(
                        "requestSha256".to_owned(),
                        match &recording.request_sha256 {
                            Some(value) => {
                                Value::String(value.to_ascii_lowercase())
                            }
                            None => Value::Null,
                        },
                    );
            }
            value
        })
        .collect();
    let document = json!({
        "version": REPLAY_STORE_VERSION,
        "digest": digest,
        "recordings": recordings_json,
    });
    let serialized = serde_json::to_string(&document).map_err(|_error| {
        ReplayStoreWriteError::Io("store could not be serialized".to_owned())
    })?;
    if serialized.len() > REPLAY_STORE_MAX_FILE_BYTES {
        return Err(ReplayStoreWriteError::SerializedBytesExceeded {
            actual: serialized.len(),
            maximum: REPLAY_STORE_MAX_FILE_BYTES,
        });
    }

    // 5. Atomic write: temp file + rename, same fs conventions as lockfile.
    let file_name =
        path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
            ReplayStoreWriteError::Io("store path has no file name".to_owned())
        })?;
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreWriteError::Io(message.to_owned()))?;
    let existing_digest = match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_symlink() || !metadata.is_file() =>
        {
            return Err(ReplayStoreWriteError::Io(
                "store must be a regular file; refusing symlink or special file"
                    .to_owned(),
            ));
        }
        Ok(_) => {
            match read_complete_file_bounded(path, REPLAY_STORE_MAX_FILE_BYTES)
            {
                BoundedFileRead::Complete(bytes) => {
                    Some(siralos_core::identity::sha256_hex(&bytes))
                }
                BoundedFileRead::TooLarge => {
                    return Err(
                        ReplayStoreWriteError::SerializedBytesExceeded {
                            actual: REPLAY_STORE_MAX_FILE_BYTES + 1,
                            maximum: REPLAY_STORE_MAX_FILE_BYTES,
                        },
                    );
                }
                BoundedFileRead::NotReadable => {
                    return Err(ReplayStoreWriteError::Io(
                        "store is not a regular readable file".to_owned(),
                    ));
                }
                BoundedFileRead::IoError(_error) => {
                    return Err(ReplayStoreWriteError::Io(
                        "store is not a regular readable file".to_owned(),
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_error) => {
            return Err(ReplayStoreWriteError::Io(
                "store is unavailable".to_owned(),
            ));
        }
    };
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreWriteError::Io(message.to_owned()))?;
    let staged = crate::atomic::stage_atomic(
        parent,
        file_name,
        &format!("{MUTATION_TEMP_PREFIX}replay-store"),
        serialized.as_bytes(),
        Some(0o600),
    )
    .map_err(|_error| {
        ReplayStoreWriteError::Io("store could not be staged".to_owned())
    })?;
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreWriteError::Io(message.to_owned()))?;
    let commit_result = if let Some(digest) = existing_digest.as_deref() {
        staged.commit_if_digest(digest)
    } else {
        staged.commit_if_absent()
    };
    commit_result.map_err(|error| match error {
        crate::atomic::AtomicWriteFailure::TargetIsNotARegularFile {
            ..
        } => ReplayStoreWriteError::Io(
            "store must be a regular file; refusing symlink or special file"
                .to_owned(),
        ),
        crate::atomic::AtomicWriteFailure::TargetUnreadable { .. } => {
            ReplayStoreWriteError::Io("store is unreadable".to_owned())
        }
        crate::atomic::AtomicWriteFailure::ReplaceFailed { .. } => {
            ReplayStoreWriteError::Io("store could not be replaced".to_owned())
        }
        _ => ReplayStoreWriteError::Io(
            "store could not be committed".to_owned(),
        ),
    })?;

    Ok(kept.len())
}

/// Load the bounded replay store, treating every byte as untrusted.
///
/// Bounded read (file > 2 MiB -> Malformed), absent -> NotFound, bad
/// JSON/shape -> Malformed, digest mismatch -> UntrustedDigest, bounds
/// violation -> Bounds. Never auto-repairs or deletes.
pub fn load_replay_store(
    path: &Path,
) -> Result<ReplayStore, ReplayStoreLoadError> {
    // Distinguish absent from other states without opening.
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ReplayStoreLoadError::NotFound);
        }
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(ReplayStoreLoadError::Malformed(
                    "store must be a regular file".to_owned(),
                ));
            }
        }
        Err(_error) => {
            return Err(ReplayStoreLoadError::Io(
                "store is unavailable".to_owned(),
            ));
        }
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    check_private_parent_for_load(parent)?;
    let parent_identity = capture_parent_identity(parent)
        .map_err(|message| ReplayStoreLoadError::Io(message.to_owned()))?;
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreLoadError::Io(message.to_owned()))?;

    let bytes =
        match read_complete_file_bounded(path, REPLAY_STORE_MAX_FILE_BYTES) {
            BoundedFileRead::Complete(bytes) => bytes,
            BoundedFileRead::TooLarge => {
                return Err(ReplayStoreLoadError::Malformed(
                    "store exceeds the 2 MiB cap".to_owned(),
                ));
            }
            BoundedFileRead::NotReadable => {
                return Err(ReplayStoreLoadError::Malformed(
                    "store is not readable".to_owned(),
                ));
            }
            BoundedFileRead::IoError(_error) => {
                return Err(ReplayStoreLoadError::Io(
                    "store is unavailable".to_owned(),
                ));
            }
        };
    verify_parent_identity(parent, &parent_identity)
        .map_err(|message| ReplayStoreLoadError::Io(message.to_owned()))?;

    let text = String::from_utf8(bytes).map_err(|_| {
        ReplayStoreLoadError::Malformed("store is not valid UTF-8".to_owned())
    })?;

    let value: Value = serde_json::from_str(&text).map_err(|_| {
        ReplayStoreLoadError::Malformed("store is not valid JSON".to_owned())
    })?;

    let obj = value.as_object().ok_or_else(|| {
        ReplayStoreLoadError::Malformed("store must be an object".to_owned())
    })?;
    if obj.keys().any(|key| {
        !matches!(key.as_str(), "version" | "digest" | "recordings")
    }) {
        return Err(ReplayStoreLoadError::Malformed(
            "store contains an unknown field".to_owned(),
        ));
    }

    // version must be 1
    match obj.get("version") {
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        _ => {
            return Err(ReplayStoreLoadError::Malformed(
                "store version must be 1".to_owned(),
            ));
        }
    }

    let digest = obj
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ReplayStoreLoadError::Malformed(
                "store requires a digest string".to_owned(),
            )
        })?
        .to_owned();

    let recordings_val = obj.get("recordings").ok_or_else(|| {
        ReplayStoreLoadError::Malformed("store requires recordings".to_owned())
    })?;
    let arr = recordings_val.as_array().ok_or_else(|| {
        ReplayStoreLoadError::Malformed(
            "store recordings must be an array".to_owned(),
        )
    })?;

    if arr.len() > siralos_core::determinism::REPLAY_STORE_MAX_RECORDINGS {
        return Err(ReplayStoreLoadError::Bounds(
            ReplayStoreBoundsError::TooManyRecordings { count: arr.len() },
        ));
    }
    let mut recordings = Vec::with_capacity(arr.len());
    let mut total_body_bytes = 0usize;
    for entry in arr {
        let table = entry.as_object().ok_or_else(|| {
            ReplayStoreLoadError::Malformed(
                "each recording must be an object".to_owned(),
            )
        })?;
        if table.keys().any(|key| {
            !matches!(
                key.as_str(),
                "providerId"
                    | "model"
                    | "status"
                    | "bodySha256"
                    | "bodyBytes"
                    | "observedAtMs"
                    | "inputTokens"
                    | "outputTokens"
                    | "cachedTokens"
                    | "requestSha256"
                    | "body"
            )
        }) {
            return Err(ReplayStoreLoadError::Malformed(
                "recording contains an unknown field".to_owned(),
            ));
        }
        let provider_id = table
            .get("providerId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires providerId".to_owned(),
                )
            })?
            .to_owned();
        if !crate::provider::replay::valid_replay_identifier(&provider_id, 256)
        {
            return Err(ReplayStoreLoadError::Malformed(
                "recording providerId is invalid".to_owned(),
            ));
        }
        let model = table
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires model".to_owned(),
                )
            })?
            .to_owned();
        if !crate::provider::replay::valid_replay_identifier(&model, 256) {
            return Err(ReplayStoreLoadError::Malformed(
                "recording model is invalid".to_owned(),
            ));
        }
        let status = match table.get("status") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => {
                let v = n.as_u64().ok_or_else(|| {
                    ReplayStoreLoadError::Malformed(
                        "recording status must be a number".to_owned(),
                    )
                })?;
                if v > u16::MAX as u64 {
                    return Err(ReplayStoreLoadError::Malformed(
                        "recording status out of range".to_owned(),
                    ));
                }
                Some(v as u16)
            }
            Some(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording status must be a number or null".to_owned(),
                ));
            }
        };
        if !status.is_some_and(|value| (200..300).contains(&value)) {
            return Err(ReplayStoreLoadError::Malformed(
                "recording status must be a successful HTTP status".to_owned(),
            ));
        }
        let body_sha256 = table
            .get("bodySha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires bodySha256".to_owned(),
                )
            })?
            .to_ascii_lowercase();
        let body_bytes = table
            .get("bodyBytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires bodyBytes".to_owned(),
                )
            })?;
        let observed_at_ms = match table.get("observedAtMs") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_u64().ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording observedAtMs must be a number".to_owned(),
                )
            })?),
            Some(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording observedAtMs must be a number or null"
                        .to_owned(),
                ));
            }
        };
        // Usage fields: optional, backward-compatible with pre-102 stores.
        let input_tokens = match table.get("inputTokens") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_u64().ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording inputTokens must be a number".to_owned(),
                )
            })?),
            Some(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording inputTokens must be a number or null"
                        .to_owned(),
                ));
            }
        };
        let output_tokens = match table.get("outputTokens") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_u64().ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording outputTokens must be a number".to_owned(),
                )
            })?),
            Some(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording outputTokens must be a number or null"
                        .to_owned(),
                ));
            }
        };
        let cached_tokens = match table.get("cachedTokens") {
            None | Some(Value::Null) => None,
            Some(Value::Number(n)) => Some(n.as_u64().ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording cachedTokens must be a number".to_owned(),
                )
            })?),
            Some(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording cachedTokens must be a number or null"
                        .to_owned(),
                ));
            }
        };
        let request_sha256 = match table.get("requestSha256") {
            None | Some(Value::Null) => None,
            Some(Value::String(value))
                if value.len() == 64
                    && value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Some(value.to_ascii_lowercase())
            }
            Some(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording requestSha256 must be 64 hex characters"
                        .to_owned(),
                ));
            }
        };
        let body = table
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires body".to_owned(),
                )
            })?
            .to_owned();
        total_body_bytes = total_body_bytes.saturating_add(body.len());
        if total_body_bytes > REPLAY_STORE_MAX_TOTAL_BODY_BYTES {
            return Err(ReplayStoreLoadError::Bounds(
                ReplayStoreBoundsError::TotalBodyBytesExceeded {
                    total: total_body_bytes,
                },
            ));
        }
        if body.len() > crate::provider::MAX_RESPONSE_BYTES {
            return Err(ReplayStoreLoadError::Bounds(
                ReplayStoreBoundsError::TotalBodyBytesExceeded {
                    total: body.len(),
                },
            ));
        }
        let candidate = ReplayRecording {
            identity: siralos_core::determinism::ProviderResponseIdentity {
                provider_id,
                model,
                status,
                body_sha256,
                body_bytes,
                observed_at_ms,
                input_tokens,
                output_tokens,
                cached_tokens,
            },
            body,
            request_sha256,
        };
        match crate::provider::replay::validate_replay_recording(
            &candidate,
            None,
        ) {
            Ok(()) => {}
            Err(
                crate::provider::replay::ReplayRecordingValidationError::BodyBytes
                | crate::provider::replay::ReplayRecordingValidationError::BodyDigest,
            ) => return Err(ReplayStoreLoadError::UntrustedDigest),
            Err(
                crate::provider::replay::ReplayRecordingValidationError::BodyTooLarge,
            ) => {
                return Err(ReplayStoreLoadError::Bounds(
                    ReplayStoreBoundsError::TotalBodyBytesExceeded {
                        total: candidate.body.len(),
                    },
                ));
            }
            Err(_) => {
                return Err(ReplayStoreLoadError::Malformed(
                    "recording failed replay validation".to_owned(),
                ));
            }
        }
        if body_credential_shape(&candidate.body).is_some() {
            return Err(ReplayStoreLoadError::Malformed(
                "recording body contains credential-shaped data".to_owned(),
            ));
        }
        recordings.push(candidate);
    }

    // Verify digest.
    let recomputed = compute_replay_store_digest(&recordings);
    if recomputed != digest {
        return Err(ReplayStoreLoadError::UntrustedDigest);
    }

    // Validate bounds.
    if let Err(err) = validate_replay_store_bounds(&recordings) {
        return Err(ReplayStoreLoadError::Bounds(err));
    }

    Ok(ReplayStore { recordings })
}

#[cfg(test)]
mod tests {
    use super::{
        body_credential_shape, load_replay_store, write_replay_store,
    };
    #[cfg(unix)]
    use super::{capture_parent_identity, verify_parent_identity};
    use siralos_core::determinism::{
        ProviderResponseIdentity, ReplayRecording, compute_replay_store_digest,
    };

    fn recording_with_body(id: usize, body: &str) -> ReplayRecording {
        ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: format!("p{id}"),
                model: format!("m{id}"),
                status: Some(200),
                body_sha256: siralos_core::identity::sha256_hex(
                    body.as_bytes(),
                ),
                body_bytes: body.len() as u64,
                observed_at_ms: Some(id as u64),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: body.to_owned(),
            request_sha256: None,
        }
    }

    fn replay_text_body(text: &str) -> String {
        serde_json::json!({
            "choices": [{
                "message": {"role": "assistant", "content": text}
            }]
        })
        .to_string()
    }

    fn temp_store_path(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let base = std::env::temp_dir()
            .join(format!("siralos-replay-{name}-{nonce}"));
        std::fs::create_dir_all(&base).expect("temp root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &base,
                std::fs::Permissions::from_mode(0o700),
            )
            .expect("private temp root");
        }
        base.join("replay-store.json")
    }

    #[test]
    fn write_refuses_parent_traversal_in_the_store_path() {
        let path = temp_store_path("traversal").join("..").join("escape.json");
        let error = write_replay_store(&path, &[]).expect_err("traversal");
        assert!(matches!(error, super::ReplayStoreWriteError::Io(_)));
    }

    #[cfg(unix)]
    #[test]
    fn parent_identity_check_rejects_a_replaced_directory() {
        let path = temp_store_path("parent-identity");
        let parent = path.parent().expect("parent");
        let expected =
            capture_parent_identity(parent).expect("initial parent");
        std::fs::remove_dir(parent).expect("remove parent");
        std::fs::create_dir(parent).expect("replacement parent");
        assert!(verify_parent_identity(parent, &expected).is_err());
    }

    #[test]
    fn round_trip_write_load_preserves_recordings_and_digest() {
        let path = temp_store_path("roundtrip");
        let recordings = vec![
            recording_with_body(0, &replay_text_body("hello world")),
            recording_with_body(1, &replay_text_body("second")),
        ];
        let count = write_replay_store(&path, &recordings).expect("write");
        assert_eq!(count, 2);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings, recordings);
        // digest is internally verified; write then tamper would fail.
    }

    #[cfg(unix)]
    #[test]
    fn load_accepts_legacy_store_under_a_non_private_parent() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_store_path("legacy-parent-mode");
        let body = replay_text_body("legacy");
        let recording = recording_with_body(0, &body);
        write_replay_store(&path, std::slice::from_ref(&recording))
            .expect("write");
        std::fs::set_permissions(
            path.parent().expect("parent"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("legacy parent mode");
        let loaded = load_replay_store(&path).expect("legacy load");
        assert_eq!(loaded.recordings, vec![recording]);
    }

    #[cfg(unix)]
    #[test]
    fn load_rejects_a_group_or_world_writable_legacy_parent() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_store_path("writable-parent");
        let body = replay_text_body("legacy");
        let recording = recording_with_body(0, &body);
        write_replay_store(&path, std::slice::from_ref(&recording))
            .expect("write");
        std::fs::set_permissions(
            path.parent().expect("parent"),
            std::fs::Permissions::from_mode(0o777),
        )
        .expect("writable parent mode");
        assert!(matches!(
            load_replay_store(&path),
            Err(super::ReplayStoreLoadError::Io(_))
        ));
    }

    #[test]
    fn uppercase_hash_fields_are_canonicalized_before_write_and_digest() {
        let path = temp_store_path("uppercase-digests");
        let body = replay_text_body("canonical");
        let mut recording = recording_with_body(0, &body);
        recording.identity.body_sha256 =
            recording.identity.body_sha256.to_ascii_uppercase();
        recording.request_sha256 = Some("ABCDEF0123456789".repeat(4));
        let count = write_replay_store(&path, &[recording]).expect("write");
        assert_eq!(count, 1);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(
            loaded.recordings[0].identity.body_sha256,
            loaded.recordings[0].identity.body_sha256.to_ascii_lowercase()
        );
        assert_eq!(
            loaded.recordings[0].request_sha256,
            Some("abcdef0123456789".repeat(4))
        );
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(!text.contains(&"ABCDEF0123456789".repeat(4)));
    }

    #[test]
    fn writer_and_loader_share_control_character_rejection() {
        let path = temp_store_path("control-text");
        let body = replay_text_body("\u{0001}");
        let recording = recording_with_body(0, &body);
        let error =
            write_replay_store(&path, std::slice::from_ref(&recording))
                .expect_err("control text is not replay-safe");
        assert!(matches!(
            error,
            super::ReplayStoreWriteError::InvalidRecording { index: 0 }
        ));

        let digest =
            compute_replay_store_digest(std::slice::from_ref(&recording));
        let document = serde_json::json!({
            "version": super::REPLAY_STORE_VERSION,
            "digest": digest,
            "recordings": [{
                "providerId": recording.identity.provider_id,
                "model": recording.identity.model,
                "status": recording.identity.status,
                "bodySha256": recording.identity.body_sha256,
                "bodyBytes": recording.identity.body_bytes,
                "observedAtMs": recording.identity.observed_at_ms,
                "body": recording.body,
            }],
        });
        std::fs::write(&path, document.to_string()).expect("write raw store");
        assert!(matches!(
            load_replay_store(&path),
            Err(super::ReplayStoreLoadError::Malformed(_))
        ));
    }

    #[test]
    fn tamper_one_body_byte_is_untrusted() {
        let path = temp_store_path("tamper");
        let body = replay_text_body("hello tamper");
        let recordings = vec![recording_with_body(0, &body)];
        write_replay_store(&path, &recordings).expect("write");
        // Modify one byte in file.
        let text = std::fs::read_to_string(&path).expect("read");
        let tampered = text.replacen("hello", "hallo", 1);
        std::fs::write(&path, tampered).expect("rewrite");
        let err = load_replay_store(&path).expect_err("untrusted");
        assert_eq!(err, super::ReplayStoreLoadError::UntrustedDigest);
    }

    #[test]
    fn credential_rejection_openai_key_shape() {
        let path = temp_store_path("openai");
        let secret = "sk-abcdefghijklmnopqrstu"; // 21 >=16
        let rec = recording_with_body(0, secret);
        let err = write_replay_store(&path, &[rec]).expect_err("refused");
        match err {
            super::ReplayStoreWriteError::CredentialShape {
                index,
                pattern,
            } => {
                assert_eq!(index, 0);
                assert_eq!(pattern, "openai-key-shape");
            }
            other => panic!("wrong error {other:?}"),
        }
        // Ensure no file was created or is empty/not readable as success.
        assert!(!path.exists() || load_replay_store(&path).is_err());
    }

    #[test]
    fn credential_rejection_aws_access_key_shape() {
        let path = temp_store_path("aws");
        let key = format!("{}{}", "AKIA", "1234567890ABCDEF"); // 20
        assert_ne!(key, super::AWS_SAMPLE_KEY);
        let rec = recording_with_body(0, &format!("key {key} here"));
        let err = write_replay_store(&path, &[rec]).expect_err("refused");
        match err {
            super::ReplayStoreWriteError::CredentialShape {
                pattern, ..
            } => assert_eq!(pattern, "aws-access-key-shape"),
            other => panic!("wrong error {other:?}"),
        }
    }

    #[test]
    fn allowlisted_aws_sample_is_accepted() {
        let path = temp_store_path("aws-allow");
        let body =
            replay_text_body(&format!("sample {}", super::AWS_SAMPLE_KEY));
        let rec = recording_with_body(0, &body);
        let count = write_replay_store(&path, &[rec]).expect("allowed");
        assert_eq!(count, 1);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings[0].body, body);
    }

    #[test]
    fn credential_rejection_bearer_token_shape() {
        let path = temp_store_path("bearer");
        let body = "Authorization: Bearer abcdefgh1234._-";
        let rec = recording_with_body(0, body);
        let err = write_replay_store(&path, &[rec]).expect_err("refused");
        match err {
            super::ReplayStoreWriteError::CredentialShape {
                pattern, ..
            } => assert_eq!(pattern, "bearer-token-shape"),
            other => panic!("wrong error {other:?}"),
        }
        // case-insensitive
        let path2 = temp_store_path("bearer-ci");
        let rec2 = recording_with_body(0, "bearer ABCDEFGH1234");
        let err2 = write_replay_store(&path2, &[rec2]).expect_err("refused");
        match err2 {
            super::ReplayStoreWriteError::CredentialShape {
                pattern, ..
            } => assert_eq!(pattern, "bearer-token-shape"),
            other => panic!("wrong error {other:?}"),
        }
    }

    #[test]
    fn credential_rejection_credential_assignment_shape() {
        let path = temp_store_path("cred-assign");
        let body = r#"api_key = "supersecret12345""#;
        let rec = recording_with_body(0, body);
        let err = write_replay_store(&path, &[rec]).expect_err("refused");
        match err {
            super::ReplayStoreWriteError::CredentialShape {
                pattern, ..
            } => assert_eq!(pattern, "credential-assignment-shape"),
            other => panic!("wrong error {other:?}"),
        }
    }

    #[test]
    fn credential_assignment_env_and_var_escapes_accepted() {
        let path = temp_store_path("cred-escape");
        // env: escape should be accepted
        let rec1 = recording_with_body(
            0,
            &replay_text_body(r#"api_key = "env:MY_SECRET""#),
        );
        let rec2 =
            recording_with_body(1, &replay_text_body(r#"secret = "${VAR}""#));
        let count = write_replay_store(&path, &[rec1, rec2]).expect("allowed");
        assert_eq!(count, 2);
    }

    #[test]
    fn eviction_65_recordings_oldest_dropped_deterministic() {
        let path = temp_store_path("evict-65");
        let body = replay_text_body("x");
        let recordings: Vec<_> =
            (0..65).map(|i| recording_with_body(i, &body)).collect();
        let count = write_replay_store(&path, &recordings).expect("write");
        assert_eq!(count, 64);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings.len(), 64);
        // Oldest dropped: first remaining should be original index 1
        assert_eq!(loaded.recordings[0].identity.provider_id, "p1");
        assert_eq!(loaded.recordings[63].identity.provider_id, "p64");
        // Deterministic: second write with same input yields same digest.
        let path2 = temp_store_path("evict-65b");
        write_replay_store(&path2, &recordings).expect("write2");
        let a = std::fs::read_to_string(&path).expect("read a");
        let b = std::fs::read_to_string(&path2).expect("read b");
        // digests inside files must match (parse)
        let va: serde_json::Value = serde_json::from_str(&a).expect("json a");
        let vb: serde_json::Value = serde_json::from_str(&b).expect("json b");
        assert_eq!(va["digest"], vb["digest"]);
    }

    #[test]
    fn existing_cache_does_not_starve_current_session_recordings() {
        let path = temp_store_path("existing-cache");
        let body = replay_text_body("cache");
        let initial: Vec<_> =
            (0..64).map(|i| recording_with_body(i, &body)).collect();
        write_replay_store(&path, &initial).expect("initial write");

        let current = recording_with_body(100, &body);
        let count =
            write_replay_store(&path, &[current]).expect("current write");
        assert_eq!(count, 64);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings[0].identity.provider_id, "p1");
        assert_eq!(loaded.recordings[62].identity.provider_id, "p63");
        assert_eq!(loaded.recordings[63].identity.provider_id, "p100");
    }

    #[test]
    fn terminal_identity_only_record_does_not_poison_valid_suffix() {
        let path = temp_store_path("terminal-suffix");
        let valid_body = replay_text_body("valid");
        let valid = recording_with_body(0, &valid_body);
        let terminal = ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: "p-terminal".to_owned(),
                model: "m-terminal".to_owned(),
                status: None,
                body_sha256: siralos_core::identity::sha256_hex(b""),
                body_bytes: 0,
                observed_at_ms: Some(99),
                input_tokens: None,
                output_tokens: None,
                cached_tokens: None,
            },
            body: String::new(),
            request_sha256: None,
        };
        let count =
            write_replay_store(&path, &[terminal, valid]).expect("write");
        assert_eq!(count, 1);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings.len(), 1);
        assert_eq!(loaded.recordings[0].body, valid_body);
    }

    #[test]
    fn over_bytes_eviction() {
        let path = temp_store_path("over-bytes");
        // Create recordings each 768 KiB -> 3 total ~2.25 MiB > 2 MiB cap.
        // Keep last 2 (1.5 MiB) to stay under both body and file caps.
        let body = replay_text_body(&"a".repeat(768 * 1024));
        let recordings: Vec<_> =
            (0..3).map(|i| recording_with_body(i, &body)).collect();
        let count = write_replay_store(&path, &recordings).expect("write");
        // Should evict oldest first until fits: keep last 2 (1.5 MiB)
        assert_eq!(count, 2);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings.len(), 2);
        assert_eq!(loaded.recordings[0].identity.provider_id, "p1");
    }

    #[test]
    fn over_cap_file_read_is_malformed() {
        let path = temp_store_path("over-cap-file");
        // Write a file larger than the serialized 2 MiB cap directly
        // (bypass the writer).
        let big = "x".repeat(super::REPLAY_STORE_MAX_FILE_BYTES + 1);
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, big).expect("write big");
        let err = load_replay_store(&path).expect_err("malformed");
        match err {
            super::ReplayStoreLoadError::Malformed(reason) => {
                assert!(reason.contains("2 MiB cap"));
            }
            other => panic!("wrong error {other:?}"),
        }
    }

    #[test]
    fn absent_is_not_found() {
        let path = temp_store_path("absent");
        // Ensure absent
        let _ = std::fs::remove_file(&path);
        let err = load_replay_store(&path).expect_err("not found");
        assert_eq!(err, super::ReplayStoreLoadError::NotFound);
    }

    #[test]
    fn malformed_json_is_malformed() {
        let path = temp_store_path("malformed-json");
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(&path, "{ not json").expect("write");
        let err = load_replay_store(&path).expect_err("malformed");
        match err {
            super::ReplayStoreLoadError::Malformed(_) => {}
            other => panic!("wrong error {other:?}"),
        }
    }

    #[test]
    fn no_error_variant_ever_contains_body_text() {
        let secret_body = "sk-abcdefghijklmnopqrstu";
        let path = temp_store_path("no-echo");
        let rec = recording_with_body(0, secret_body);
        let err = write_replay_store(&path, &[rec]).expect_err("refused");
        let display = format!("{err}");
        assert!(
            !display.contains(secret_body),
            "error echoed body: {display}"
        );

        // Also test load errors don't echo file content with secret.
        // Write a valid file then tamper to cause UntrustedDigest with secret inside.
        let path2 = temp_store_path("no-echo-load");
        let ok = recording_with_body(0, &replay_text_body("hello"));
        write_replay_store(&path2, &[ok]).expect("write");
        let text = std::fs::read_to_string(&path2).expect("read");
        // Insert secret into file body field manually then expect UntrustedDigest without echo
        let tampered = text.replace("hello", secret_body);
        std::fs::write(&path2, tampered).expect("rewrite");
        let err2 = load_replay_store(&path2).expect_err("untrusted");
        let display2 = format!("{err2}");
        assert!(
            !display2.contains(secret_body),
            "load error echoed body: {display2}"
        );
    }

    #[test]
    fn body_credential_shape_first_match_wins() {
        // Body contains both sk- and bearer; openai should win.
        let body = "sk-abcdefghijklmnopqrstu and Bearer abcdefgh1234";
        assert_eq!(body_credential_shape(body), Some("openai-key-shape"));
    }

    #[test]
    fn credential_shape_helpers_are_hand_rolled() {
        // Ensure helpers don't use regex crate by checking basic cases.
        assert_eq!(body_credential_shape("nothing here"), None);
        assert_eq!(body_credential_shape("sk-short"), None); // <16 after sk-
        assert_eq!(body_credential_shape("AKIA123"), None); // <16 after AKIA
        assert_eq!(body_credential_shape("Bearer short"), None); // <8 token
        assert_eq!(body_credential_shape(r#"api_key = "short""#), None); // <8 inside quotes
    }
}
