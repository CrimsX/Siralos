//! macOS bundle discovery (R8-6b, TS oracle
//! `packages/adapters/src/godot/discovery/macos-bundle.ts`).

use std::path::{Path, PathBuf};

/// Returns the enclosing `.app` bundle directory when `path` lives
/// inside one, walking from the executable's parent toward the root.
/// The first ancestor whose name ends in `.app` (case-insensitively) is
/// the bundle. Returns `None` when no enclosing bundle exists.
#[must_use]
pub fn enclosing_app_bundle(path: &str) -> Option<String> {
    let mut current = Path::new(path).parent()?.to_path_buf();
    for _ in 0..64 {
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current.as_path() {
            break;
        }
        if current
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_lowercase().ends_with(".app"))
        {
            return Some(current.to_string_lossy().into_owned());
        }
        current = parent.to_path_buf();
    }
    None
}

/// Resolves the exact executable of a macOS Godot application bundle.
///
/// A bundle may be configured as `/path/Godot.app` or directly as
/// `/path/Godot.app/Contents/MacOS/Godot`. Bundles are never launched
/// through `open` and never use Apple Events: only the exact
/// executable is returned for direct execution.
pub fn resolve_macos_bundle(path: &Path) -> Result<PathBuf, String> {
    let is_app = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_ascii_lowercase().ends_with(".app"));
    if !is_app {
        return Err("The configured path is not an .app bundle.".to_owned());
    }
    let contents = path.join("Contents");
    let macos_directory = contents.join("MacOS");
    let contents_metadata = std::fs::metadata(&contents)
        .map_err(|_| "The bundle has no Contents directory.".to_owned())?;
    if !contents_metadata.is_dir() {
        return Err("The bundle Contents path is not a directory.".to_owned());
    }
    let macos_metadata =
        std::fs::metadata(&macos_directory).map_err(|_| {
            "The bundle has no Contents/MacOS directory.".to_owned()
        })?;
    if !macos_metadata.is_dir() {
        return Err(
            "The bundle Contents/MacOS path is not a directory.".to_owned()
        );
    }
    let executable_name = read_bundle_executable_name(&contents);
    let executable_path = macos_directory.join(&executable_name);
    let executable_metadata = std::fs::metadata(&executable_path)
        .map_err(|_| {
            format!(
                "The bundle executable {executable_name} does not exist in Contents/MacOS."
            )
        })?;
    if !executable_metadata.is_file() {
        return Err(format!(
            "The bundle executable {executable_name} is not a regular file."
        ));
    }
    Ok(executable_path)
}

const BUNDLE_EXECUTABLE_NAME_LIMIT: usize = 64 * 1024;

/// Reads `CFBundleExecutable` from an XML Info.plist. Binary plists are
/// not decoded; the conventional `Godot` name is used as a fallback.
/// Plist content is untrusted and only scanned textually, bounded to
/// 64 KiB.
fn read_bundle_executable_name(contents_directory: &Path) -> String {
    let plist_path = contents_directory.join("Info.plist");
    let bytes = match read_plist_prefix(&plist_path) {
        Some(bytes) => bytes,
        None => return "Godot".to_owned(),
    };
    let content = String::from_utf8_lossy(&bytes);
    let pattern = "<key>CFBundleExecutable</key>";
    let Some(key_index) = content.find(pattern) else {
        return "Godot".to_owned();
    };
    let after_key = &content[key_index + pattern.len()..];
    // Find <string>...</string> after the key, allowing whitespace/newlines.
    let Some(string_start) = after_key.find("<string>") else {
        return "Godot".to_owned();
    };
    let after_open = &after_key[string_start + "<string>".len()..];
    let Some(string_end) = after_open.find("</string>") else {
        return "Godot".to_owned();
    };
    let name = after_open[..string_end].trim().to_owned();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return "Godot".to_owned();
    }
    name
}

fn read_plist_prefix(path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    use std::io::Read as _;
    let mut reader = file;
    let mut buffer = Vec::new();
    let limit = BUNDLE_EXECUTABLE_NAME_LIMIT as u64 + 1;
    if reader.by_ref().take(limit).read_to_end(&mut buffer).is_err() {
        return None;
    }
    if buffer.len() > BUNDLE_EXECUTABLE_NAME_LIMIT {
        buffer.truncate(BUNDLE_EXECUTABLE_NAME_LIMIT);
    }
    Some(buffer)
}

#[cfg(test)]
mod tests {
    use super::{enclosing_app_bundle, resolve_macos_bundle};
    use std::path::Path;

    #[test]
    fn enclosing_bundle_found_case_insensitively() {
        assert_eq!(
            enclosing_app_bundle("/a/Godot.APP/Contents/MacOS/Godot"),
            Some("/a/Godot.APP".to_owned())
        );
        assert_eq!(
            enclosing_app_bundle("/a/b/Godot.app/Contents/MacOS/foo"),
            Some("/a/b/Godot.app".to_owned())
        );
    }

    #[test]
    fn enclosing_bundle_none_for_standalone() {
        assert_eq!(enclosing_app_bundle("/a/b/godot"), None);
    }

    #[test]
    fn resolve_bundle_rejects_non_app() {
        assert!(resolve_macos_bundle(Path::new("/a/Godot")).is_err());
    }
}
