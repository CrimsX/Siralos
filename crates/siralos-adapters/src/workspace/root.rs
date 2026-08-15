//! Canonical workspace root resolution (R4).
//!
//! The workspace root is canonicalized exactly once at construction
//! and every model-facing path stays contained within it; there is no
//! current-working-directory dependence after construction. The root
//! itself must be a real directory.

use std::fmt;
use std::path::PathBuf;

/// Why the workspace root could not be established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRootError {
    /// The root cannot be canonicalized (missing or inaccessible).
    NotAccessible(String),
    /// The canonical root is not a directory.
    NotADirectory,
}

impl fmt::Display for WorkspaceRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NotAccessible(detail) => {
                write!(formatter, "Workspace root is not accessible: {detail}")
            }
            Self::NotADirectory => {
                formatter.write_str("Workspace root is not a directory.")
            }
        }
    }
}

impl std::error::Error for WorkspaceRootError {}

/// Canonicalize the launch workspace once and verify it is a real
/// directory. The returned path is the single containment root for
/// every workspace operation in this session.
pub fn resolve_workspace_root(
    cwd: &std::path::Path,
) -> Result<PathBuf, WorkspaceRootError> {
    let canonical = std::fs::canonicalize(cwd).map_err(|error| {
        WorkspaceRootError::NotAccessible(error.to_string())
    })?;
    let metadata = std::fs::metadata(&canonical).map_err(|error| {
        WorkspaceRootError::NotAccessible(error.to_string())
    })?;
    if !metadata.is_dir() {
        return Err(WorkspaceRootError::NotADirectory);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::resolve_workspace_root;

    #[test]
    fn resolves_a_real_directory_and_rejects_missing_paths() {
        let root = std::env::temp_dir();
        assert!(resolve_workspace_root(&root).is_ok());
        let missing = root.join("siralos-missing-root-does-not-exist");
        assert!(resolve_workspace_root(&missing).is_err());
    }
}
