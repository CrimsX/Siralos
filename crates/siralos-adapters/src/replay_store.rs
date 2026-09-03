//! Bounded persisted recordings store (Stage 8, decision 78 B1).
//!
//! On-disk shape is `{ "version": 1, "digest": "<hex>", "recordings": [...] }`
//! where each recording carries the canonical identity fields plus `body`.
//! The file is runtime DATA at `.siralos/replay-store.json`; every byte is
//! untrusted, bounded to 2 MiB, and digest-verified on load. Writes are
//! atomic over the established lockfile pattern (temp file + rename) and are
//! refused whole-sale when any body matches a credential shape.

use std::path::Path;

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
    None
}

fn is_openai_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn contains_openai_key_shape(body: &str) -> bool {
    let bytes = body.as_bytes();
    let prefix = b"sk-";
    if bytes.len() < 3 + 16 {
        return false;
    }
    for i in 0..=bytes.len().saturating_sub(3) {
        if &bytes[i..i + 3] != prefix {
            continue;
        }
        let mut count = 0;
        for &b in &bytes[i + 3..] {
            let c = b as char;
            if is_openai_char(c) {
                count += 1;
            } else {
                break;
            }
        }
        if count >= 16 {
            return true;
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
        if &bytes[i..i + 4] != prefix {
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
        let token = &body[i..i + 20];
        if token == AWS_SAMPLE_KEY {
            continue;
        }
        return true;
    }
    false
}

fn is_bearer_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'
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

fn contains_credential_assignment_shape(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let orig = body.as_bytes();
    // keywords: api_key, api-key, apikey, secret, token, credential
    let keywords =
        ["api_key", "api-key", "apikey", "secret", "token", "credential"];
    // We scan lower for keyword occurrence but also need word boundary check.
    for i in 0..bytes.len() {
        // word boundary: start or previous not word char
        let is_boundary =
            if i == 0 { true } else { !is_word_char(orig[i - 1] as char) };
        if !is_boundary {
            continue;
        }
        let mut matched_len: Option<usize> = None;
        for kw in &keywords {
            if bytes.len() >= i + kw.len()
                && &bytes[i..i + kw.len()] == kw.as_bytes()
            {
                // For api variants, we have three; prefer longest match.
                // If multiple match at same i, pick longest.
                let len = kw.len();
                if matched_len.is_none_or(|prev| len > prev) {
                    matched_len = Some(len);
                }
            }
        }
        let kw_len = match matched_len {
            Some(v) => v,
            None => continue,
        };
        let mut pos = i + kw_len;
        // \s*=
        while pos < bytes.len() && (orig[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || orig[pos] != b'=' {
            continue;
        }
        pos += 1;
        while pos < bytes.len() && (orig[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || orig[pos] != b'"' {
            continue;
        }
        pos += 1; // after opening quote
        if pos >= bytes.len() {
            continue;
        }
        // negative lookahead: value must not start with "${" or "env:"
        if bytes.len() >= pos + 2 && &bytes[pos..pos + 2] == b"${" {
            continue;
        }
        if bytes.len() >= pos + 4 && &bytes[pos..pos + 4] == b"env:" {
            continue;
        }
        // find closing quote
        let mut end_opt: Option<usize> = None;
        for (j, &b) in orig.iter().enumerate().skip(pos) {
            if b == b'"' {
                end_opt = Some(j);
                break;
            }
        }
        let end = match end_opt {
            Some(v) => v,
            None => continue,
        };
        let value_len = end - pos;
        if value_len >= 8 {
            return true;
        }
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
    /// Bounded-cache violation.
    Bounds(ReplayStoreBoundsError),
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
            Self::Bounds(err) => write!(f, "replay store bounds: {err}"),
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

/// Write the bounded replay store atomically.
///
/// Scans every body first; any credential shape match refuses the whole
/// write with the offending index + pattern. Then deterministic
/// oldest-first eviction keeps the last `REPLAY_STORE_MAX_RECORDINGS`
/// that fit the 2 MiB total-bytes cap, evicting from the front. Then
/// atomic write (temp file + rename, same conventions as
/// `crates/siralos-adapters/src/lockfile.rs`) of the canonical JSON with
/// the recomputed digest. Returns the persisted count.
pub fn write_replay_store(
    path: &Path,
    recordings: &[ReplayRecording],
) -> Result<usize, ReplayStoreWriteError> {
    // 1. Sanitization-before-persist: scan every body.
    for (index, recording) in recordings.iter().enumerate() {
        if let Some(pattern) = body_credential_shape(&recording.body) {
            return Err(ReplayStoreWriteError::CredentialShape {
                index,
                pattern,
            });
        }
    }

    // 2. Deterministic oldest-first eviction.
    let mut kept: Vec<ReplayRecording> = recordings.to_vec();
    if kept.len() > REPLAY_STORE_MAX_RECORDINGS {
        let drain = kept.len() - REPLAY_STORE_MAX_RECORDINGS;
        kept.drain(0..drain);
    }
    let mut total: usize = kept.iter().map(|r| r.body.len()).sum();
    while total > REPLAY_STORE_MAX_TOTAL_BODY_BYTES && !kept.is_empty() {
        total -= kept[0].body.len();
        kept.remove(0);
    }
    // If even after eviction the total still exceeds (single huge body),
    // surface as bounds. Body size is already bounded at record time, so
    // this is a defensive typed error.
    if total > REPLAY_STORE_MAX_TOTAL_BODY_BYTES {
        return Err(ReplayStoreWriteError::Bounds(
            ReplayStoreBoundsError::TotalBodyBytesExceeded { total },
        ));
    }
    // Validate final kept set (defensive: also checks count).
    if let Err(err) = validate_replay_store_bounds(&kept) {
        return Err(ReplayStoreWriteError::Bounds(err));
    }

    // 3. Recompute digest.
    let digest = compute_replay_store_digest(&kept);

    // 4. Build canonical JSON.
    let recordings_json: Vec<Value> = kept
        .iter()
        .map(|recording| {
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
        })
        .collect();
    let document = json!({
        "version": REPLAY_STORE_VERSION,
        "digest": digest,
        "recordings": recordings_json,
    });
    let serialized = serde_json::to_string(&document)
        .map_err(|e| ReplayStoreWriteError::Io(e.to_string()))?;

    // 5. Atomic write: temp file + rename, same fs conventions as lockfile.
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // Ensure parent exists for hermetic temp-dir tests.
    if !parent.as_os_str().is_empty() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ReplayStoreWriteError::Io(e.to_string()))?;
    }
    let nonce = write_nonce();
    let temporary =
        parent.join(format!("{MUTATION_TEMP_PREFIX}replay-store-{nonce}"));
    std::fs::write(&temporary, &serialized).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        ReplayStoreWriteError::Io(format!("store could not be staged: {e}"))
    })?;

    // lstat-verified target check; symlink at target is replaced by rename, never followed.
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                let _ = std::fs::remove_file(&temporary);
                return Err(ReplayStoreWriteError::Io(
                    "store must be a regular file; refusing symlink or special file"
                        .to_owned(),
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(ReplayStoreWriteError::Io(format!(
                "store is unreadable: {error}"
            )));
        }
    }

    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(ReplayStoreWriteError::Io(format!(
            "store could not be replaced: {error}"
        )));
    }

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
        Err(error) => {
            return Err(ReplayStoreLoadError::Io(error.to_string()));
        }
    }

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
            BoundedFileRead::IoError(error) => {
                return Err(ReplayStoreLoadError::Io(error.to_string()));
            }
        };

    let text = String::from_utf8(bytes).map_err(|_| {
        ReplayStoreLoadError::Malformed("store is not valid UTF-8".to_owned())
    })?;

    let value: Value = serde_json::from_str(&text).map_err(|_| {
        ReplayStoreLoadError::Malformed("store is not valid JSON".to_owned())
    })?;

    let obj = value.as_object().ok_or_else(|| {
        ReplayStoreLoadError::Malformed("store must be an object".to_owned())
    })?;

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

    let mut recordings = Vec::with_capacity(arr.len());
    for entry in arr {
        let table = entry.as_object().ok_or_else(|| {
            ReplayStoreLoadError::Malformed(
                "each recording must be an object".to_owned(),
            )
        })?;
        let provider_id = table
            .get("providerId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires providerId".to_owned(),
                )
            })?
            .to_owned();
        let model = table
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires model".to_owned(),
                )
            })?
            .to_owned();
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
        let body_sha256 = table
            .get("bodySha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires bodySha256".to_owned(),
                )
            })?
            .to_owned();
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
        let body = table
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ReplayStoreLoadError::Malformed(
                    "recording requires body".to_owned(),
                )
            })?
            .to_owned();

        recordings.push(ReplayRecording {
            identity: siralos_core::determinism::ProviderResponseIdentity {
                provider_id,
                model,
                status,
                body_sha256,
                body_bytes,
                observed_at_ms,
            },
            body,
        });
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

fn write_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        body_credential_shape, load_replay_store, write_replay_store,
    };
    use siralos_core::determinism::{
        ProviderResponseIdentity, ReplayRecording,
    };

    fn recording_with_body(id: usize, body: &str) -> ReplayRecording {
        ReplayRecording {
            identity: ProviderResponseIdentity {
                provider_id: format!("p{id}"),
                model: format!("m{id}"),
                status: Some(200),
                body_sha256: format!("sha{id}"),
                body_bytes: body.len() as u64,
                observed_at_ms: Some(id as u64),
            },
            body: body.to_owned(),
        }
    }

    fn temp_store_path(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let base = std::env::temp_dir()
            .join(format!("siralos-replay-{name}-{nonce}"));
        std::fs::create_dir_all(&base).expect("temp root");
        base.join("replay-store.json")
    }

    #[test]
    fn round_trip_write_load_preserves_recordings_and_digest() {
        let path = temp_store_path("roundtrip");
        let recordings = vec![
            recording_with_body(0, "hello world"),
            recording_with_body(1, "{\"choices\":[]}"),
        ];
        let count = write_replay_store(&path, &recordings).expect("write");
        assert_eq!(count, 2);
        let loaded = load_replay_store(&path).expect("load");
        assert_eq!(loaded.recordings, recordings);
        // digest is internally verified; write then tamper would fail.
    }

    #[test]
    fn tamper_one_body_byte_is_untrusted() {
        let path = temp_store_path("tamper");
        let recordings = vec![recording_with_body(0, "hello tamper")];
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
        let body = format!("sample {}", super::AWS_SAMPLE_KEY);
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
        let rec1 = recording_with_body(0, r#"api_key = "env:MY_SECRET""#);
        let rec2 = recording_with_body(1, r#"secret = "${VAR}""#);
        let count = write_replay_store(&path, &[rec1, rec2]).expect("allowed");
        assert_eq!(count, 2);
    }

    #[test]
    fn eviction_65_recordings_oldest_dropped_deterministic() {
        let path = temp_store_path("evict-65");
        let recordings: Vec<_> =
            (0..65).map(|i| recording_with_body(i, "x")).collect();
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
    fn over_bytes_eviction() {
        let path = temp_store_path("over-bytes");
        // Create recordings each 768 KiB -> 3 total ~2.25 MiB > 2 MiB cap.
        // Keep last 2 (1.5 MiB) to stay under both body and file caps.
        let body = "a".repeat(768 * 1024);
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
        // Write a file larger than 2 MiB directly (bypass write path).
        let big = "x".repeat(2 * 1024 * 1024 + 1);
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
        let ok = recording_with_body(0, "hello");
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
