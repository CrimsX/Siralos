//! Root-only project.godot detection (R8).
//!
//! Mirrors packages/adapters/src/godot/project/project-files.ts.
//! Only a regular, non-symlinked project.godot at the workspace root
//! is detected. Bounded read is verified canonically.

use std::path::{Path, PathBuf};

use siralos_core::identity::sha256_hex;
use siralos_godot::godot::GODOT_LIMITS;

use crate::workspace::fs::{BoundedFileRead, read_complete_file_bounded};
use crate::workspace::resolve::resolve_workspace_path;

/// Root-only project.godot detection.
///
/// Returns the file content plus sha256 when present, or a typed
/// failure. Uses resolve_workspace_path plus symlink_metadata plus
/// read_complete_file_bounded with GODOT_LIMITS.max_project_file_bytes
/// and re-verifies canonical identity after the read.
pub fn read_project_file(
    workspace_root: &Path,
) -> Result<(String, String), String> {
    let canonical_root = std::fs::canonicalize(workspace_root)
        .map_err(|e| format!("Workspace root is not accessible: {e}"))?;

    let resolved = resolve_workspace_path(&canonical_root, "project.godot")
        .map_err(|_| {
            "No project.godot exists at the workspace root.".to_owned()
        })?;

    let absolute: PathBuf = resolved.absolute_path.clone();

    let metadata = std::fs::symlink_metadata(&absolute).map_err(|_| {
        "No project.godot exists at the workspace root.".to_owned()
    })?;

    if metadata.file_type().is_symlink() {
        return Err(
            "project.godot must be a regular file; symbolic links are rejected.".to_owned(),
        );
    }
    if !metadata.is_file() {
        return Err("project.godot is not a regular file.".to_owned());
    }
    if metadata.len() > GODOT_LIMITS.max_project_file_bytes as u64 {
        return Err(format!(
            "project.godot exceeds the {} MiB limit.",
            GODOT_LIMITS.max_project_file_bytes / (1024 * 1024)
        ));
    }

    let canonical_before = std::fs::canonicalize(&absolute).map_err(|e| {
        format!("Failed to canonicalize project.godot before read: {e}")
    })?;

    let bytes = match read_complete_file_bounded(
        &absolute,
        GODOT_LIMITS.max_project_file_bytes,
    ) {
        BoundedFileRead::Complete(b) => b,
        BoundedFileRead::TooLarge => {
            return Err(format!(
                "project.godot exceeds the {} MiB limit.",
                GODOT_LIMITS.max_project_file_bytes / (1024 * 1024)
            ));
        }
        BoundedFileRead::NotReadable => {
            return Err(
                "No project.godot exists at the workspace root.".to_owned()
            );
        }
        BoundedFileRead::IoError(e) => {
            return Err(format!("Failed to read project.godot: {e}"));
        }
    };

    let canonical_after = std::fs::canonicalize(&absolute).map_err(|_| {
        "project.godot changed during inspection; the read was rejected."
            .to_owned()
    })?;
    if canonical_after != canonical_before {
        return Err(
            "project.godot changed during inspection; the read was rejected."
                .to_owned(),
        );
    }

    let sha256 = sha256_hex(&bytes);
    let content = String::from_utf8(bytes)
        .map_err(|_| "project.godot is not valid UTF-8.".to_owned())?;

    Ok((content, sha256))
}
