//! PATH resolution and the package.json version gate for the
//! pinned Sandbox Runtime distribution (`resolution`): pure
//! filesystem reads, no spawning, no writes.

use std::path::{Path, PathBuf};

/// Probe evidence: the runtime is the pinned distribution.
const SRT_PACKAGE_JSON_NAME: &str = "package.json";

/// Search `PATH` for one executable file, canonicalized. Symlinks
/// resolve here so the version gate reads the real distribution.
/// Pure filesystem reads; nothing spawns.
pub(super) fn resolve_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_var) {
        let candidate = directory.join(name);
        if let Ok(metadata) = std::fs::symlink_metadata(&candidate) {
            if metadata.file_type().is_file()
                && let Ok(canonical) = std::fs::canonicalize(&candidate)
            {
                return Some(canonical);
            }
        }
    }
    None
}

/// Read the distribution version beside a resolved binary: walk up
/// from the canonical binary path to the first `package.json` and
/// require it to name the pinned distribution. Returns the version
/// string (the caller compares to the pin) or `None` when the
/// layout is unexpected. Pure filesystem reads; nothing spawns.
pub(super) fn read_srt_version(binary: &Path) -> Option<String> {
    let canonical =
        std::fs::canonicalize(binary).unwrap_or_else(|_| binary.into());
    for ancestor in canonical.ancestors().skip(1) {
        let manifest = ancestor.join(SRT_PACKAGE_JSON_NAME);
        // A missing manifest is not the distribution root: keep
        // climbing. A manifest that parses but names another
        // package owns this path, so stop instead of rummaging
        // further up.
        let Ok(bytes) = std::fs::read(&manifest) else { continue };
        let Ok(parsed): Result<serde_json::Value, _> =
            serde_json::from_slice(&bytes)
        else {
            return None;
        };
        let name = parsed.get("name").and_then(|n| n.as_str())?;
        if name != super::PINNED_SRT_DISTRIBUTION {
            return None;
        }
        return parsed
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
    }
    None
}
