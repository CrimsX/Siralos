//! Atomic staged replacement of single-owner host files (W4.3).
//!
//! Six production writers had grown the same algorithm by copy: stage the bytes in
//! a temporary file beside the target, check that the target is a regular file,
//! rename over it, and clean the temporary up on every failure. This module is
//! that algorithm once.
//!
//! **What the mechanism guarantees.** The temporary pathname is initially claimed
//! in the target's own directory with `create_new` (O_EXCL), so it is on the same
//! filesystem and cannot start as a symlink planted at the nonce path. The bytes are
//! flushed with `sync_all` before the swap, so a crash cannot leave a truncated
//! replacement of a file whose digest gates identity. The swap itself is
//! `std::fs::rename`, which replaces the directory entry atomically.
//!
//! **What it does not guarantee.** The created file handle is retained and its
//! identity evidence is checked against the staged pathname immediately before
//! the swap; an observed mismatch is refused and the substituted object is left
//! alone by cleanup. `std::fs::rename` still takes a pathname, however, so a
//! same-user process can substitute after that check and before the rename. The
//! cleanup check has the same residual pathname race. On non-Unix targets the
//! std metadata snapshot is length plus modification time rather than a native
//! file index, so it is useful defense in depth but not a cryptographic identity
//! proof. The target preconditions are likewise pathname-based. This is defense
//! in depth for these single-owner host files, not a handle-relative identity-bound
//! commit primitive.
//!
//! **Permissions.** On Unix a staged file is created with mode `0600` unless the
//! caller supplies another mode. A replacement therefore adopts the staged file's
//! permissions rather than the replaced file's.
//!
//! **Platform.** Directory-entry replacement is atomic on the filesystems this
//! product targets. On Windows, replacement can fail when an open handle does not
//! permit delete/rename sharing; that surfaces as a typed replacement failure
//! rather than a silent write.

use std::fs::{File, OpenOptions, remove_file, rename, symlink_metadata};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic component of a staged file name, so two stages in the same
/// nanosecond still differ.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Why an atomic replacement failed.
///
/// Every variant names the path it failed on, so a caller can map it into its own
/// error type without guessing which step failed.
pub enum AtomicWriteFailure {
    /// The staged bytes could not be written, a staged name could not be claimed
    /// after one retry, or identity evidence could not be captured.
    Staged {
        /// The staged path that could not be created or written.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The staged pathname no longer matches the retained identity evidence for
    /// the file created by [`stage_atomic`], so replacement was refused before
    /// rename.
    StagedIdentityUnverifiable {
        /// The staged path whose retained identity evidence did not match.
        path: PathBuf,
    },
    /// The target exists and is a symlink or not a regular file.
    TargetIsNotARegularFile {
        /// The refused target.
        path: PathBuf,
    },
    /// The target changed after the caller observed its digest.
    TargetChanged {
        /// The target whose identity no longer matched.
        path: PathBuf,
    },
    /// The target's metadata could not be read.
    TargetUnreadable {
        /// The unreadable target.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The rename over the target failed.
    ReplaceFailed {
        /// The target that could not be replaced.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
}

/// Redacted diagnostics retain the failure phase and OS error kind, but never
/// format the path or the I/O error message because either can contain a private
/// absolute path.
impl std::fmt::Debug for AtomicWriteFailure {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Staged { source, .. } => formatter
                .debug_struct("Staged")
                .field("source_kind", &source.kind())
                .field("raw_os_error", &source.raw_os_error())
                .finish(),
            Self::StagedIdentityUnverifiable { .. } => {
                formatter.write_str("StagedIdentityUnverifiable")
            }
            Self::TargetIsNotARegularFile { .. } => {
                formatter.write_str("TargetIsNotARegularFile")
            }
            Self::TargetChanged { .. } => formatter.write_str("TargetChanged"),
            Self::TargetUnreadable { source, .. } => formatter
                .debug_struct("TargetUnreadable")
                .field("source_kind", &source.kind())
                .field("raw_os_error", &source.raw_os_error())
                .finish(),
            Self::ReplaceFailed { source, .. } => formatter
                .debug_struct("ReplaceFailed")
                .field("source_kind", &source.kind())
                .field("raw_os_error", &source.raw_os_error())
                .finish(),
        }
    }
}

impl std::fmt::Display for AtomicWriteFailure {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Staged { .. } => {
                write!(formatter, "atomic write could not be staged")
            }
            Self::StagedIdentityUnverifiable { .. } => write!(
                formatter,
                "atomic staged file identity could not be verified"
            ),
            Self::TargetIsNotARegularFile { .. } => write!(
                formatter,
                "atomic target must be a regular file; refusing symlink or special file"
            ),
            Self::TargetChanged { .. } => {
                write!(
                    formatter,
                    "atomic target changed while it was being replaced"
                )
            }
            Self::TargetUnreadable { .. } => {
                write!(formatter, "atomic target is unreadable")
            }
            Self::ReplaceFailed { .. } => {
                write!(formatter, "atomic target could not be replaced")
            }
        }
    }
}

impl std::error::Error for AtomicWriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Staged { source, .. }
            | Self::TargetUnreadable { source, .. }
            | Self::ReplaceFailed { source, .. } => Some(source),
            Self::StagedIdentityUnverifiable { .. } => None,
            Self::TargetChanged { .. } => None,
            Self::TargetIsNotARegularFile { .. } => None,
        }
    }
}

/// Filesystem identity evidence retained for the file created by `stage_atomic`.
///
/// Unix has a stable device/inode pair. Other targets use the strongest
/// portable regular-file metadata fingerprint exposed by std; that fallback
/// detects ordinary substitutions but is not a cryptographic object identity.
#[derive(Clone, Eq, PartialEq)]
struct StagedFileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    length: u64,
    #[cfg(not(unix))]
    modified: std::time::SystemTime,
}

/// One staged replacement: the bytes are on disk, the target is untouched.
///
/// Dropping the value attempts conditional cleanup only while an observed check
/// shows that the path still identifies the created object. A substitution that
/// races that check remains a pathname-race limitation.
pub struct StagedWrite {
    temporary: PathBuf,
    target: PathBuf,
    staged_file: Option<File>,
    staged_identity: StagedFileIdentity,
    staged_sha256: String,
    committed: bool,
}

/// Redacted diagnostics expose lifecycle state without either private path.
impl std::fmt::Debug for StagedWrite {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("StagedWrite")
            .field("committed", &self.committed)
            .finish()
    }
}

impl StagedWrite {
    /// The staged file, for a caller that verifies the bytes before committing.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.temporary
    }

    /// Replace the target with the staged bytes.
    ///
    /// The target must be absent, or a regular file that is not a symlink; a
    /// symlink or special file is refused and the staged file is removed. The staged
    /// pathname is also checked against the retained file identity evidence before
    /// rename.
    /// Because the standard-library rename is still pathname-based, this check is
    /// not a complete defense against a same-user process that races the final
    /// rename. On non-Unix targets the retained check uses the portable
    /// length/modified-time snapshot because std exposes no stable file index.
    ///
    /// # Errors
    /// Returns an AtomicWriteFailure when the staged identity cannot be verified,
    /// the target is unreadable or is not a regular file, or the replacement fails.
    pub fn commit(self) -> Result<(), AtomicWriteFailure> {
        self.commit_inner(None, false)
    }

    /// Commit only when the target is still absent. This advisory pathname
    /// check narrows the ordinary first-writer window; it is not a kernel
    /// identity-bound create and does not close the final pathname race.
    pub fn commit_if_absent(self) -> Result<(), AtomicWriteFailure> {
        self.commit_inner(None, true)
    }

    /// Commit only when the target still has the caller's observed SHA-256.
    /// This advisory compare-and-swap narrows the ordinary concurrent-writer
    /// window; the final pathname rename is not identity-bound.
    pub fn commit_if_digest(
        self,
        expected_sha256: &str,
    ) -> Result<(), AtomicWriteFailure> {
        self.commit_inner(Some(expected_sha256), false)
    }

    fn commit_inner(
        mut self,
        expected_sha256: Option<&str>,
        require_absent: bool,
    ) -> Result<(), AtomicWriteFailure> {
        match symlink_metadata(&self.target) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AtomicWriteFailure::TargetIsNotARegularFile {
                        path: self.target.clone(),
                    });
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(source) => {
                return Err(AtomicWriteFailure::TargetUnreadable {
                    path: self.target.clone(),
                    source,
                });
            }
        }
        if require_absent {
            match symlink_metadata(&self.target) {
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(AtomicWriteFailure::TargetUnreadable {
                        path: self.target.clone(),
                        source,
                    });
                }
                Ok(_) => {
                    return Err(AtomicWriteFailure::TargetChanged {
                        path: self.target.clone(),
                    });
                }
            }
        }
        if let Some(expected) = expected_sha256 {
            use crate::workspace::fs::{
                BoundedFileRead, read_complete_file_bounded,
            };
            const MAX_CAS_READ_BYTES: usize = 16 * 1024 * 1024;
            let current = match read_complete_file_bounded(
                &self.target,
                MAX_CAS_READ_BYTES,
            ) {
                BoundedFileRead::Complete(bytes) => bytes,
                BoundedFileRead::TooLarge | BoundedFileRead::NotReadable => {
                    return Err(AtomicWriteFailure::TargetChanged {
                        path: self.target.clone(),
                    });
                }
                BoundedFileRead::IoError(source) => {
                    return Err(AtomicWriteFailure::TargetUnreadable {
                        path: self.target.clone(),
                        source,
                    });
                }
            };
            if siralos_core::identity::sha256_hex(&current) != expected {
                return Err(AtomicWriteFailure::TargetChanged {
                    path: self.target.clone(),
                });
            }
        }
        match self.staged_path_matches_identity() {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                return Err(AtomicWriteFailure::StagedIdentityUnverifiable {
                    path: self.temporary.clone(),
                });
            }
        }
        if !self.staged_content_matches_digest() {
            // Identity evidence alone is not content proof: on targets without
            // a stable file index the snapshot is length plus modified time,
            // which a same-user process can reproduce. The commit refuses
            // unless the staged bytes are still exactly what was staged.
            return Err(AtomicWriteFailure::StagedIdentityUnverifiable {
                path: self.temporary.clone(),
            });
        }
        rename(&self.temporary, &self.target).map_err(|source| {
            AtomicWriteFailure::ReplaceFailed {
                path: self.target.clone(),
                source,
            }
        })?;
        self.committed = true;
        Ok(())
    }

    /// Re-read the staged pathname and require the caller's exact bytes.
    fn staged_content_matches_digest(&self) -> bool {
        use crate::workspace::fs::{
            BoundedFileRead, read_complete_file_bounded,
        };
        const MAX_STAGED_REVALIDATION_BYTES: usize = 16 * 1024 * 1024;
        match read_complete_file_bounded(
            &self.temporary,
            MAX_STAGED_REVALIDATION_BYTES,
        ) {
            BoundedFileRead::Complete(bytes) => {
                siralos_core::identity::sha256_hex(&bytes)
                    == self.staged_sha256
            }
            // An unreadable or oversized staged file cannot be proven to be the
            // caller's bytes, so the commit fails closed.
            BoundedFileRead::TooLarge
            | BoundedFileRead::NotReadable
            | BoundedFileRead::IoError(_) => false,
        }
    }

    /// Check the temporary path against the retained handle and path evidence.
    fn staged_path_matches_identity(&self) -> std::io::Result<bool> {
        let staged_file = self.staged_file.as_ref().ok_or_else(|| {
            std::io::Error::other("staged file handle is unavailable")
        })?;
        let handle_identity = staged_file_identity(&staged_file.metadata()?)?;
        if handle_identity != self.staged_identity {
            return Ok(false);
        }
        let path_metadata = symlink_metadata(&self.temporary)?;
        if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
            return Ok(false);
        }
        Ok(staged_file_identity(&path_metadata)? == self.staged_identity)
    }
}

impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.committed
            && self.staged_path_matches_identity().unwrap_or(false)
        {
            self.staged_file.take();
            remove_staged_if_matching(&self.temporary, &self.staged_identity);
        }
    }
}

/// Stage the bytes beside the target for an atomic replacement.
///
/// The staged name is the stem plus a nonce, in the target's own directory. A name
/// that already exists is retried once with a fresh nonce and then refused, so two
/// concurrent stages cannot share a path.
///
/// The mode, when given, sets the staged file's Unix permission bits and is
/// ignored on platforms without them.
///
/// # Errors
/// Returns an AtomicWriteFailure when no staged path could be created, the bytes
/// could not be written, or staged-file identity evidence is unavailable.
pub fn stage_atomic(
    directory: &Path,
    file_name: &str,
    temp_stem: &str,
    contents: &[u8],
    mode: Option<u32>,
) -> Result<StagedWrite, AtomicWriteFailure> {
    let target = directory.join(file_name);
    let mut last: Option<(PathBuf, std::io::Error)> = None;
    for _ in 0..2 {
        let temporary =
            directory.join(format!("{temp_stem}-{}", staged_nonce()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode.unwrap_or(0o600));
        }
        match options.open(&temporary) {
            Ok(mut file) => {
                let initial_identity = match file
                    .metadata()
                    .and_then(|metadata| staged_file_identity(&metadata))
                {
                    Ok(identity) => identity,
                    Err(source) => {
                        drop(file);
                        return Err(AtomicWriteFailure::Staged {
                            path: temporary,
                            source,
                        });
                    }
                };
                if let Err(source) =
                    file.write_all(contents).and_then(|()| file.sync_all())
                {
                    let cleanup_identity = file
                        .metadata()
                        .and_then(|metadata| staged_file_identity(&metadata))
                        .unwrap_or(initial_identity);
                    drop(file);
                    remove_staged_if_matching(&temporary, &cleanup_identity);
                    return Err(AtomicWriteFailure::Staged {
                        path: temporary,
                        source,
                    });
                }
                if let Err(source) = apply_mode(&file, mode) {
                    let cleanup_identity = file
                        .metadata()
                        .and_then(|metadata| staged_file_identity(&metadata))
                        .unwrap_or(initial_identity);
                    drop(file);
                    remove_staged_if_matching(&temporary, &cleanup_identity);
                    return Err(AtomicWriteFailure::Staged {
                        path: temporary,
                        source,
                    });
                }
                let staged_identity = match file
                    .metadata()
                    .and_then(|metadata| staged_file_identity(&metadata))
                {
                    Ok(identity) => identity,
                    Err(source) => {
                        drop(file);
                        remove_staged_if_matching(
                            &temporary,
                            &initial_identity,
                        );
                        return Err(AtomicWriteFailure::Staged {
                            path: temporary,
                            source,
                        });
                    }
                };
                return Ok(StagedWrite {
                    temporary,
                    target,
                    staged_file: Some(file),
                    staged_identity,
                    staged_sha256: siralos_core::identity::sha256_hex(
                        contents,
                    ),
                    committed: false,
                });
            }
            Err(source) if source.kind() == ErrorKind::AlreadyExists => {
                last = Some((temporary, source));
            }
            Err(source) => {
                return Err(AtomicWriteFailure::Staged {
                    path: temporary,
                    source,
                });
            }
        }
    }
    let (path, source) = last.expect("a retry records its failure");
    Err(AtomicWriteFailure::Staged { path, source })
}

/// Apply the caller's Unix mode through the already-open staged-file handle.
#[cfg(unix)]
fn apply_mode(file: &File, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}

/// Platforms without Unix permission bits ignore the requested mode. Their
/// ACL semantics remain an explicit platform limitation rather than being
/// represented as owner-only proof.
#[cfg(not(unix))]
fn apply_mode(_file: &File, _mode: Option<u32>) -> std::io::Result<()> {
    Ok(())
}

/// Read a stable object identity from metadata without following a link.
#[cfg(unix)]
fn staged_file_identity(
    metadata: &std::fs::Metadata,
) -> std::io::Result<StagedFileIdentity> {
    use std::os::unix::fs::MetadataExt;
    Ok(StagedFileIdentity { device: metadata.dev(), inode: metadata.ino() })
}

/// Use the strongest portable regular-file metadata snapshot where std exposes
/// no stable device/inode pair. This mirrors the adapter's existing bounded-read
/// identity boundary; it is intentionally documented as weaker than Unix
/// device/inode identity.
#[cfg(not(unix))]
fn staged_file_identity(
    metadata: &std::fs::Metadata,
) -> std::io::Result<StagedFileIdentity> {
    Ok(StagedFileIdentity {
        length: metadata.len(),
        modified: metadata.modified()?,
    })
}

/// Remove a staged pathname only while an observed fingerprint matches the
/// retained evidence. The check and pathname removal remain susceptible to the
/// same residual substitution race.
fn remove_staged_if_matching(path: &Path, identity: &StagedFileIdentity) {
    let Ok(metadata) = symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || staged_file_identity(&metadata).ok().as_ref() != Some(identity)
    {
        return;
    }
    let _ = remove_file(path);
}

/// A staged name that differs across processes and threads.
///
/// The name is a clock reading plus a process-local counter, so it is guessable.
/// That is not a weakness here: the staged path is created exclusively, and a
/// name someone else already owns is refused rather than reused.
fn staged_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{sequence:x}")
}

#[cfg(test)]
mod tests {
    use super::{AtomicWriteFailure, stage_atomic};
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    /// A scratch directory that removes itself when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join("siralos-atomic")
                .join(format!("{label}-{}", super::staged_nonce()));
            fs::create_dir_all(&path).expect("scratch dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Regular entries in the directory, sorted and named only.
        fn entries(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .expect("read scratch")
                .map(|entry| {
                    entry
                        .expect("dir entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn debug_output_redacts_private_paths_and_preserves_failure_phase() {
        #[cfg(windows)]
        let sentinel = r"C:\siralos-private\sentinel\atomic.tmp".to_owned();
        #[cfg(not(windows))]
        let sentinel = "/siralos-private/sentinel/atomic.tmp".to_owned();
        let staged = AtomicWriteFailure::Staged {
            path: PathBuf::from(&sentinel),
            source: io::Error::other(sentinel.clone()),
        };
        assert_eq!(
            format!("{staged:?}"),
            "Staged { source_kind: Other, raw_os_error: None }"
        );
        assert!(!format!("{staged:?}").contains(&sentinel));
        assert!(!staged.to_string().contains(&sentinel));

        let cases = [
            (
                AtomicWriteFailure::StagedIdentityUnverifiable {
                    path: PathBuf::from(&sentinel),
                },
                "StagedIdentityUnverifiable",
            ),
            (
                AtomicWriteFailure::TargetIsNotARegularFile {
                    path: PathBuf::from(&sentinel),
                },
                "TargetIsNotARegularFile",
            ),
            (
                AtomicWriteFailure::TargetChanged {
                    path: PathBuf::from(&sentinel),
                },
                "TargetChanged",
            ),
            (
                AtomicWriteFailure::TargetUnreadable {
                    path: PathBuf::from(&sentinel),
                    source: io::Error::other(sentinel.clone()),
                },
                "TargetUnreadable",
            ),
            (
                AtomicWriteFailure::ReplaceFailed {
                    path: PathBuf::from(&sentinel),
                    source: io::Error::other(sentinel.clone()),
                },
                "ReplaceFailed",
            ),
        ];
        for (error, phase) in cases {
            let debug = format!("{error:?}");
            assert!(debug.contains(phase), "missing phase in {debug}");
            assert!(!debug.contains(&sentinel), "path leaked in {debug}");
            assert!(!error.to_string().contains(&sentinel));
        }
    }

    #[test]
    fn staged_write_debug_omits_private_paths() {
        let scratch = Scratch::new("debug");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"x", None)
                .expect("stage");
        let debug = format!("{staged:?}");
        assert!(debug.contains("StagedWrite"));
        assert!(debug.contains("committed: false"));
        assert!(
            !debug.contains(&scratch.path().to_string_lossy().into_owned())
        );
        drop(staged);
    }

    #[test]
    fn substituted_staged_path_is_refused_without_deleting_the_substitute() {
        let scratch = Scratch::new("substitution");
        let target = scratch.path().join("target.txt");
        fs::write(&target, b"old").expect("seed");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"new", None)
                .expect("stage");
        let staged_path = staged.path().to_path_buf();
        fs::remove_file(&staged_path).expect("remove original staged file");
        fs::write(&staged_path, b"substitute").expect("plant substitute");

        let error = staged
            .commit()
            .expect_err("a substituted staged path must be refused");
        assert!(matches!(
            error,
            AtomicWriteFailure::StagedIdentityUnverifiable { .. }
        ));
        assert_eq!(fs::read(&target).expect("read target"), b"old");
        assert_eq!(
            fs::read(&staged_path).expect("read substitute"),
            b"substitute"
        );
    }

    #[test]
    fn same_length_staged_content_substitution_is_refused() {
        // On targets whose identity snapshot is length plus modified time, a
        // same-user process can rewrite the staged bytes without changing the
        // snapshot. The commit re-reads the bytes and refuses.
        let scratch = Scratch::new("content-substitution");
        let target = scratch.path().join("target.txt");
        fs::write(&target, b"old").expect("seed");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"aaaa", None)
                .expect("stage");
        let staged_path = staged.path().to_path_buf();
        fs::write(&staged_path, b"bbbb").expect("replace staged bytes");

        let error = staged
            .commit()
            .expect_err("replaced staged content must be refused");
        assert!(matches!(
            error,
            AtomicWriteFailure::StagedIdentityUnverifiable { .. }
        ));
        assert_eq!(fs::read(&target).expect("read target"), b"old");
    }

    #[test]
    fn stages_and_replaces_an_absent_target() {
        let scratch = Scratch::new("absent");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"hello", None)
                .expect("stage");
        assert!(
            !scratch.path().join("target.txt").exists(),
            "target written early"
        );
        assert!(staged.path().exists(), "staged file missing");
        staged.commit().expect("commit");
        assert_eq!(
            fs::read(scratch.path().join("target.txt")).expect("read"),
            b"hello"
        );
        assert_eq!(scratch.entries(), vec!["target.txt".to_owned()]);
    }

    #[test]
    fn replaces_existing_contents_exactly_and_leaves_no_temporary() {
        let scratch = Scratch::new("replace");
        fs::write(scratch.path().join("target.txt"), b"old").expect("seed");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"", None)
                .expect("stage");
        staged.commit().expect("commit");
        assert_eq!(
            fs::read(scratch.path().join("target.txt")).expect("read"),
            b""
        );
        assert_eq!(scratch.entries(), vec!["target.txt".to_owned()]);
    }

    #[test]
    fn dropping_without_committing_removes_the_staged_file() {
        let scratch = Scratch::new("drop");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"x", None)
                .expect("stage");
        let staged_path = staged.path().to_path_buf();
        drop(staged);
        assert!(!staged_path.exists(), "staged file survived the drop");
        assert!(scratch.entries().is_empty(), "scratch not clean");
    }

    #[test]
    fn a_refused_commit_removes_the_staged_file_and_keeps_the_target() {
        let scratch = Scratch::new("refused");
        let target = scratch.path().join("target.txt");
        fs::create_dir(&target).expect("seed directory target");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"x", None)
                .expect("stage");
        let error =
            staged.commit().expect_err("directory target must be refused");
        assert!(matches!(
            error,
            AtomicWriteFailure::TargetIsNotARegularFile { .. }
        ));
        assert!(target.is_dir(), "the directory target was disturbed");
        assert_eq!(scratch.entries(), vec!["target.txt".to_owned()]);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_target_is_refused_and_the_referent_is_untouched() {
        let scratch = Scratch::new("symlink");
        let referent = scratch.path().join("referent.txt");
        fs::write(&referent, b"referent").expect("seed referent");
        let target = scratch.path().join("target.txt");
        std::os::unix::fs::symlink(&referent, &target).expect("symlink");
        let staged =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"x", None)
                .expect("stage");
        let error =
            staged.commit().expect_err("symlink target must be refused");
        assert!(matches!(
            error,
            AtomicWriteFailure::TargetIsNotARegularFile { .. }
        ));
        assert_eq!(fs::read(&referent).expect("read referent"), b"referent");
        assert!(
            fs::symlink_metadata(&target)
                .expect("lstat")
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_requested_unix_mode_is_applied_to_the_replacement() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("mode");
        let staged = stage_atomic(
            scratch.path(),
            "target.txt",
            ".tmp",
            b"x",
            Some(0o600),
        )
        .expect("stage");
        staged.commit().expect("commit");
        let mode = fs::metadata(scratch.path().join("target.txt"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn staged_names_are_unique_across_many_calls() {
        let scratch = Scratch::new("unique");
        let mut names = std::collections::BTreeSet::new();
        for _ in 0..64 {
            let staged =
                stage_atomic(scratch.path(), "target.txt", ".tmp", b"x", None)
                    .expect("stage");
            assert!(
                names.insert(
                    staged
                        .path()
                        .file_name()
                        .expect("name")
                        .to_string_lossy()
                        .into_owned()
                ),
                "a staged name repeated"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_directory_fails_as_staged() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("readonly");
        fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o500))
            .expect("chmod");
        let result =
            stage_atomic(scratch.path(), "target.txt", ".tmp", b"x", None);
        let _ = fs::set_permissions(
            scratch.path(),
            fs::Permissions::from_mode(0o700),
        );
        let error = result
            .expect_err("a read-only directory must refuse a staged file");
        assert!(matches!(error, AtomicWriteFailure::Staged { .. }));
    }
}
