//! The workspace `siralos.lock` adapter (Stage 5.4, decision 50).
//!
//! Siralos verifies `siralos.lock` and never writes one on any production
//! path. Normal execution must not silently modify the lock (ADR 0036 §12), so
//! no product code calls `write_workspace_lock`. That writer is the prepared
//! implementation of `siralos profile lock` — the operation §12 freezes the
//! semantics of and deliberately leaves unimplemented — and it stages its bytes
//! with [`crate::atomic::stage_atomic`] and swaps them with
//! [`crate::atomic::StagedWrite::commit`], which owns the exclusive temporary
//! file, the regular-file check on the target, the symlink and special-target
//! refusals, and the cleanup on any failure, for when that operation lands.
//! Loading re-derives the lock digest from the parsed identities, so a
//! hand-edited or corrupt lock is typed invalid rather than trusted.

use std::path::Path;

use crate::workspace::fs::{
    BoundedFileRead, MUTATION_TEMP_PREFIX, read_complete_file_bounded,
};
use siralos_core::composition::lock::{
    LockPluginIdentity, WorkspaceLock, create_workspace_lock,
};

/// Maximum `siralos.lock` size in bytes.
pub const MAX_SIRALOS_LOCK_BYTES: usize = 64 * 1024;

/// Plugin-count bound mirrored from the core lock model so untrusted
/// arrays are rejected before their entries are cloned or traversed.
const MAX_LOCK_PLUGIN_ENTRIES: usize = 16;
/// Identity-field bounds mirrored from the core lock model so invalid
/// untrusted fields are refused before they are cloned.
const MAX_LOCK_ID_BYTES: usize = 64;
const MAX_LOCK_PATH_BYTES: usize = 256;
const LOCK_DIGEST_BYTES: usize = 64;

fn invalid_lock_identity() -> LockFailure {
    failure("siralos.lock contains invalid identity data".to_owned())
}

fn is_lock_hex64(value: &str) -> bool {
    value.len() == LOCK_DIGEST_BYTES
        && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn validate_lock_profile_digest(digest: &str) -> Result<(), LockFailure> {
    if is_lock_hex64(digest) { Ok(()) } else { Err(invalid_lock_identity()) }
}

fn validate_lock_identity_fields(
    id: &str,
    path: &str,
    digest: &str,
) -> Result<(), LockFailure> {
    let id_ok = !id.is_empty()
        && id.len() <= MAX_LOCK_ID_BYTES
        && !id.chars().any(char::is_control);
    let path_ok = !path.is_empty()
        && path.len() <= MAX_LOCK_PATH_BYTES
        && siralos_core::workspace::path::validate_relative_path(path).is_ok();
    if id_ok && path_ok && is_lock_hex64(digest) {
        Ok(())
    } else {
        Err(invalid_lock_identity())
    }
}

fn lock_file_name() -> &'static str {
    "siralos.lock"
}

/// A typed lock-file failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockFailure {
    /// Bounded truthful message.
    pub message: String,
}

fn failure(message: impl Into<String>) -> LockFailure {
    LockFailure { message: message.into() }
}

/// The verification outcome against the on-disk lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockVerification {
    /// No lock file exists.
    Missing,
    /// The stored lock digest matches the recomputed one.
    Current,
    /// The stored lock digest differs; expected/actual recorded.
    Stale {
        /// The recomputed lock digest.
        expected: String,
        /// The stored lock digest.
        actual: String,
    },
}

/// Load and re-derive the workspace lock. `Ok(None)` means no lock
/// file exists (a typed state, not an error).
///
/// # Errors
///
/// Returns [`LockFailure`] for unreadable/oversize/non-UTF-8 files,
/// syntax errors, unknown fields, digest mismatches, and identity
/// violations.
pub fn load_workspace_lock(
    root: &Path,
) -> Result<Option<WorkspaceLock>, LockFailure> {
    let path = root.join(lock_file_name());
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(_error) => {
            return Err(failure("siralos.lock is unreadable".to_owned()));
        }
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(failure(
                    "siralos.lock must be a regular file; refusing symlink or special file"
                        .to_owned(),
                ));
            }
        }
    }
    let bytes = match read_complete_file_bounded(&path, MAX_SIRALOS_LOCK_BYTES)
    {
        BoundedFileRead::Complete(bytes) => bytes,
        BoundedFileRead::TooLarge => {
            return Err(failure(format!(
                "siralos.lock exceeds the {MAX_SIRALOS_LOCK_BYTES}-byte bound."
            )));
        }
        BoundedFileRead::NotReadable => {
            return Err(failure(
                "siralos.lock must be a regular file; refusing symlink or special file"
                    .to_owned(),
            ));
        }
        BoundedFileRead::IoError(_error) => {
            return Err(failure("siralos.lock is unreadable".to_owned()));
        }
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| failure("siralos.lock is not valid UTF-8.".to_owned()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|_| failure("siralos.lock syntax is invalid".to_owned()))?;
    let root_table = value
        .as_table()
        .ok_or_else(|| failure("siralos.lock must be a table."))?;
    for key in root_table.keys() {
        if key != "lockDigest"
            && key != "plugins"
            && key != "profileDigest"
            && key != "schemaVersion"
        {
            return Err(failure(
                "siralos.lock contains an unknown field".to_owned(),
            ));
        }
    }
    if let Some(schema_version) = value.get("schemaVersion") {
        if schema_version.as_integer() != Some(1) {
            return Err(failure(
                "siralos.lock schemaVersion must be the integer 1.".to_owned(),
            ));
        }
    }
    let profile_digest = match value.get("profileDigest") {
        None => None,
        Some(toml::Value::String(digest)) => {
            validate_lock_profile_digest(digest)?;
            Some(digest.to_owned())
        }
        Some(_) => {
            return Err(failure(
                "The lock profileDigest must be a string.".to_owned(),
            ));
        }
    };
    let mut plugins = Vec::new();
    if let Some(entries) = value.get("plugins") {
        let Some(list) = entries.as_array() else {
            return Err(failure(
                "The lock plugins entry must be an array.".to_owned(),
            ));
        };
        if list.len() > MAX_LOCK_PLUGIN_ENTRIES {
            return Err(failure(format!(
                "siralos.lock exceeds the {MAX_LOCK_PLUGIN_ENTRIES}-plugin bound."
            )));
        }
        for entry in list {
            let Some(table) = entry.as_table() else {
                return Err(failure(
                    "Each lock plugin entry must be a table.".to_owned(),
                ));
            };
            for key in table.keys() {
                if !matches!(key.as_str(), "id" | "path" | "digest") {
                    return Err(failure(
                        "A lock plugin entry contains an unknown field."
                            .to_owned(),
                    ));
                }
            }
            let id =
                table.get("id").and_then(toml::Value::as_str).ok_or_else(
                    || failure("A lock plugin entry requires an id."),
                )?;
            let path =
                table.get("path").and_then(toml::Value::as_str).ok_or_else(
                    || failure("A lock plugin entry requires a path."),
                )?;
            let digest =
                table.get("digest").and_then(toml::Value::as_str).ok_or_else(
                    || failure("A lock plugin entry requires a digest."),
                )?;
            validate_lock_identity_fields(id, path, digest)?;
            plugins.push(LockPluginIdentity {
                id: id.to_owned(),
                path: path.to_owned(),
                digest: digest.to_owned(),
            });
        }
    }
    let recomputed = create_workspace_lock(
        profile_digest.as_deref(),
        &plugins,
    )
    .map_err(|_| {
        failure("siralos.lock contains invalid identity data".to_owned())
    })?;
    if let Some(stored) = value.get("lockDigest").and_then(toml::Value::as_str)
    {
        if stored != recomputed.lock_digest {
            return Err(failure(
                "The stored lockDigest does not match the recomputed identities; the lock is corrupt or hand-edited."
                    .to_owned(),
            ));
        }
    } else {
        return Err(failure(
            "The lock requires a lockDigest string.".to_owned(),
        ));
    }
    Ok(Some(recomputed))
}

/// Write the lock atomically: a unique temporary file in the lock's
/// directory, then a rename over the target. The target must be absent, or
/// a regular file that is not a symlink; a symlink or special target is
/// refused and the staged file is removed. The temporary file is cleaned up
/// on any failure.
///
/// # Errors
///
/// Returns [`LockFailure`] when serialization or any filesystem step
/// fails.
pub fn write_workspace_lock(
    root: &Path,
    lock: &WorkspaceLock,
) -> Result<(), LockFailure> {
    if lock.plugins.len() > MAX_LOCK_PLUGIN_ENTRIES {
        return Err(failure(format!(
            "siralos.lock exceeds the {MAX_LOCK_PLUGIN_ENTRIES}-plugin bound."
        )));
    }
    if let Some(profile_digest) = lock.profile_digest.as_deref() {
        validate_lock_profile_digest(profile_digest)?;
    }
    for identity in &lock.plugins {
        validate_lock_identity_fields(
            &identity.id,
            &identity.path,
            &identity.digest,
        )?;
    }
    let recomputed =
        create_workspace_lock(lock.profile_digest.as_deref(), &lock.plugins)
            .map_err(|_| {
            failure("siralos.lock contains invalid identity data".to_owned())
        })?;
    if lock.lock_digest != recomputed.lock_digest
        || lock.profile_digest != recomputed.profile_digest
    {
        return Err(failure(
            "siralos.lock identity data does not match its digest".to_owned(),
        ));
    }
    let lock = &recomputed;
    let mut document = toml::map::Map::new();
    document.insert(
        "lockDigest".to_owned(),
        toml::Value::String(lock.lock_digest.clone()),
    );
    if let Some(profile_digest) = &lock.profile_digest {
        document.insert(
            "profileDigest".to_owned(),
            toml::Value::String(profile_digest.clone()),
        );
    }
    if !lock.plugins.is_empty() {
        let entries = lock
            .plugins
            .iter()
            .map(|identity| {
                let mut table = toml::map::Map::new();
                table.insert(
                    "digest".to_owned(),
                    toml::Value::String(identity.digest.clone()),
                );
                table.insert(
                    "id".to_owned(),
                    toml::Value::String(identity.id.clone()),
                );
                table.insert(
                    "path".to_owned(),
                    toml::Value::String(identity.path.clone()),
                );
                toml::Value::Table(table)
            })
            .collect();
        document.insert("plugins".to_owned(), toml::Value::Array(entries));
    }
    let serialized =
        toml::to_string(&toml::Value::Table(document)).map_err(|_| {
            failure("siralos.lock could not be serialized".to_owned())
        })?;
    let staged = crate::atomic::stage_atomic(
        root,
        lock_file_name(),
        &format!("{MUTATION_TEMP_PREFIX}siralos-lock"),
        serialized.as_bytes(),
        None,
    )
    .map_err(|error| match error {
        crate::atomic::AtomicWriteFailure::TargetIsNotARegularFile { .. } => {
            failure(
                "siralos.lock must be a regular file; refusing symlink or special file"
                    .to_owned(),
            )
        }
        _ => failure("siralos.lock could not be staged".to_owned()),
    })?;
    staged.commit().map_err(|error| match error {
        crate::atomic::AtomicWriteFailure::TargetIsNotARegularFile { .. } => failure(
            "siralos.lock must be a regular file; refusing symlink or special file"
                .to_owned(),
        ),
        _ => failure("siralos.lock could not be replaced".to_owned()),
    })?;
    Ok(())
}

/// Verify the on-disk lock against a recomputed one.
///
/// # Errors
///
/// Returns [`LockFailure`] when the stored lock is unreadable, corrupt,
/// or violates identity bounds.
pub fn verify_workspace_lock(
    root: &Path,
    current: &WorkspaceLock,
) -> Result<LockVerification, LockFailure> {
    match load_workspace_lock(root)? {
        None => Ok(LockVerification::Missing),
        Some(stored) => {
            if stored.lock_digest == current.lock_digest {
                Ok(LockVerification::Current)
            } else {
                Ok(LockVerification::Stale {
                    expected: current.lock_digest.clone(),
                    actual: stored.lock_digest.clone(),
                })
            }
        }
    }
}

#[cfg(test)]
mod lockfile_tests {
    use super::load_workspace_lock;
    use super::{
        LockVerification, MAX_LOCK_PLUGIN_ENTRIES, verify_workspace_lock,
        write_workspace_lock,
    };
    use siralos_core::composition::lock::{
        LockPluginIdentity, WorkspaceLock, create_workspace_lock,
    };

    fn workspace() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("siralos-lock-tests-{nonce}"));
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn sample_lock() -> WorkspaceLock {
        create_workspace_lock(
            Some(&"a".repeat(64)),
            &[LockPluginIdentity {
                id: "guest".to_owned(),
                path: "guest.wasm".to_owned(),
                digest: "b".repeat(64),
            }],
        )
        .expect("lock")
    }

    #[test]
    fn missing_then_roundtrip_then_current() {
        let root = workspace();
        let lock = sample_lock();
        assert_eq!(load_workspace_lock(&root).expect("load"), None);
        assert_eq!(
            verify_workspace_lock(&root, &lock).expect("verify"),
            LockVerification::Missing,
        );
        write_workspace_lock(&root, &lock).expect("write");
        let loaded =
            load_workspace_lock(&root).expect("load").expect("a lock");
        assert_eq!(loaded, lock);
        assert_eq!(
            verify_workspace_lock(&root, &lock).expect("verify"),
            LockVerification::Current,
        );
    }

    #[test]
    fn regenerated_declarations_verify_stale() {
        let root = workspace();
        write_workspace_lock(&root, &sample_lock()).expect("write");
        let mutated =
            create_workspace_lock(Some(&"c".repeat(64)), &[]).expect("lock");
        let verification =
            verify_workspace_lock(&root, &mutated).expect("verify");
        match &verification {
            LockVerification::Stale { expected, actual } => {
                assert_eq!(expected, &mutated.lock_digest);
                assert_eq!(actual, &sample_lock().lock_digest);
            }
            other => panic!("unexpected verification: {other:?}"),
        }
    }

    #[test]
    fn lock_at_plugin_count_bound_roundtrips() {
        let root = workspace();
        let identities = (0..MAX_LOCK_PLUGIN_ENTRIES)
            .map(|index| LockPluginIdentity {
                id: format!("plugin{index}"),
                path: format!("plugins/plugin{index}.wasm"),
                digest: "a".repeat(64),
            })
            .collect::<Vec<_>>();
        let lock = create_workspace_lock(Some(&"b".repeat(64)), &identities)
            .expect("lock");

        write_workspace_lock(&root, &lock).expect("write");
        let loaded = load_workspace_lock(&root).expect("load").expect("lock");

        assert_eq!(loaded, lock);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn load_rejects_plugin_count_over_limit_before_iterating_entries() {
        let root = workspace();
        let mut entries = vec!["false".to_owned()];
        entries.extend((0..MAX_LOCK_PLUGIN_ENTRIES).map(|index| {
            format!(
                "{{ id = \"plugin{index}\", path = \"plugins/plugin{index}.wasm\", digest = \"{}\" }}",
                "a".repeat(64),
            )
        }));
        let text = format!(
            "lockDigest = \"{}\"\nplugins = [{}]\n",
            "0".repeat(64),
            entries.join(","),
        );
        std::fs::write(root.join("siralos.lock"), text).expect("write");

        let error = load_workspace_lock(&root).expect_err("refused");

        assert_eq!(error.message, "siralos.lock exceeds the 16-plugin bound.",);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn write_rejects_plugin_count_over_limit_before_iterating_entries() {
        let root = workspace();
        let lock = WorkspaceLock {
            profile_digest: None,
            plugins: (0..=MAX_LOCK_PLUGIN_ENTRIES)
                .map(|index| LockPluginIdentity {
                    id: format!("plugin{index}"),
                    path: format!("plugins/plugin{index}.wasm"),
                    digest: "a".repeat(64),
                })
                .collect(),
            lock_digest: "0".repeat(64),
        };

        let error = write_workspace_lock(&root, &lock).expect_err("refused");

        assert_eq!(error.message, "siralos.lock exceeds the 16-plugin bound.",);
        assert!(!root.join("siralos.lock").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn load_rejects_identity_fields_outside_bounds() {
        let root = workspace();
        let good_digest = "a".repeat(64);
        let cases = [
            format!(
                "lockDigest = \"{}\"\nprofileDigest = \"nothex\"\nplugins = []\n",
                "0".repeat(64),
            ),
            format!(
                "lockDigest = \"{}\"\nplugins = [{{ id = \"{}\", path = \"plugin.wasm\", digest = \"{good_digest}\" }}]\n",
                "0".repeat(64),
                "i".repeat(65),
            ),
            format!(
                "lockDigest = \"{}\"\nplugins = [{{ id = \"\\u0007\", path = \"plugin.wasm\", digest = \"{good_digest}\" }}]\n",
                "0".repeat(64),
            ),
            format!(
                "lockDigest = \"{}\"\nplugins = [{{ id = \"guest\", path = \"{}\", digest = \"{good_digest}\" }}]\n",
                "0".repeat(64),
                "p".repeat(257),
            ),
            format!(
                "lockDigest = \"{}\"\nplugins = [{{ id = \"guest\", path = \"../outside.wasm\", digest = \"{good_digest}\" }}]\n",
                "0".repeat(64),
            ),
            format!(
                "lockDigest = \"{}\"\nplugins = [{{ id = \"guest\", path = \"plugin.wasm\", digest = \"{}\" }}]\n",
                "0".repeat(64),
                "d".repeat(65),
            ),
        ];

        for text in cases {
            std::fs::write(root.join("siralos.lock"), text).expect("write");
            let error = load_workspace_lock(&root).expect_err("refused");
            assert_eq!(
                error.message,
                "siralos.lock contains invalid identity data",
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn write_rejects_identity_fields_outside_bounds() {
        let root = workspace();
        let good_digest = "a".repeat(64);
        let locks = [
            WorkspaceLock {
                profile_digest: Some("nothex".to_owned()),
                plugins: Vec::new(),
                lock_digest: "0".repeat(64),
            },
            WorkspaceLock {
                profile_digest: None,
                plugins: vec![LockPluginIdentity {
                    id: "i".repeat(65),
                    path: "plugin.wasm".to_owned(),
                    digest: good_digest.clone(),
                }],
                lock_digest: "0".repeat(64),
            },
            WorkspaceLock {
                profile_digest: None,
                plugins: vec![LockPluginIdentity {
                    id: "guest\u{0007}".to_owned(),
                    path: "plugin.wasm".to_owned(),
                    digest: good_digest.clone(),
                }],
                lock_digest: "0".repeat(64),
            },
            WorkspaceLock {
                profile_digest: None,
                plugins: vec![LockPluginIdentity {
                    id: "guest".to_owned(),
                    path: "p".repeat(257),
                    digest: good_digest.clone(),
                }],
                lock_digest: "0".repeat(64),
            },
            WorkspaceLock {
                profile_digest: None,
                plugins: vec![LockPluginIdentity {
                    id: "guest".to_owned(),
                    path: "../outside.wasm".to_owned(),
                    digest: good_digest.clone(),
                }],
                lock_digest: "0".repeat(64),
            },
            WorkspaceLock {
                profile_digest: None,
                plugins: vec![LockPluginIdentity {
                    id: "guest".to_owned(),
                    path: "plugin.wasm".to_owned(),
                    digest: "d".repeat(65),
                }],
                lock_digest: "0".repeat(64),
            },
        ];

        for lock in locks {
            let error =
                write_workspace_lock(&root, &lock).expect_err("refused");
            assert_eq!(
                error.message,
                "siralos.lock contains invalid identity data",
            );
            assert!(!root.join("siralos.lock").exists());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lock_diagnostics_do_not_echo_toml_id_or_path() {
        let root = workspace();
        let entry = |path: &str| {
            format!(
                "{{ id = \"privateplugin\", path = \"{path}\", digest = \"{}\" }}",
                "a".repeat(64),
            )
        };
        let text = format!(
            "lockDigest = \"{}\"\nplugins = [{},{}]\n",
            "0".repeat(64),
            entry("plugins/first.wasm"),
            entry("plugins/private/second.wasm"),
        );
        std::fs::write(root.join("siralos.lock"), text).expect("write");

        let diagnostic =
            load_workspace_lock(&root).expect_err("refused").message;

        assert!(!diagnostic.contains("privateplugin"));
        assert!(!diagnostic.contains("plugins/private/second.wasm"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lock_parse_diagnostic_does_not_echo_toml_line() {
        let root = workspace();
        std::fs::write(root.join("siralos.lock"), "private_marker = ???\n")
            .expect("write");

        let diagnostic =
            load_workspace_lock(&root).expect_err("refused").message;

        assert!(!diagnostic.contains("private_marker"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn write_io_diagnostic_does_not_echo_workspace_path() {
        let root = workspace();
        let missing_root = root.join("private-workspace");

        let error = write_workspace_lock(&missing_root, &sample_lock())
            .expect_err("refused");

        assert!(!error.message.contains("private-workspace"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hand_edited_lock_is_typed_invalid() {
        let root = workspace();
        write_workspace_lock(&root, &sample_lock()).expect("write");
        let path = root.join("siralos.lock");
        let text = std::fs::read_to_string(&path).expect("read");
        let tampered = text.replace(&"a".repeat(8), &"f".repeat(8));
        std::fs::write(&path, tampered).expect("rewrite");
        let error = load_workspace_lock(&root).expect_err("refused");
        assert!(error.message.contains("corrupt or hand-edited"));
    }
}
