//! Static, bounded, read-only enumeration and validation of workspace
//! GDScript files for check-only diagnostics.
//!
//! The source workspace is only read here; the checked engine only ever
//! sees the disposable mirror. Symlinked scripts and directories are
//! never followed, excluded directories are skipped case-insensitively,
//! and every bound sets the explicit truncation flag instead of failing.

use std::fs;
use std::path::{Path, PathBuf};

use crate::workspace::resolve::resolve_workspace_path;
use siralos_core::godot::GODOT_LIMITS;
use siralos_core::identity::sha256_hex;

/// Directory names never scanned for project GDScript files.
pub const PROJECT_SCAN_EXCLUDED_DIRECTORIES: [&str; 6] =
    [".git", ".godot", "node_modules", "dist", "coverage", ".siralos"];

const MUTATION_STAGING_PREFIXES: [&str; 2] =
    [".siralos-mutation-", ".siralos-quarantine-"];

/// One enumerated script target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotScriptTarget {
    /// Workspace-relative path with `/` separators.
    pub path: String,
    /// File size in bytes.
    pub bytes: u64,
}

/// Deterministic enumeration result over eligible `.gd` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotScriptEnumeration {
    /// Sorted workspace-relative targets with `/` separators.
    pub targets: Vec<GodotScriptTarget>,
    /// True when any bound cut the walk short.
    pub truncated: bool,
}

/// Explicit enumeration bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumerationLimits {
    /// Maximum number of collected scripts.
    pub max_files: usize,
    /// Maximum total collected bytes.
    pub max_total_bytes: u64,
    /// Maximum directory entries examined per directory.
    pub max_entries: usize,
    /// Maximum directory depth below the root.
    pub max_depth: usize,
}

impl Default for EnumerationLimits {
    fn default() -> Self {
        Self {
            max_files: GODOT_LIMITS.max_gdscript_files_per_project,
            max_total_bytes: GODOT_LIMITS.max_gdscript_total_bytes as u64,
            max_entries: GODOT_LIMITS.max_project_entries_examined,
            max_depth: GODOT_LIMITS.max_mirror_depth,
        }
    }
}

/// Statically enumerate eligible `.gd` files deterministically.
pub fn enumerate_gdscript_files(
    workspace_root: &Path,
) -> GodotScriptEnumeration {
    enumerate_gdscript_files_with_limits(
        workspace_root,
        EnumerationLimits::default(),
    )
}

/// Enumerate with explicit bounds.
pub fn enumerate_gdscript_files_with_limits(
    workspace_root: &Path,
    limits: EnumerationLimits,
) -> GodotScriptEnumeration {
    let mut state = WalkState {
        collected: Vec::new(),
        total_bytes: 0,
        truncated: false,
        limits,
    };
    walk(workspace_root, "", 0, &mut state);
    state.collected.sort_by(|left, right| left.path.cmp(&right.path));
    GodotScriptEnumeration {
        targets: state.collected,
        truncated: state.truncated,
    }
}

struct WalkState {
    collected: Vec<GodotScriptTarget>,
    total_bytes: u64,
    truncated: bool,
    limits: EnumerationLimits,
}

fn walk(
    directory: &Path,
    relative_directory: &str,
    depth: usize,
    state: &mut WalkState,
) {
    if state.truncated || depth > state.limits.max_depth {
        if depth > state.limits.max_depth {
            state.truncated = true;
        }
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    if entries.len() > state.limits.max_entries {
        state.truncated = true;
        return;
    }
    for entry in entries.drain(..) {
        if state.truncated {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if PROJECT_SCAN_EXCLUDED_DIRECTORIES
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(&name))
            || MUTATION_STAGING_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        {
            continue;
        }
        let entry_path = entry.path();
        let metadata = match fs::symlink_metadata(&entry_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            walk(
                &entry_path,
                &join_relative(relative_directory, &name),
                depth + 1,
                state,
            );
            continue;
        }
        if !metadata.is_file() || !name.to_lowercase().ends_with(".gd") {
            continue;
        }
        let size = metadata.len();
        if size > GODOT_LIMITS.max_gdscript_file_bytes as u64
            || state.collected.len() >= state.limits.max_files
            || state.total_bytes + size > state.limits.max_total_bytes
        {
            state.truncated = true;
            return;
        }
        state.collected.push(GodotScriptTarget {
            path: join_relative(relative_directory, &name),
            bytes: size,
        });
        state.total_bytes += size;
    }
}

fn join_relative(relative_directory: &str, name: &str) -> String {
    if relative_directory.is_empty() {
        name.to_owned()
    } else {
        format!("{relative_directory}/{name}")
    }
}

/// Why a check-script request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GodotCheckScriptRejection {
    /// The path is lexically invalid.
    InvalidPath,
    /// The path is absolute.
    Absolute,
    /// The path escapes the workspace.
    Traversal,
    /// The path is not a `.gd` script.
    NotGd,
    /// The script does not exist.
    Missing,
    /// The target is not a regular file.
    NotRegular,
    /// The target is a symbolic link.
    Symlink,
    /// The script exceeds the per-file bound.
    TooLarge,
    /// The script could not be read or verified.
    Unreadable,
}

/// A validated, hashed check-script target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedCheckScript {
    /// Canonical absolute path.
    pub canonical_path: PathBuf,
    /// SHA-256 over the exact bytes (64 hex).
    pub sha256: String,
    /// Size in bytes.
    pub bytes: u64,
}

/// A refused check-script request with its typed reason and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCheckScriptFailure {
    /// Typed refusal reason.
    pub reason: GodotCheckScriptRejection,
    /// Bounded truthful message.
    pub message: String,
}

/// Validate and hash one workspace-relative `.gd` path: lexical
/// validation, containment verification, non-following metadata checks,
/// size bound, and a SHA-256 over the exact bytes. The returned target is
/// the exact digest binding for the prepared check and its approval.
pub fn validate_check_script(
    workspace_root: &Path,
    relative_path: &str,
) -> Result<ValidatedCheckScript, GodotCheckScriptFailure> {
    if relative_path.is_empty()
        || relative_path.len() > GODOT_LIMITS.max_res_reference_path_bytes
        || relative_path.contains('\0')
    {
        return Err(failure(
            GodotCheckScriptRejection::InvalidPath,
            "The script path is invalid.",
        ));
    }
    if !relative_path.to_lowercase().ends_with(".gd") {
        return Err(failure(
            GodotCheckScriptRejection::NotGd,
            "Only workspace-relative .gd script paths can be checked.",
        ));
    }
    let resolved = match resolve_workspace_path(workspace_root, relative_path)
    {
        Ok(resolved) => resolved,
        Err(rejection) => {
            return Err(map_rejection(rejection, relative_path));
        }
    };
    let metadata = match fs::symlink_metadata(&resolved.absolute_path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Err(missing(relative_path));
        }
    };
    if metadata.is_symlink() {
        return Err(failure(
            GodotCheckScriptRejection::Symlink,
            "Symbolic links are rejected; the script must be a regular file.",
        ));
    }
    if !metadata.is_file() {
        return Err(failure(
            GodotCheckScriptRejection::NotRegular,
            "The script path is not a regular file.",
        ));
    }
    if metadata.len() > GODOT_LIMITS.max_gdscript_file_bytes as u64 {
        return Err(failure(
            GodotCheckScriptRejection::TooLarge,
            format!(
                "The script exceeds the {}-byte GDScript file bound.",
                GODOT_LIMITS.max_gdscript_file_bytes
            ),
        ));
    }
    let bytes = match fs::read(&resolved.absolute_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Err(failure(
                GodotCheckScriptRejection::Unreadable,
                format!(
                    "The script {relative_path} could not be verified; it may have changed during inspection."
                ),
            ));
        }
    };
    Ok(ValidatedCheckScript {
        canonical_path: resolved.absolute_path,
        sha256: sha256_hex(&bytes),
        bytes: bytes.len() as u64,
    })
}

fn map_rejection(
    rejection: crate::workspace::resolve::PathRejection,
    relative_path: &str,
) -> GodotCheckScriptFailure {
    use crate::workspace::resolve::PathRejection as Rejection;
    match rejection {
        Rejection::Absolute => failure(
            GodotCheckScriptRejection::Absolute,
            "The script path must be workspace-relative, not absolute.",
        ),
        Rejection::OutsideWorkspace | Rejection::LinkEscape => failure(
            GodotCheckScriptRejection::Traversal,
            "The script path must not escape the workspace.",
        ),
        _ => missing(relative_path),
    }
}

fn missing(relative_path: &str) -> GodotCheckScriptFailure {
    failure(
        GodotCheckScriptRejection::Missing,
        format!("The script {relative_path} does not exist in the workspace."),
    )
}

fn failure(
    reason: GodotCheckScriptRejection,
    message: impl Into<String>,
) -> GodotCheckScriptFailure {
    GodotCheckScriptFailure { reason, message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::{
        EnumerationLimits, GodotCheckScriptRejection,
        enumerate_gdscript_files_with_limits, validate_check_script,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_root() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let id = format!(
            "siralos-scriptenum-{}-{}",
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(id);
        fs::create_dir_all(&root).expect("mkdir");
        root
    }

    #[test]
    fn enumerates_sorted_targets_and_skips_excluded() {
        let root = unique_root();
        fs::write(root.join("b.gd"), "extends Node\n").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("a.gd"), "tool\n").unwrap();
        fs::write(root.join("sub").join("readme.txt"), "nope").unwrap();
        fs::create_dir_all(root.join(".godot")).unwrap();
        fs::write(root.join(".godot").join("cache.gd"), "hidden").unwrap();
        fs::create_dir_all(root.join(".siralos-mutation-x")).unwrap();
        fs::write(
            root.join(".siralos-mutation-x").join("staged.gd"),
            "staged",
        )
        .unwrap();
        let result = enumerate_gdscript_files_with_limits(
            &root,
            EnumerationLimits::default(),
        );
        let paths: Vec<&str> =
            result.targets.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(paths, ["b.gd", "sub/a.gd"]);
        assert!(!result.truncated);
        assert_eq!(result.targets[0].bytes, "extends Node\n".len() as u64);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn count_bound_sets_truncation() {
        let root = unique_root();
        for index in 0..4 {
            fs::write(root.join(format!("{index}.gd")), format!("# {index}"))
                .unwrap();
        }
        let result = enumerate_gdscript_files_with_limits(
            &root,
            EnumerationLimits {
                max_files: 2,
                max_total_bytes: u64::MAX,
                max_entries: usize::MAX,
                max_depth: usize::MAX,
            },
        );
        assert_eq!(result.targets.len(), 2);
        assert!(result.truncated);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn depth_bound_sets_truncation() {
        let root = unique_root();
        fs::create_dir_all(root.join("l1/l2")).unwrap();
        fs::write(root.join("l1").join("l2").join("deep.gd"), "#").unwrap();
        let result = enumerate_gdscript_files_with_limits(
            &root,
            EnumerationLimits {
                max_files: usize::MAX,
                max_total_bytes: u64::MAX,
                max_entries: usize::MAX,
                max_depth: 0,
            },
        );
        assert!(result.truncated);
        assert_eq!(result.targets.len(), 0);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn validates_existing_script_with_hash() {
        let root = unique_root();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src").join("player.gd"), "extends Node2D\n")
            .unwrap();
        let validated =
            validate_check_script(&root, "src/player.gd").expect("valid");
        assert_eq!(validated.bytes, "extends Node2D\n".len() as u64);
        assert_eq!(validated.sha256.len(), 64);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_traversal_extension_and_missing() {
        let root = unique_root();
        fs::write(root.join("ok.gd"), "#").unwrap();
        let traversal = validate_check_script(&root, "../escape.gd")
            .expect_err("rejected");
        assert_eq!(traversal.reason, GodotCheckScriptRejection::Traversal);
        let not_gd = validate_check_script(&root, "ok.txt").err().unwrap();
        assert_eq!(not_gd.reason, GodotCheckScriptRejection::NotGd);
        let missing = validate_check_script(&root, "ghost.gd").err().unwrap();
        assert_eq!(missing.reason, GodotCheckScriptRejection::Missing);
        assert_eq!(
            missing.message,
            "The script ghost.gd does not exist in the workspace."
        );
        fs::remove_dir_all(&root).ok();
    }
}
