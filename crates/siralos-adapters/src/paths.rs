//! User state directory resolution.
//!
//! Siralos keeps user-level state (configuration, runs, checkpoints)
//! beneath one home-directory-owned state directory, canonically
//! `~/.siralos`. R1 establishes the resolution primitive only; the
//! TypeScript reference implementation remains authoritative for the
//! full layout.
//!
//! Filesystem identities are kept in [`PathBuf`] for as long as
//! practical: no component of this module assumes the home directory or
//! the resulting path is valid UTF-8.

use std::env;
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
/// Home resolution mirrors the TypeScript reference implementation
/// (`os.homedir()`): `USERPROFILE`, then `HOMEDRIVE` + `HOMEPATH`, on
/// Windows; `HOME` elsewhere. The directory is not created and nothing is
/// written; this function is read-only.
///
/// # Errors
///
/// Returns [`StateDirError::NoHomeDirectory`] when no home directory can
/// be determined from the environment.
pub fn state_dir() -> Result<PathBuf, StateDirError> {
    let home = home_dir_from_env().ok_or(StateDirError::NoHomeDirectory)?;
    Ok(state_dir_for(&home))
}

/// Build the state directory path beneath `home`.
///
/// Pure path construction with no environment access; kept `pub(crate)`
/// so the canonical name is exercised by tests without mutating process
/// environment (which would require `unsafe` under edition 2024).
pub(crate) fn state_dir_for(home: &Path) -> PathBuf {
    home.join(STATE_DIR_NAME)
}

/// Read the home directory from the process environment.
///
/// `OsString` values are preserved verbatim; a set-but-empty variable is
/// treated as absent because joining an empty component would silently
/// change the resulting path's meaning.
fn home_dir_from_env() -> Option<PathBuf> {
    if cfg!(windows) {
        let profile = env::var_os("USERPROFILE");
        if let Some(profile) = profile.filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(profile));
        }
        let drive = env::var_os("HOMEDRIVE").filter(|value| !value.is_empty());
        let path = env::var_os("HOMEPATH").filter(|value| !value.is_empty());
        match (drive, path) {
            (Some(drive), Some(path)) => {
                let mut home = PathBuf::from(drive);
                home.push(path);
                Some(home)
            }
            _ => None,
        }
    } else {
        env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }
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
        let home =
            PathBuf::from(OsStringExt::from_vec(b"/home/\xFF\xFE".to_vec()));
        let state = state_dir_for(&home);
        assert!(state.ends_with(STATE_DIR_NAME));
        assert!(state.starts_with(&home));
    }
}
