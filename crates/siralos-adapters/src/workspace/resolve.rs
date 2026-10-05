//! Containment-safe workspace path resolution (R4, reference
//! `resolveWorkspacePath`).
//!
//! Every model-facing path is validated (NUL, empty, absolute, drive,
//! parent traversal), joined against the canonical root, checked for
//! lexical containment, then canonicalized (symlinks resolved) and
//! re-checked for containment, so any symlink/junction/reparse escape
//! present at resolution time is rejected and the resolved path is the
//! canonical in-workspace target. This is validation-time containment:
//! the mechanism does not bind the later pathname-based open to the
//! validated object against a same-user process that substitutes the
//! target or a parent after resolution (see SECURITY.md "Workspace
//! read containment"). The returned workspace-relative path is the
//! canonical target's relative path with `/` separators, exactly like
//! the reference.

use crate::workspace::fs::normalize_join;

use std::fmt;
use std::path::{Path, PathBuf};

/// A successfully resolved workspace path.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolvedWorkspacePath {
    /// Canonical target path relative to the canonical root (`/`
    /// separators; `"."` for the root itself).
    pub workspace_relative_path: String,
    /// Canonical absolute target path.
    ///
    /// This is host-only: report-safe output, logs, and provider projections
    /// must use [`Self::workspace_relative_path`] instead.
    pub absolute_path: PathBuf,
}

impl fmt::Debug for ResolvedWorkspacePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // An absolute workspace path must not reach any diagnostic surface, so
        // `Debug` projects only the workspace-relative name.
        formatter
            .debug_struct("ResolvedWorkspacePath")
            .field("workspace_relative_path", &self.workspace_relative_path)
            .field("absolute_path", &"[ABSOLUTE]")
            .finish()
    }
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
    /// A path component contains a reserved character.
    InvalidCharacter,
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
            Self::InvalidCharacter => {
                formatter.write_str("Path contains an invalid character.")
            }
            Self::OutsideWorkspace => {
                formatter.write_str("Path is outside the Siralos workspace.")
            }
            Self::Unresolvable(_) => {
                formatter.write_str("Path cannot be resolved.")
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
    if siralos_core::workspace::path::is_absolute_pattern(requested) {
        return Err(PathRejection::Absolute);
    }
    if requested.split(['/', '\\']).any(|component| component.contains(':')) {
        return Err(PathRejection::InvalidCharacter);
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|_error| {
        PathRejection::Unresolvable(
            "workspace root is not accessible".to_owned(),
        )
    })?;
    let resolved = normalize_join(&canonical_root, requested);
    if resolved != canonical_root && !resolved.starts_with(&canonical_root) {
        return Err(PathRejection::OutsideWorkspace);
    }
    let canonical_target =
        std::fs::canonicalize(&resolved).map_err(|_error| {
            PathRejection::Unresolvable(
                "requested path is not accessible".to_owned(),
            )
        })?;
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

    #[cfg(unix)]
    #[test]
    fn parent_symlink_escape_is_rejected_and_inside_links_are_allowed() {
        // Policy A: a symlink/junction/reparse parent is allowed only
        // when its canonical target stays inside the workspace; any
        // escape is rejected at resolution (validation time).
        let base = std::env::temp_dir().join("siralos-resolve-link-test");
        let outside =
            std::env::temp_dir().join("siralos-resolve-link-outside");
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(base.join("real")).unwrap();
        fs::create_dir_all(base.join("alias")).unwrap();
        fs::write(base.join("real/f.txt"), "x").unwrap();
        fs::write(base.join("alias/f.txt"), "x").unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "outside").unwrap();
        std::os::unix::fs::symlink(&outside, base.join("link")).unwrap();
        std::os::unix::fs::symlink(
            base.join("alias"),
            base.join("inside-link"),
        )
        .unwrap();
        // Escape through a symlinked parent fails closed.
        assert_eq!(
            resolve_workspace_path(&base, "link/secret.txt").unwrap_err(),
            PathRejection::LinkEscape,
        );
        assert_eq!(
            resolve_workspace_path(&base, "link").unwrap_err(),
            PathRejection::LinkEscape,
        );
        // An in-workspace parent symlink resolves to its canonical
        // target and stays contained.
        let resolved =
            resolve_workspace_path(&base, "inside-link/f.txt").unwrap();
        assert_eq!(resolved.workspace_relative_path, "alias/f.txt");
        assert!(resolved.absolute_path.starts_with(&base));
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn resolved_path_debug_does_not_expose_an_absolute_path() {
        let base = std::env::temp_dir().join("siralos-resolve-debug-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("f.txt"), "x").unwrap();
        let resolved = resolve_workspace_path(&base, "f.txt").unwrap();
        let debug = format!("{resolved:?}");
        assert!(debug.contains("f.txt"), "{debug}");
        assert!(
            !debug.contains(&base.to_string_lossy().to_string()),
            "{debug}"
        );
        let _ = fs::remove_dir_all(&base);
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
