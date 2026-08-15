//! Validated workspace-relative paths (R4, ADR 0016 path safety).
//!
//! One validated path type replaces repeated raw-string validation at
//! the model-facing boundary. The validator mirrors the TypeScript
//! reference `validateRelativeWorkspacePath`/`resolveWorkspacePath`
//! security model: NUL bytes, empty paths, absolute paths, drive
//! prefixes, and parent traversal are rejected; both `/` and `\` are
//! separators everywhere. Filesystem-native paths (`Path`/`PathBuf`)
//! remain the internal representation below the validated boundary.

use std::fmt;

/// Why a workspace-relative path was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathValidationError {
    /// The path contains a NUL byte.
    NullByte,
    /// The path is empty.
    Empty,
    /// The path is absolute or carries a drive/prefix.
    Absolute,
    /// A `..` component would escape the workspace.
    ParentTraversal,
}

impl fmt::Display for PathValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullByte => {
                formatter.write_str("Path contains a null byte.")
            }
            Self::Empty => formatter.write_str("Path is empty."),
            Self::Absolute => {
                formatter.write_str("Path must be relative to the workspace.")
            }
            Self::ParentTraversal => {
                formatter.write_str("Path must remain inside the workspace.")
            }
        }
    }
}

/// A validated workspace-relative path.
///
/// The exact caller-supplied spelling is retained (the reference stores
/// resolved canonical relative paths verbatim); the validation rejects
/// every escape form the security model forbids. The path itself grants
/// no authority: every operation still resolves it against the
/// canonical workspace root and re-checks containment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkspaceRelativePath(String);

impl WorkspaceRelativePath {
    /// Validate and construct a workspace-relative path.
    pub fn parse(value: &str) -> Result<Self, PathValidationError> {
        validate_relative_path(value)?;
        Ok(Self(value.to_owned()))
    }

    /// The exact validated relative path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Path components split on `/` and `\` (empty and `.` components
    /// removed), used by exclusion and protection classification.
    pub fn components(&self) -> Vec<&str> {
        self.0
            .split(['/', '\\'])
            .filter(|component| !component.is_empty() && *component != ".")
            .collect()
    }

    /// The final path component, if any.
    pub fn basename(&self) -> Option<&str> {
        self.components().into_iter().next_back()
    }

    /// True when any component equals an excluded directory name under
    /// the platform case-folding policy (Windows/macOS fold).
    pub fn is_in_excluded_directory(
        &self,
        excluded: &[&str],
        fold: bool,
    ) -> Option<String> {
        let fold_component = |component: &str| -> String {
            if fold { component.to_lowercase() } else { component.to_owned() }
        };
        let excluded: Vec<String> =
            excluded.iter().map(|name| fold_component(name)).collect();
        for component in self.components() {
            let folded = fold_component(component);
            if excluded.contains(&folded) {
                return Some(component.to_owned());
            }
        }
        None
    }
}

impl fmt::Display for WorkspaceRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Validate a raw relative path against the reference security model.
pub fn validate_relative_path(value: &str) -> Result<(), PathValidationError> {
    if value.contains('\0') {
        return Err(PathValidationError::NullByte);
    }
    if value.is_empty() {
        return Err(PathValidationError::Empty);
    }
    if is_absolute_pattern(value) {
        return Err(PathValidationError::Absolute);
    }
    if value.split(['/', '\\']).any(|component| component == "..") {
        return Err(PathValidationError::ParentTraversal);
    }
    Ok(())
}

/// Absolute-path detection matching the reference patterns
/// `^(?:[A-Za-z]:)?[\\/]` and `^[A-Za-z]:` (drive letters).
fn is_absolute_pattern(value: &str) -> bool {
    let bytes = value.as_bytes();
    let drive_prefix =
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    // The reference rejects any drive-prefixed path (`DRIVE_PATTERN`
    // `^[A-Za-z]:`), not only drive paths with a following separator.
    if drive_prefix {
        return true;
    }
    bytes.first().is_some_and(|byte| *byte == b'/' || *byte == b'\\')
}

/// True when the workspace-relative path is protected behavioral
/// configuration (`AGENTS.md` at any depth, `.siralos/**`), matching
/// the reference classifier exactly (case-insensitive basename/component
/// matching on every platform).
pub fn is_protected_behavioral_config_path(relative: &str) -> bool {
    let normalized = relative.replace('\\', "/").replacen("./", "", 1);
    if normalized.is_empty() || normalized == "." {
        return false;
    }
    let components: Vec<&str> = normalized
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    let basename = components.last().copied().unwrap_or("");
    if components
        .iter()
        .any(|component| component.eq_ignore_ascii_case(".siralos"))
    {
        return true;
    }
    basename.eq_ignore_ascii_case("AGENTS.md")
}

/// Protected write targets under the reference `develop-offline`
/// policy: `.git`/`.siralos` components, `.env`, `.env.*`, `*.pem`,
/// `*.key`, and behavioral configuration. Component matching folds
/// case on Windows/macOS (conservatively: macOS is treated as
/// case-insensitive), so `.GIT`/`.Git`/`.ENV` variants cannot address
/// protected paths. Protection cannot be overridden by approval.
pub fn is_protected_write_target(relative: &str, fold: bool) -> bool {
    let path = match WorkspaceRelativePath::parse(relative) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let fold_component = |component: &str| -> String {
        if fold { component.to_lowercase() } else { component.to_owned() }
    };
    let components: Vec<String> =
        path.components().iter().map(|c| fold_component(c)).collect();
    let basename = components.last().cloned().unwrap_or_default();
    if components
        .iter()
        .any(|component| component == ".git" || component == ".siralos")
    {
        return true;
    }
    if is_protected_behavioral_config_path(relative) {
        return true;
    }
    basename == ".env"
        || basename.starts_with(".env.")
        || basename.ends_with(".pem")
        || basename.ends_with(".key")
}

#[cfg(test)]
mod tests {
    use super::{
        PathValidationError, WorkspaceRelativePath,
        is_protected_behavioral_config_path, is_protected_write_target,
        validate_relative_path,
    };

    #[test]
    fn rejects_traversal_and_escape_forms() {
        for value in ["../secret.txt", "a/../../b", "a\\..\\b"] {
            assert_eq!(
                validate_relative_path(value),
                Err(PathValidationError::ParentTraversal)
            );
        }
        // A leading separator is absolute before it is traversal.
        assert_eq!(
            validate_relative_path("\\..\\x"),
            Err(PathValidationError::Absolute)
        );
    }

    #[test]
    fn rejects_absolute_and_drive_paths() {
        for value in
            ["/etc/passwd", "\\server\\share", "C:\\x", "c:/x", "C:", "d:y"]
        {
            assert_eq!(
                validate_relative_path(value),
                Err(PathValidationError::Absolute)
            );
        }
    }

    #[test]
    fn rejects_empty_and_nul_paths() {
        assert_eq!(
            validate_relative_path(""),
            Err(PathValidationError::Empty)
        );
        assert_eq!(
            validate_relative_path("a\0b"),
            Err(PathValidationError::NullByte)
        );
    }

    #[test]
    fn accepts_plain_relative_paths() {
        for value in ["a.txt", "src/main.ts", "a//b", "a/./b", "dir/"] {
            assert!(validate_relative_path(value).is_ok(), "{}", value);
        }
    }

    #[test]
    fn behavioral_config_classification_matches_reference() {
        for path in [
            "AGENTS.md",
            "src/AGENTS.md",
            "agents.md",
            "src/AgEnTs.MD",
            ".siralos",
            ".siralos/config.json",
            "src/.siralos/workflows/dev.json",
            ".SIRALOS/rules",
        ] {
            assert!(is_protected_behavioral_config_path(path), "{}", path);
        }
        for path in [
            "src/player/player.gd",
            "docs/manual.md",
            "addons/foo/README.md",
            "agents_md.txt",
        ] {
            assert!(!is_protected_behavioral_config_path(path), "{}", path);
        }
    }

    #[test]
    fn protected_write_targets_fold_case_on_insensitive_platforms() {
        for path in [
            ".git/config",
            ".siralos/config.json",
            ".env",
            ".env.local",
            "keys/id_rsa.pem",
            "secrets/tls.key",
            "AGENTS.md",
            "src/AGENTS.md",
        ] {
            assert!(is_protected_write_target(path, true), "{}", path);
        }
        assert!(is_protected_write_target(".GIT/config", true));
        assert!(!is_protected_write_target("NODE_MODULES/x", false));
        assert!(!is_protected_write_target("src/main.ts", false));
        assert!(!is_protected_write_target("src/main.ts", true));
    }

    #[test]
    fn components_and_basename_are_deterministic() {
        let path = WorkspaceRelativePath::parse("src/deep/player.gd").unwrap();
        assert_eq!(path.components(), ["src", "deep", "player.gd"]);
        assert_eq!(path.basename(), Some("player.gd"));
        let windows = WorkspaceRelativePath::parse("src\\player.gd").unwrap();
        assert_eq!(windows.components(), ["src", "player.gd"]);
    }
}
