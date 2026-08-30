//! The workspace `siralos.lock` adapter (Stage 5.4, decision 50).
//!
//! Owns load/write/verify over the machine-generated lock artifact. The
//! write follows the one established atomic pattern (decision 38/39
//! plugin records): a unique temporary file, an lstat-verified
//! regular-file check on the target (symlinks are replaced by the
//! rename, never followed), and cleanup on any failure. Loading
//! re-derives the lock digest from the parsed identities, so a hand-
//! edited or corrupt lock is typed invalid rather than trusted.

use std::path::Path;

use crate::workspace::fs::{
    BoundedFileRead, MUTATION_TEMP_PREFIX, read_complete_file_bounded,
};
use siralos_core::composition::lock::{
    LockPluginIdentity, WorkspaceLock, create_workspace_lock,
};

/// Maximum `siralos.lock` size in bytes.
pub const MAX_SIRALOS_LOCK_BYTES: usize = 64 * 1024;

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
        Err(error) => {
            return Err(failure(format!(
                "siralos.lock is unreadable: {error}"
            )));
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
        BoundedFileRead::IoError(error) => {
            return Err(failure(format!(
                "siralos.lock is unreadable: {error}"
            )));
        }
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| failure("siralos.lock is not valid UTF-8.".to_owned()))?;
    let value: toml::Value =
        toml::from_str(&text).map_err(|error| failure(error.message()))?;
    let root_table = value
        .as_table()
        .ok_or_else(|| failure("siralos.lock must be a table."))?;
    for key in root_table.keys() {
        if key != "lockDigest"
            && key != "plugins"
            && key != "profileDigest"
            && key != "schemaVersion"
        {
            return Err(failure(format!("Unknown lock field {key:?}.")));
        }
    }
    let profile_digest = match value.get("profileDigest") {
        None => None,
        Some(toml::Value::String(digest)) => Some(digest.clone()),
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
        for entry in list {
            let Some(table) = entry.as_table() else {
                return Err(failure(
                    "Each lock plugin entry must be a table.".to_owned(),
                ));
            };
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
            plugins.push(LockPluginIdentity {
                id: id.to_owned(),
                path: path.to_owned(),
                digest: digest.to_owned(),
            });
        }
    }
    let recomputed =
        create_workspace_lock(profile_digest.as_deref(), &plugins)
            .map_err(|error| failure(error.message))?;
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

/// Write the lock atomically: unique temporary file, lstat-verified
/// regular-file target check, rename over the target (a symlink target
/// is replaced, never followed), temp-file cleanup on any failure.
///
/// # Errors
///
/// Returns [`LockFailure`] when serialization or any filesystem step
/// fails.
pub fn write_workspace_lock(
    root: &Path,
    lock: &WorkspaceLock,
) -> Result<(), LockFailure> {
    let path = root.join(lock_file_name());
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
    let serialized = toml::to_string(&toml::Value::Table(document))
        .map_err(|error| failure(error.to_string()))?;
    let nonce = write_nonce();
    let temporary =
        root.join(format!("{MUTATION_TEMP_PREFIX}siralos-lock-{nonce}"));
    std::fs::write(&temporary, &serialized).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        failure(format!("siralos.lock could not be staged: {error}"))
    })?;
    let target_metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                let _ = std::fs::remove_file(&temporary);
                return Err(failure(
                    "siralos.lock must be a regular file; refusing symlink or special file"
                        .to_owned(),
                ));
            }
            Some(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(failure(format!(
                "siralos.lock is unreadable: {error}"
            )));
        }
    };
    let rename_result = std::fs::rename(&temporary, &path);
    if let Err(error) = rename_result {
        let _ = std::fs::remove_file(&temporary);
        return Err(failure(format!(
            "siralos.lock could not be replaced: {error}"
        )));
    }
    let _ = target_metadata;
    Ok(())
}

fn write_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
        .to_string()
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
        LockVerification, verify_workspace_lock, write_workspace_lock,
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
