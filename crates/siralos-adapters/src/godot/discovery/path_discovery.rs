//! Conservative fixed-name PATH discovery (R8-6b, TS oracle
//! `packages/adapters/src/godot/discovery/path-discovery.ts`).

use std::collections::HashSet;
use std::path::Path;

use siralos_core::godot::installations::{
    GodotEditionHint, GodotInstallation, GodotInstallationSource,
};
use siralos_core::godot::limits::GODOT_LIMITS;

use super::candidate_names::{godot_candidate_names, path_list_separator};
use super::executable_validation::{
    ExecutableIdentity, ValidateExecutableOptions, validate_executable,
};

/// Options for `discover_on_path`.
#[derive(Debug, Clone)]
pub struct PathDiscoveryOptions {
    /// Sanitized host PATH (or `None`/empty for no entries).
    pub host_path: Option<String>,
    /// Sanitized host PATHEXT (Windows only).
    pub host_path_ext: Option<String>,
    /// Node-style platform string (`"win32"` for Windows).
    pub platform: String,
    /// Canonical workspace root (for containment rejection).
    pub workspace_root: String,
}

/// Discover Godot executables on PATH (fixed names only, no shell).
///
/// Returns `(installations, truncated)` with candidates sorted by
/// `id` and bounded by `GODOT_LIMITS.max_candidates` (16).
#[must_use]
pub fn discover_on_path(
    options: PathDiscoveryOptions,
) -> (Vec<GodotInstallation>, bool) {
    let entries = split_path(options.host_path.as_deref(), &options.platform);
    let names = godot_candidate_names(&options.platform);
    let path_ext =
        parse_path_ext(options.host_path_ext.as_deref(), &options.platform);

    let mut seen: HashSet<String> = HashSet::new();
    let mut candidates: Vec<GodotInstallation> = Vec::new();
    let mut truncated = false;
    let mut index: usize = 0;

    'outer: for directory in &entries {
        for name in &names {
            let variants = apply_path_ext(name, &path_ext, &options.platform);
            for variant in variants {
                if candidates.len() >= GODOT_LIMITS.max_candidates {
                    truncated = true;
                    break 'outer;
                }
                let candidate_path = Path::new(directory).join(&variant);

                // Canonicalize the candidate without shell execution.
                let canonical = match std::fs::canonicalize(&candidate_path) {
                    Ok(path) => path,
                    Err(_) => continue,
                };
                let canonical_str = canonical.to_string_lossy().into_owned();
                let folded =
                    fold_for_dedupe(&canonical_str, &options.platform);
                if seen.contains(&folded) {
                    continue;
                }
                if candidates.len() >= GODOT_LIMITS.max_candidates {
                    truncated = true;
                    break 'outer;
                }
                seen.insert(folded);

                let validated =
                    validate_executable(ValidateExecutableOptions {
                        path: candidate_path.to_string_lossy().into_owned(),
                        workspace_root: options.workspace_root.clone(),
                        max_bytes: None,
                    });

                index += 1;
                let id = format!("path-{index}");
                let installation = match validated {
                    Ok(identity) => installation_from_identity(
                        id,
                        GodotInstallationSource::Path,
                        "PATH",
                        &identity,
                        GodotEditionHint::Unknown,
                    ),
                    Err(error) => invalid_installation(
                        id,
                        GodotInstallationSource::Path,
                        "PATH",
                        error,
                    ),
                };
                candidates.push(installation);
                if candidates.len() >= GODOT_LIMITS.max_candidates {
                    // Peek ahead: if more would have been found, mark truncated.
                    // Simpler: if we filled the bound but there are still
                    // entries/variants remaining, caller is told truncated.
                    // We set truncated when we would exceed on the next hit;
                    // the current length check already caps output.
                }
            }
        }
    }

    // Truncated when the loop would have produced more than max_candidates.
    // Re-scan cheaply to detect overflow only when we hit the bound.
    if candidates.len() >= GODOT_LIMITS.max_candidates {
        // Determine whether more eligible canonicals exist beyond the bound.
        // Bounded scan: re-iterate until one unseen canonical is found.
        let mut extra_found = false;
        let mut seen_for_check = seen;
        let mut emitted = 0usize;
        'check: for directory in &entries {
            for name in &names {
                let variants =
                    apply_path_ext(name, &path_ext, &options.platform);
                for variant in variants {
                    let candidate_path = Path::new(directory).join(&variant);
                    let Ok(canonical) = std::fs::canonicalize(&candidate_path)
                    else {
                        continue;
                    };
                    let folded = fold_for_dedupe(
                        &canonical.to_string_lossy(),
                        &options.platform,
                    );
                    if seen_for_check.contains(&folded) {
                        continue;
                    }
                    seen_for_check.insert(folded);
                    emitted += 1;
                    // First emitted beyond already-counted outputs hits truncation.
                    if emitted + candidates.len() > GODOT_LIMITS.max_candidates
                    {
                        extra_found = true;
                        break 'check;
                    }
                    // Cap the check scan to avoid unbounded I/O.
                    if emitted > GODOT_LIMITS.max_candidates {
                        break 'check;
                    }
                }
            }
        }
        if extra_found {
            truncated = true;
        }
    }

    candidates.sort_by(|left, right| left.id.cmp(&right.id));
    (candidates, truncated)
}

pub(crate) fn installation_from_identity(
    id: String,
    source: GodotInstallationSource,
    source_label: &str,
    identity: &ExecutableIdentity,
    edition_hint: GodotEditionHint,
) -> GodotInstallation {
    GodotInstallation {
        id,
        source_label: source_label.to_owned(),
        source,
        canonical_path: identity.canonical_path.clone(),
        size_bytes: identity.size_bytes,
        modified_at_ms: identity.modified_at_ms,
        sha256: identity.sha256.clone(),
        edition_hint,
        status_valid: true,
        error: None,
    }
}

pub(crate) fn invalid_installation(
    id: String,
    source: GodotInstallationSource,
    source_label: &str,
    error: String,
) -> GodotInstallation {
    GodotInstallation {
        id,
        source_label: source_label.to_owned(),
        source,
        canonical_path: String::new(),
        size_bytes: 0,
        modified_at_ms: 0,
        sha256: String::new(),
        edition_hint: GodotEditionHint::Unknown,
        status_valid: false,
        error: Some(error),
    }
}

fn split_path(host_path: Option<&str>, platform: &str) -> Vec<String> {
    let Some(raw) = host_path else {
        return Vec::new();
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let separator = path_list_separator(platform);
    let mut entries: Vec<String> = raw
        .split(separator)
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| entry.to_owned())
        .collect();
    // Dedup entries preserving order (like [...new Set(entries)]).
    let mut seen = HashSet::new();
    entries.retain(|entry| seen.insert(entry.clone()));
    entries
}

fn parse_path_ext(host_path_ext: Option<&str>, platform: &str) -> Vec<String> {
    if platform != "win32" {
        return Vec::new();
    }
    let Some(raw) = host_path_ext else {
        return Vec::new();
    };
    let mut extensions: Vec<String> = raw
        .split(';')
        .map(|extension| extension.trim().to_ascii_lowercase())
        .filter(|extension| extension.starts_with('.'))
        .collect();
    let mut seen = HashSet::new();
    extensions.retain(|extension| seen.insert(extension.clone()));
    extensions
}

fn apply_path_ext(
    name: &str,
    path_ext: &[String],
    platform: &str,
) -> Vec<String> {
    if platform != "win32" || path_ext.is_empty() {
        return vec![name.to_owned()];
    }
    let has_extension = name.rfind('.').is_some_and(|index| {
        let after = &name[index + 1..];
        !after.is_empty()
            && after.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '_'
            })
    });
    if has_extension {
        if name.to_ascii_lowercase().ends_with(".exe") {
            return vec![name.to_owned()];
        }
        return Vec::new();
    }
    let mut variants = vec![name.to_owned()];
    for extension in path_ext {
        if extension == ".exe" {
            variants.push(format!("{name}{extension}"));
        }
    }
    // Dedup like [...new Set(variants)].
    let mut seen = HashSet::new();
    variants.retain(|variant| seen.insert(variant.clone()));
    variants
}

fn fold_for_dedupe(path: &str, platform: &str) -> String {
    if platform == "win32" || platform == "darwin" {
        path.to_ascii_lowercase()
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_path_ext, fold_for_dedupe, parse_path_ext, split_path};

    #[test]
    fn split_path_dedups_and_trims() {
        let entries = split_path(Some("a:b::a"), "linux");
        assert_eq!(entries, vec!["a", "b"]);
        assert!(split_path(None, "linux").is_empty());
        assert!(split_path(Some("   "), "linux").is_empty());
    }

    #[test]
    fn parse_path_ext_only_on_win32() {
        assert!(parse_path_ext(Some(".EXE;.BAT"), "linux").is_empty());
        let parsed = parse_path_ext(Some(".EXE;.BAT;.CMD"), "win32");
        assert_eq!(parsed, vec![".exe", ".bat", ".cmd"]);
    }

    #[test]
    fn apply_path_ext_safe() {
        let path_ext = vec![".exe".to_owned(), ".bat".to_owned()];
        // Name without extension gets .exe variant.
        assert_eq!(
            apply_path_ext("godot", &path_ext, "win32"),
            vec!["godot", "godot.exe"]
        );
        // Name with .exe kept.
        assert_eq!(
            apply_path_ext("godot.exe", &path_ext, "win32"),
            vec!["godot.exe"]
        );
        // Non-exe extension dropped.
        assert!(apply_path_ext("godot.bat", &path_ext, "win32").is_empty());
        // Posix ignores PATHEXT.
        assert_eq!(apply_path_ext("godot", &path_ext, "linux"), vec!["godot"]);
    }

    #[test]
    fn fold_for_dedupe_case_on_win32_and_darwin() {
        assert_eq!(fold_for_dedupe("/A/B", "win32"), "/a/b");
        assert_eq!(fold_for_dedupe("/A/B", "darwin"), "/a/b");
        assert_eq!(fold_for_dedupe("/A/B", "linux"), "/A/B");
    }
}
