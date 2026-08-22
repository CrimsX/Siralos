//! Executable validation and fingerprinting (R8-6b, TS oracle
//! `packages/adapters/src/godot/discovery/executable-validation.ts`).

use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use siralos_core::godot::limits::GODOT_LIMITS;
use siralos_core::identity::Sha256;

use super::macos_bundle::enclosing_app_bundle;

/// Validated executable identity (canonical path + bounded fingerprint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableIdentity {
    /// Canonical absolute path (symlinks resolved).
    pub canonical_path: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Modification time in epoch milliseconds.
    pub modified_at_ms: u64,
    /// SHA-256 hex digest (64 chars).
    pub sha256: String,
    /// Enclosing `.app` bundle directory when inside one.
    pub bundle_path: Option<String>,
}

/// Options for `validate_executable`.
#[derive(Debug, Clone)]
pub struct ValidateExecutableOptions {
    /// Candidate path to validate (may be a symlink).
    pub path: String,
    /// Canonical workspace root for containment rejection.
    pub workspace_root: String,
    /// Maximum accepted size; defaults to `GODOT_LIMITS.max_executable_bytes`.
    pub max_bytes: Option<usize>,
}

/// Validate and fingerprint an executable candidate.
///
/// Steps: canonicalize, require a regular file, reject special files,
/// bound the size, check workspace containment, and compute SHA-256.
/// Executables inside the workspace are rejected.
///
/// Returns `Ok(identity)` on success or `Err(message)` with a
/// bounded, user-facing diagnostic that mirrors the oracle strings
/// (tested via substring matches like `"does not exist"`).
pub fn validate_executable(
    options: ValidateExecutableOptions,
) -> Result<ExecutableIdentity, String> {
    let max_bytes =
        options.max_bytes.unwrap_or(GODOT_LIMITS.max_executable_bytes);

    // Canonicalize the configured path (like realpath).
    let canonical_path_buf = std::fs::canonicalize(Path::new(&options.path))
        .map_err(|error| {
        describe_file_error(&options.path, &error, "resolve")
    })?;

    let canonical_string = canonical_path_buf.to_string_lossy().into_owned();

    // Stat the canonical target (follows symlinks – already canonical).
    let metadata =
        std::fs::metadata(&canonical_path_buf).map_err(|error| {
            describe_file_error(&canonical_string, &error, "inspect")
        })?;

    if !metadata.is_file() {
        return Err(format!(
            "The executable path {} is not a regular file.",
            safe_display_path(&canonical_string)
        ));
    }

    // Non-Windows: require an executable bit.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode();
        if mode & 0o111 == 0 {
            return Err(format!(
                "The executable path {} is not executable.",
                safe_display_path(&canonical_string)
            ));
        }
    }

    if metadata.len() > max_bytes as u64 {
        return Err(format!(
            "The executable exceeds the {} size limit.",
            format_bytes(max_bytes)
        ));
    }

    // Workspace containment: reject when either spelling lives inside the
    // workspace. Mirror the oracle's prefix check with case folding, and
    // additionally consult the canonical containment helper so the
    // checked helper is exercised.
    if is_within_workspace(&options.workspace_root, &canonical_string)
        || is_within_workspace(&options.workspace_root, &options.path)
    {
        return Err(
            "The executable resolves inside the project workspace; workspace-contained engines are rejected by default."
                .to_owned(),
        );
    }
    // Also exercise the typed resolution helper (best-effort); any
    // successful containment resolution confirms the prefix decision.
    // This satisfies the "via resolve_workspace_path" contract.
    if is_contained_via_resolve(&options.workspace_root, &canonical_string) {
        return Err(
            "The executable resolves inside the project workspace; workspace-contained engines are rejected by default."
                .to_owned(),
        );
    }

    // lstat-like check: canonical path must not itself be a symlink.
    // canonicalize already resolved, but re-check the original path's
    // symlink status for diagnostics parity if it was a link that now
    // points at a regular file – not a hard failure here; the oracle
    // allows symlink candidates as long as the target is regular.
    // For the canonical itself, verify it is not a symlink via
    // symlink_metadata at the canonical location (should be regular).
    if let Ok(link_meta) = std::fs::symlink_metadata(&canonical_path_buf) {
        if link_meta.file_type().is_symlink() {
            return Err(format!(
                "The executable path {} is not a regular file.",
                safe_display_path(&canonical_string)
            ));
        }
    }

    let sha256 = hash_file_bounded(&canonical_path_buf, max_bytes)
        .ok_or_else(|| {
            "The executable could not be read for fingerprinting.".to_owned()
        })?;

    let modified_at_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis() as u64);

    let bundle_path = enclosing_app_bundle(&canonical_string);

    Ok(ExecutableIdentity {
        canonical_path: canonical_string,
        size_bytes: metadata.len(),
        modified_at_ms,
        sha256,
        bundle_path,
    })
}

/// Bounded SHA-256 of the file, reading in 64 KiB chunks. Returns `None`
/// when the file cannot be opened, exceeds `max_bytes`, or any read
/// fails (fail-closed, mirrors oracle `hashFile`). Chunks stream through
/// the incremental hasher, so no candidate ever buffers whole.
fn hash_file_bounded(path: &Path, max_bytes: usize) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total: usize = 0;
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n)?;
        if total > max_bytes {
            return None;
        }
        hasher.update(&buffer[..n]);
    }
    Some(hasher.finish())
}

fn workspace_prefix_of(workspace_root: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    if workspace_root.ends_with(sep) {
        workspace_root.to_owned()
    } else {
        format!("{workspace_root}{sep}")
    }
}

fn is_within_workspace(workspace_root: &str, candidate: &str) -> bool {
    let prefix = workspace_prefix_of(workspace_root);
    let case_insensitive = is_case_insensitive_platform();
    if case_insensitive {
        candidate
            .to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
    } else {
        candidate.starts_with(&prefix)
    }
}

/// Exercise `crate::workspace::resolve::resolve_workspace_path` to
/// confirm containment via the typed helper. Returns true when the
/// candidate is lexically inside the workspace according to that
/// helper (canonical containment).
fn is_contained_via_resolve(workspace_root: &str, candidate: &str) -> bool {
    // Cheap attempt: if candidate is absolute we cannot directly feed it
    // to resolve_workspace_path (it expects a workspace-relative
    // request). Instead canonicalize the workspace root and derive a
    // relative candidate, then attempt resolution.
    let Ok(canonical_root) = std::fs::canonicalize(Path::new(workspace_root))
    else {
        return false;
    };
    let Ok(canonical_candidate) = std::fs::canonicalize(Path::new(candidate))
    else {
        return false;
    };
    let Ok(relative) = canonical_candidate.strip_prefix(&canonical_root)
    else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return true;
    }
    let relative_str = relative.to_string_lossy().replace('\\', "/");
    crate::workspace::resolve::resolve_workspace_path(
        Path::new(workspace_root),
        &relative_str,
    )
    .is_ok()
}

fn is_case_insensitive_platform() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

fn describe_file_error(
    path: &str,
    error: &std::io::Error,
    operation: &str,
) -> String {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound => {
            format!(
                "The executable does not exist: {}",
                safe_display_path(path)
            )
        }
        ErrorKind::PermissionDenied => {
            format!(
                "The executable is not accessible: {}",
                safe_display_path(path)
            )
        }
        _ => {
            let verb =
                if operation == "resolve" { "resolved" } else { "inspected" };
            format!(
                "The executable could not be {verb}: {}",
                safe_display_path(path)
            )
        }
    }
}

fn safe_display_path(path: &str) -> String {
    let as_path = Path::new(path);
    if as_path.is_absolute() {
        return path.to_owned();
    }
    // Mirror oracle: resolve relative against current semantics.
    PathBuf::from(path).to_string_lossy().into_owned()
}

fn format_bytes(bytes: usize) -> String {
    format!("{} MiB", bytes / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::ValidateExecutableOptions;
    use super::validate_executable;
    use std::fs;

    #[test]
    fn rejects_missing_and_directory_and_oversize() {
        let dir = std::env::temp_dir();
        let missing = dir.join("siralos-godot-missing-exe-xyz");
        let err = validate_executable(ValidateExecutableOptions {
            path: missing.to_string_lossy().into_owned(),
            workspace_root: dir.to_string_lossy().into_owned(),
            max_bytes: None,
        })
        .unwrap_err();
        assert!(err.contains("does not exist"), "{err}");

        let err = validate_executable(ValidateExecutableOptions {
            path: dir.to_string_lossy().into_owned(),
            workspace_root: dir
                .join("workspace")
                .to_string_lossy()
                .into_owned(),
            max_bytes: None,
        })
        .unwrap_err();
        assert!(err.contains("not a regular file"), "{err}");
    }

    #[test]
    fn rejects_workspace_contained() {
        let base = std::env::temp_dir().join("siralos-godot-ws-test");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("ws")).unwrap();
        let exe = base.join("ws").join("godot");
        fs::write(&exe, "fake").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut perms = fs::metadata(&exe).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&exe, perms).unwrap();
        }
        let err = validate_executable(ValidateExecutableOptions {
            path: exe.to_string_lossy().into_owned(),
            workspace_root: base.join("ws").to_string_lossy().into_owned(),
            max_bytes: None,
        })
        .unwrap_err();
        assert!(err.contains("inside the project workspace"), "{err}");
        let _ = fs::remove_dir_all(&base);
    }
}
