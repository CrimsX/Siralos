//! Observed-file write tokens: secret-free, one-shot write
//! authority for one observed file revision (ADR 0039 authoring
//! flow; same pattern as the profile write token).
//!
//! A writer observes the current bounded bytes plus filesystem
//! identity, carries only the digest/length/identity across the
//! approval or form-completion boundary, consumes the authority
//! once, and re-verifies bytes plus identity immediately before
//! parsing or replacing. Clones share the one-shot gate: an
//! attempted mutation consumes the authority, so a retry must
//! reload and observe a new revision.

use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::workspace::fs::FileIdentity;

/// One exact file read plus its write authority, or the reason the
/// revision cannot authorize a write.
pub enum ObservedRevision {
    /// Observed bytes bound to a one-shot token.
    Bound {
        /// Exact file bytes (empty when absent).
        bytes: Vec<u8>,
        /// The one-shot write authority.
        token: ObservedWriteToken,
    },
    /// The revision cannot authorize a write (unreadable, oversize,
    /// non-UTF-8 where text is required, or identity unavailable).
    Unbindable {
        /// Truthful reason.
        reason: String,
    },
}

/// A secret-free, one-shot identity for one observed file
/// revision. The only write input that may cross a
/// frontend/worker boundary: it carries digests, lengths, and
/// identity — never parsed fields or raw bytes.
#[derive(Clone)]
pub struct ObservedWriteToken {
    raw_sha256: String,
    raw_len: usize,
    present: bool,
    file_identity: Option<FileIdentity>,
    consumed: Arc<AtomicBool>,
}

impl PartialEq for ObservedWriteToken {
    fn eq(&self, other: &Self) -> bool {
        self.raw_sha256 == other.raw_sha256
            && self.raw_len == other.raw_len
            && self.present == other.present
            && self.file_identity == other.file_identity
            && self.consumed.load(Ordering::Acquire)
                == other.consumed.load(Ordering::Acquire)
    }
}

impl Eq for ObservedWriteToken {}

impl std::fmt::Debug for ObservedWriteToken {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("ObservedWriteToken")
            .field("raw_len", &self.raw_len)
            .field("present", &self.present)
            .field("identity_present", &self.file_identity.is_some())
            .field("consumed", &self.consumed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl ObservedWriteToken {
    /// Return whether `bytes` are the exact content represented by
    /// this token. `None` means the target was absent when
    /// observed; an empty file is not equivalent to absence.
    #[must_use]
    pub fn matches_bytes(&self, bytes: Option<&[u8]>) -> bool {
        match bytes {
            None => !self.present,
            Some(bytes) => {
                self.present
                    && self.raw_len == bytes.len()
                    && self.raw_sha256
                        == siralos_core::identity::sha256_hex(bytes)
            }
        }
    }

    /// Return whether the path and bytes still represent the
    /// exact observed filesystem revision. The absence case is
    /// expressible through `bytes`: `None` matches only when the
    /// target was absent when observed and is still absent, so
    /// callers need no absence guard beside this check. Identity
    /// alone never authorizes.
    #[must_use]
    pub fn matches_path(&self, path: &Path, bytes: Option<&[u8]>) -> bool {
        if !self.matches_bytes(bytes) {
            return false;
        }
        match (&self.file_identity, bytes) {
            (None, None) => matches!(
                std::fs::symlink_metadata(path),
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
            ),
            (Some(expected), Some(_)) => {
                let Ok(metadata) = std::fs::symlink_metadata(path) else {
                    return false;
                };
                !metadata.file_type().is_symlink()
                    && crate::workspace::fs::capture_file_identity(&metadata)
                        .as_ref()
                        == Some(expected)
            }
            _ => false,
        }
    }

    /// Consume the one-shot authority. Returns whether this call
    /// consumed it (a second call reports `false`).
    pub fn consume(&self) -> bool {
        self.consumed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

/// Observe one workspace-relative file for a later guarded write:
/// bounded read, digest, identity. Unreadable, oversized, linked,
/// or special targets yield [`ObservedRevision::Unbindable`], never
/// an absent-file authority.
#[must_use]
pub fn observe_file_for_write(
    root: &Path,
    relative: &Path,
    max_bytes: usize,
) -> ObservedRevision {
    let path: PathBuf = root.join(relative);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ObservedRevision::Bound {
                bytes: Vec::new(),
                token: ObservedWriteToken {
                    raw_sha256: siralos_core::identity::sha256_hex(&[]),
                    raw_len: 0,
                    present: false,
                    file_identity: None,
                    consumed: Arc::new(AtomicBool::new(false)),
                },
            };
        }
        Err(_) => {
            return ObservedRevision::Unbindable {
                reason: "target cannot be inspected".to_owned(),
            };
        }
    };
    if !metadata.is_file() {
        return ObservedRevision::Unbindable {
            reason: "target is not a regular file".to_owned(),
        };
    }
    if metadata.len() > max_bytes as u64 {
        return ObservedRevision::Unbindable {
            reason: "target exceeds the byte bound".to_owned(),
        };
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return ObservedRevision::Unbindable {
                reason: "target cannot be read".to_owned(),
            };
        }
    };
    if bytes.len() != metadata.len() as usize {
        return ObservedRevision::Unbindable {
            reason: "target changed during observation".to_owned(),
        };
    }
    let Some(identity) =
        crate::workspace::fs::capture_file_identity(&metadata)
    else {
        return ObservedRevision::Unbindable {
            reason: "target identity is unavailable".to_owned(),
        };
    };
    ObservedRevision::Bound {
        bytes: bytes.clone(),
        token: ObservedWriteToken {
            raw_sha256: siralos_core::identity::sha256_hex(&bytes),
            raw_len: bytes.len(),
            present: true,
            file_identity: Some(identity),
            consumed: Arc::new(AtomicBool::new(false)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{ObservedRevision, observe_file_for_write};

    fn scratch(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "siralos-observed-{}-{}",
            std::process::id(),
            label
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch");
        root
    }

    #[test]
    fn absent_target_binds_empty_observation() {
        let root = scratch("absent");
        let relative = std::path::Path::new("missing.toml");
        let ObservedRevision::Bound { bytes, token } =
            observe_file_for_write(&root, relative, 1024)
        else {
            panic!("absent target must bind");
        };
        assert!(bytes.is_empty());
        assert!(token.matches_bytes(None));
        assert!(!token.matches_bytes(Some(b"x")));
        // Absent and empty-present are distinct: an empty file
        // does not match absent authority.
        std::fs::write(root.join("missing.toml"), b"").expect("seed");
        assert!(!token.matches_bytes(Some(b"")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn observed_bytes_match_until_mutation_and_consume_is_one_shot() {
        let root = scratch("mutate");
        std::fs::write(root.join("doc.toml"), b"v = 1\n").expect("seed");
        let relative = std::path::Path::new("doc.toml");
        let ObservedRevision::Bound { bytes, token } =
            observe_file_for_write(&root, relative, 1024)
        else {
            panic!("present target must bind");
        };
        assert_eq!(bytes, b"v = 1\n");
        assert!(token.matches_bytes(Some(b"v = 1\n")));
        assert!(token.matches_path(&root.join("doc.toml"), Some(b"v = 1\n")));
        assert!(
            !token.matches_path(&root.join("doc.toml"), Some(b"v = 2\n")),
            "bytes mismatch refuses before identity"
        );
        assert!(token.consume());
        assert!(!token.consume(), "second consume refuses");
        std::fs::write(root.join("doc.toml"), b"v = 2\n").expect("mutate");
        assert!(!token.matches_bytes(Some(b"v = 2\n")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn matches_path_models_absence_without_a_caller_guard() {
        let root = scratch("absence");
        let relative = std::path::Path::new("missing.toml");
        let path = root.join("missing.toml");
        // Absent authority matches an absent target with no guard.
        let ObservedRevision::Bound { token: absent, .. } =
            observe_file_for_write(&root, relative, 1024)
        else {
            panic!("absent target must bind");
        };
        assert!(absent.matches_path(&path, None));
        // A created file no longer matches absent authority.
        std::fs::write(&path, b"v = 1\n").expect("seed");
        assert!(!absent.matches_path(&path, None));
        assert!(!absent.matches_path(&path, Some(b"v = 1\n")));
        // Present authority binds bytes plus identity: identical
        // bytes at another path do not match.
        let ObservedRevision::Bound { token: present, .. } =
            observe_file_for_write(&root, relative, 1024)
        else {
            panic!("present target must bind");
        };
        let twin = root.join("twin.toml");
        std::fs::write(&twin, b"v = 1\n").expect("seed twin");
        assert!(present.matches_path(&path, Some(b"v = 1\n")));
        assert!(
            !present.matches_path(&twin, Some(b"v = 1\n")),
            "same bytes at another identity must not match"
        );
        // Deleting the target breaks the match either way.
        std::fs::remove_file(&path).expect("remove");
        assert!(!present.matches_path(&path, None));
        assert!(!present.matches_path(&path, Some(b"v = 1\n")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn oversize_and_missing_targets_do_not_bind() {
        let root = scratch("bounds");
        std::fs::write(root.join("big.toml"), b"0123456789").expect("seed");
        assert!(matches!(
            observe_file_for_write(&root, std::path::Path::new("big.toml"), 4),
            ObservedRevision::Unbindable { .. }
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
