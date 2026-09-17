//! Atomic staged replacement of single-owner host files (W4.3).
//!
//! Six production writers had grown the same algorithm by copy: stage the bytes in
//! a temporary file beside the target, check that the target is a regular file,
//! rename over it, and clean the temporary up on every failure. This module is
//! that algorithm once.
//!
//! **What the mechanism guarantees.** The temporary file is created in the
//! target's own directory with `create_new` (O_EXCL), so it is on the same
//! filesystem and cannot be a path someone else already owns — including a symlink
//! planted at the nonce path, which `std::fs::write` would have followed. The bytes
//! are flushed with `sync_all` before the swap, so a crash cannot leave a truncated
//! replacement of a file whose digest gates identity. The swap itself is
//! `std::fs::rename`, which replaces the directory entry atomically.
//!
//! **What it does not guarantee.** The target check before the swap is advisory: a
//! target swapped between the check and the rename can only be *replaced*, never
//! followed, because the staged path was created exclusively. Nothing here closes a
//! race for a caller that reads the target concurrently; it closes the write path,
//! which is the one this module owns.
//!
//! **Permissions.** A staged file carries the process default mode unless the
//! caller passes one, so a replacement adopts the staged file's permissions rather
//! than the replaced file's. A caller that needs a specific mode passes it.
//!
//! **Platform.** Directory-entry replacement is atomic on the filesystems this
//! product targets; a rename over an open file fails on Windows, which surfaces as
//! a typed replacement failure rather than a silent write.

use std::fs::{OpenOptions, remove_file, rename, symlink_metadata};
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
#[derive(Debug)]
pub enum AtomicWriteFailure {
    /// The staged bytes could not be written, or a staged name could not be
    /// claimed after one retry.
    Staged {
        /// The staged path that could not be created or written.
        path: PathBuf,
        /// The underlying failure.
        source: std::io::Error,
    },
    /// The target exists and is a symlink or not a regular file.
    TargetIsNotARegularFile {
        /// The refused target.
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

impl std::fmt::Display for AtomicWriteFailure {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Staged { path, source } => {
                write!(
                    formatter,
                    "{} could not be staged: {source}",
                    path.display()
                )
            }
            Self::TargetIsNotARegularFile { path } => write!(
                formatter,
                "{} must be a regular file; refusing symlink or special file",
                path.display()
            ),
            Self::TargetUnreadable { path, source } => {
                write!(formatter, "{} is unreadable: {source}", path.display())
            }
            Self::ReplaceFailed { path, source } => {
                write!(
                    formatter,
                    "{} could not be replaced: {source}",
                    path.display()
                )
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
            Self::TargetIsNotARegularFile { .. } => None,
        }
    }
}

/// One staged replacement: the bytes are on disk, the target is untouched.
///
/// Dropping the value removes the staged file, so a failure between staging and
/// committing cannot leave a temporary behind.
#[derive(Debug)]
pub struct StagedWrite {
    temporary: PathBuf,
    target: PathBuf,
    committed: bool,
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
    /// symlink or special file is refused and the staged file is removed.
    ///
    /// # Errors
    /// Returns an AtomicWriteFailure when the target is unreadable, is not a
    /// regular file, or cannot be replaced.
    pub fn commit(mut self) -> Result<(), AtomicWriteFailure> {
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
        rename(&self.temporary, &self.target).map_err(|source| {
            AtomicWriteFailure::ReplaceFailed {
                path: self.target.clone(),
                source,
            }
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedWrite {
    fn drop(&mut self) {
        if !self.committed {
            let _ = remove_file(&self.temporary);
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
/// Returns an AtomicWriteFailure when no staged path could be created or the bytes
/// could not be written.
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
        match OpenOptions::new().write(true).create_new(true).open(&temporary)
        {
            Ok(mut file) => {
                if let Err(source) =
                    file.write_all(contents).and_then(|()| file.sync_all())
                {
                    let _ = remove_file(&temporary);
                    return Err(AtomicWriteFailure::Staged {
                        path: temporary,
                        source,
                    });
                }
                drop(file);
                apply_mode(&temporary, mode);
                return Ok(StagedWrite {
                    temporary,
                    target,
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

/// Apply the caller's Unix mode to a staged file, where the platform has one.
#[cfg(unix)]
fn apply_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let _ = std::fs::set_permissions(
            path,
            std::fs::Permissions::from_mode(mode),
        );
    }
}

/// Platforms without Unix permission bits ignore the requested mode.
#[cfg(not(unix))]
fn apply_mode(_path: &Path, _mode: Option<u32>) {}

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
