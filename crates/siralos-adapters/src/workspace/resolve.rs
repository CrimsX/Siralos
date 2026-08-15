//! Containment-safe workspace path resolution (R4, reference
//! `resolveWorkspacePath`).
//!
//! Every model-facing path is validated (NUL, empty, absolute, drive,
//! parent traversal), joined against the canonical root, checked for
//! lexical containment, then canonicalized (symlinks resolved) and
//! re-checked for containment, so a symlink/junction/reparse escape
//! never widens authority. The returned workspace-relative path is
//! the canonical target's relative path with `/` separators, exactly
//! like the reference.

use crate::workspace::fs::normalize_join;

use std::fmt;
use std::path::{Path, PathBuf};

/// A successfully resolved workspace path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspacePath {
    /// Canonical target path relative to the canonical root (`/`
    /// separators; `"."` for the root itself).
    pub workspace_relative_path: String,
    /// Canonical absolute target path.
    pub absolute_path: PathBuf,
}

/// Why a workspace path was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRejection {
    /// The path contains a NUL byte.
    NullByte,
    /// The path is empty.
    Empty,
    /// The path is absolute or drive-prefixed.
    Absolute,
    /// The resolved path escapes the workspace.
    OutsideWorkspace,
    /// The path cannot be canonicalized.
    Unresolvable(String),
    /// The canonical target escapes the workspace (link escape).
    LinkEscape,
}

impl fmt::Display for PathRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NullByte => {
                formatter.write_str("Path contains a null byte.")
            }
            Self::Empty => formatter.write_str("Path is empty."),
            Self::Absolute => {
                formatter.write_str("Path must be relative to the workspace.")
            }
            Self::OutsideWorkspace => {
                formatter.write_str("Path is outside the Siralos workspace.")
            }
            Self::Unresolvable(detail) => {
                write!(formatter, "Path cannot be resolved: {detail}")
            }
            Self::LinkEscape => {
                formatter.write_str("Path is outside the Siralos workspace.")
            }
        }
    }
}

impl std::error::Error for PathRejection {}

/// Resolve one requested workspace path against the canonical root,
/// mirroring the reference resolution order and messages.
pub fn resolve_workspace_path(
    root: &Path,
    requested: &str,
) -> Result<ResolvedWorkspacePath, PathRejection> {
    if requested.contains('\0') {
        return Err(PathRejection::NullByte);
    }
    if requested.is_empty() {
        return Err(PathRejection::Empty);
    }
    if is_absolute_pattern(requested) {
        return Err(PathRejection::Absolute);
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|error| {
        PathRejection::Unresolvable(format!(
            "Workspace root is not accessible: {error}"
        ))
    })?;
    let resolved = normalize_join(&canonical_root, requested);
    if resolved != canonical_root && !resolved.starts_with(&canonical_root) {
        return Err(PathRejection::OutsideWorkspace);
    }
    let canonical_target = std::fs::canonicalize(&resolved)
        .map_err(|error| PathRejection::Unresolvable(error.to_string()))?;
    if canonical_target != canonical_root
        && !canonical_target.starts_with(&canonical_root)
    {
        return Err(PathRejection::LinkEscape);
    }
    let workspace_relative_path = if canonical_target == canonical_root {
        ".".to_owned()
    } else {
        let relative = canonical_target
            .strip_prefix(&canonical_root)
            .map_err(|_| PathRejection::OutsideWorkspace)?;
        let mut components = Vec::new();
        for component in relative.components() {
            if let std::path::Component::Normal(name) = component {
                components.push(name.to_string_lossy().into_owned());
            }
        }
        components.join("/")
    };
    Ok(ResolvedWorkspacePath {
        workspace_relative_path,
        absolute_path: canonical_target,
    })
}

/// Absolute-path detection matching the reference patterns
/// `^(?:[A-Za-z]:)?[\\/]` and `^[A-Za-z]:`.
fn is_absolute_pattern(value: &str) -> bool {
    let bytes = value.as_bytes();
    let drive_prefix =
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if drive_prefix {
        return true;
    }
    bytes.first().is_some_and(|byte| *byte == b'/' || *byte == b'\\')
}

#[cfg(test)]
mod tests {
    use super::{PathRejection, resolve_workspace_path};

    use std::fs;

    #[test]
    fn rejects_escape_and_absolute_requests() {
        let root = std::env::temp_dir();
        assert_eq!(
            resolve_workspace_path(&root, "../x").unwrap_err(),
            PathRejection::OutsideWorkspace,
        );
        assert_eq!(
            resolve_workspace_path(&root, "/etc").unwrap_err(),
            PathRejection::Absolute,
        );
        assert_eq!(
            resolve_workspace_path(&root, "a\0b").unwrap_err(),
            PathRejection::NullByte,
        );
        assert_eq!(
            resolve_workspace_path(&root, "").unwrap_err(),
            PathRejection::Empty,
        );
    }

    #[test]
    fn resolves_relative_paths_and_root_itself() {
        let base = std::env::temp_dir().join("siralos-resolve-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("a")).unwrap();
        fs::write(base.join("a/f.txt"), "x").unwrap();
        let resolved = resolve_workspace_path(&base, "a/f.txt").unwrap();
        assert_eq!(resolved.workspace_relative_path, "a/f.txt");
        let root = resolve_workspace_path(&base, ".").unwrap();
        assert_eq!(root.workspace_relative_path, ".");
        let _ = fs::remove_dir_all(&base);
    }
}
