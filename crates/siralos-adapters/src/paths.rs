//! User state directory resolution.
//!
//! Siralos keeps user-level state (configuration, runs, checkpoints)
//! beneath one home-directory-owned state directory, canonically
//! `~/.siralos`. R1 established the resolution primitive; the
//! differential behavioral harness (ADR 0033) audits it against the
//! TypeScript reference and drives remediation of drift. The TypeScript
//! reference implementation remains authoritative for the full layout.
//!
//! Home resolution mirrors the reference's `os.homedir()` (libuv):
//! - Windows: `USERPROFILE` when set and non-empty; an explicitly set
//!   but empty `USERPROFILE` is a resolution failure (libuv reports
//!   ENOENT rather than falling back); otherwise the OS user profile.
//!   `HOMEDRIVE`/`HOMEPATH` are not consulted (current libuv does not
//!   use them; documented divergence from the historical Node docs).
//! - POSIX: `HOME` when set and non-empty; otherwise the OS user
//!   database home (getpwuid).
//!
//! The OS-user-database fallback lives in the `dirs` crate behind a
//! safe wrapper: the standard library has no equivalent, and
//! hand-rolling it would require unsafe FFI, which is forbidden.
//!
//! Filesystem identities are kept in [`PathBuf`] for as long as
//! practical: no component of this module assumes the home directory or
//! the resulting path is valid UTF-8.

use std::fmt;
use std::path::{Path, PathBuf};

/// Canonical name of the user state directory inside the home directory.
///
/// The leading dot keeps the directory hidden on Unix-like systems and
/// marks it as host-owned configuration on Windows; it must never be
/// inside the workspace namespace.
const STATE_DIR_NAME: &str = ".siralos";

/// Resolve the canonical user state directory from the process
/// environment.
///
/// The directory is not created and nothing is written; this function
/// is read-only.
///
/// # Errors
///
/// Returns [`StateDirError::NoHomeDirectory`] when no home directory can
/// be determined (including an explicitly empty `USERPROFILE` on
/// Windows, which the reference treats as a failure).
pub fn state_dir() -> Result<PathBuf, StateDirError> {
    // Windows order must mirror the reference (`os.homedir()`/libuv):
    // `USERPROFILE` wins when set and non-empty, an explicitly empty
    // `USERPROFILE` is a failure, and otherwise the OS user profile is
    // used. `dirs` alone cannot express this order (it prefers the
    // known-folder profile over `USERPROFILE`), so the primary variable
    // is resolved here and `dirs` supplies the OS-profile fallback.
    #[cfg(windows)]
    {
        if std::env::var_os("USERPROFILE")
            .is_some_and(|value| value.is_empty())
        {
            return Err(StateDirError::NoHomeDirectory);
        }
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .ok_or(StateDirError::NoHomeDirectory)?;
        Ok(state_dir_for(&home))
    }
    // POSIX order mirrors the reference: `HOME` wins when set and
    // non-empty (dirs treats an empty `HOME` as absent, matching
    // libuv's fallback to the user database), otherwise getpwuid.
    #[cfg(not(windows))]
    {
        let home = dirs::home_dir().ok_or(StateDirError::NoHomeDirectory)?;
        Ok(state_dir_for(&home))
    }
}

/// Build the state directory path beneath `home`.
///
/// Pure path construction with no environment access; kept `pub(crate)`
/// so the canonical name is exercised by tests without mutating process
/// environment (which would require `unsafe` under edition 2024).
pub(crate) fn state_dir_for(home: &Path) -> PathBuf {
    home.join(STATE_DIR_NAME)
}

/// Failure to resolve the user state directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateDirError {
    /// No home directory could be determined from the environment.
    NoHomeDirectory,
}

impl fmt::Display for StateDirError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHomeDirectory => formatter.write_str(
                "no home directory could be determined from the environment",
            ),
        }
    }
}

impl std::error::Error for StateDirError {}

#[cfg(test)]
mod tests {
    use super::{STATE_DIR_NAME, state_dir_for};
    use std::path::{Path, PathBuf};

    #[test]
    fn state_dir_name_is_the_canonical_identity() {
        assert_eq!(STATE_DIR_NAME, ".siralos");
    }

    #[test]
    fn appends_the_state_dir_name_beneath_any_home_path() {
        let home = Path::new("/home/user");
        assert_eq!(state_dir_for(home), PathBuf::from("/home/user/.siralos"));
    }

    #[test]
    fn preserves_unicode_home_paths_verbatim() {
        // Unicode home paths must be preserved byte-for-byte; no
        // normalization, no lossy conversion.
        let home = Path::new("/home/über-使用者");
        let state = state_dir_for(home);
        assert_eq!(state, PathBuf::from("/home/über-使用者/.siralos"));
    }

    #[cfg(windows)]
    #[test]
    fn preserves_non_utf8_home_paths_on_windows() {
        use std::os::windows::ffi::OsStringExt;
        // Unpaired surrogate 0xD800 is not valid UTF-8; the state-dir
        // computation must not require the home path to be valid UTF-8.
        // The explicit OsString binding and u16 literals pin inference
        // (PathBuf::from over an unresolved from_wide result is
        // ambiguous on current toolchains).
        let wide: std::ffi::OsString = OsStringExt::from_wide(&[
            0x0043u16, 0x003A, 0x005C, 0xD800, 0x005C,
        ]);
        let home = PathBuf::from(wide);
        let state = state_dir_for(&home);
        assert!(state.ends_with(STATE_DIR_NAME));
        assert!(state.starts_with(&home));
    }

    #[cfg(not(windows))]
    #[test]
    fn preserves_non_utf8_home_paths_on_unix() {
        use std::os::unix::ffi::OsStringExt;
        // Bind the extension-trait result to its concrete type. Rust 1.85
        // cannot infer it through PathBuf's multiple From implementations.
        let bytes = std::ffi::OsString::from_vec(b"/home/\xFF\xFE".to_vec());
        let home = PathBuf::from(bytes);
        let state = state_dir_for(&home);
        assert!(state.ends_with(STATE_DIR_NAME));
        assert!(state.starts_with(&home));
    }
}
