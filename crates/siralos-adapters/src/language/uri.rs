//! Generic language-service URI mapping (Stage 3R R5).
//!
//! The language-service boundary maps service URIs to workspace-relative
//! paths: `file://` URIs under the served workspace root become
//! workspace-relative paths with `/` separators; out-of-root URIs are
//! rejected (never guessed); decoded `..` segments can never escape the
//! root; and results never expose the absolute root. The semantics are
//! byte-identical to the reference `mirrorUriToWorkspaceRelative`
//! (the Godot adapter consumes the same semantics through its mirror
//! naming). Native path separators are normalized at this boundary so
//! they never alter language-result semantic identity.

/// Normalize a native path the way the reference does: separator runs
/// collapse to the host separator and one trailing separator is
/// stripped.
fn normalize_path(value: &str) -> String {
    let sep = std::path::MAIN_SEPARATOR;
    let mut out = String::with_capacity(value.len());
    let mut previous_separator = false;
    for character in value.chars() {
        if character == '/' || character == '\\' {
            if !previous_separator {
                out.push(sep);
                previous_separator = true;
            }
        } else {
            out.push(character);
            previous_separator = false;
        }
    }
    if out.ends_with(sep) {
        out.pop();
    }
    out
}

/// Percent-decode like `decodeURIComponent`: every percent sequence is
/// decoded, the result must be valid UTF-8, and malformed input is
/// rejected.
fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Convert a `file://` URI to an absolute native path, or None when
/// unsafe (non-file scheme, host authority, malformed percent
/// encoding, or invalid UTF-8).
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    // A non-empty authority (file://host/path) is rejected: only the
    // local machine's paths are meaningful here.
    let authority_end = rest.find('/')?;
    let authority = &rest[..authority_end];
    if !authority.is_empty() && authority != "localhost" {
        return None;
    }
    let path_text = &rest[authority_end..];
    let decoded = decode_uri_component(path_text)?;
    // Windows drive URIs: file:///C:/dir/file.gd
    if decoded.len() >= 3
        && decoded.as_bytes()[0] == b'/'
        && decoded.as_bytes()[1].is_ascii_alphabetic()
        && decoded.as_bytes()[2] == b':'
        && decoded.as_bytes().get(3) == Some(&b'/')
    {
        return Some(decoded[1..].replace('/', "\\"));
    }
    Some(decoded)
}

/// True when a decoded relative path contains an escaping `..`
/// segment (mirroring the reference rejection).
fn contains_escaping_segment(relative: &str) -> bool {
    relative.split(['\\', '/']).any(|segment| segment == "..")
}

/// Map a service URI to a workspace-relative path with `/` separators,
/// or None when the URI is not under the workspace root or cannot be
/// decoded safely (out-of-workspace URIs are rejected, never guessed).
pub fn map_uri_to_workspace_relative(
    uri: &str,
    workspace_root: &str,
) -> Option<String> {
    let absolute = file_uri_to_path(uri)?;
    let root = normalize_path(workspace_root);
    let normalized = normalize_path(&absolute);
    if normalized == root {
        return None;
    }
    let mut prefix = root;
    prefix.push(std::path::MAIN_SEPARATOR);
    if !normalized.starts_with(&prefix) {
        return None;
    }
    let relative = &normalized[prefix.len()..];
    if relative.is_empty() {
        return None;
    }
    // Decoded `..` segments must never escape the workspace root: the
    // decoded path is checked after percent-decoding, so
    // file:///root/../secret.gd is rejected here rather than normalized
    // away.
    if contains_escaping_segment(relative) {
        return None;
    }
    Some(relative.replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_in_root_uris_to_relative_paths() {
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project/scripts/player.gd",
                "/work/project",
            ),
            Some("scripts/player.gd".to_owned()),
        );
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project/a.gd",
                "/work/project",
            ),
            Some("a.gd".to_owned()),
        );
    }

    #[test]
    fn rejects_out_of_root_and_root_itself() {
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///elsewhere/engine.gd",
                "/work/project",
            ),
            None,
        );
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project",
                "/work/project",
            ),
            None,
        );
    }

    #[test]
    fn decodes_percent_encoding() {
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project/my%20file.gd",
                "/work/project",
            ),
            Some("my file.gd".to_owned()),
        );
        // Malformed percent encoding is rejected.
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project/%zz.gd",
                "/work/project",
            ),
            None,
        );
        // Invalid UTF-8 percent sequences are rejected.
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project/%FF.gd",
                "/work/project",
            ),
            None,
        );
    }

    #[test]
    fn rejects_escaping_segments_and_hosts() {
        assert_eq!(
            map_uri_to_workspace_relative(
                "file:///work/project/../secret.gd",
                "/work/project",
            ),
            None,
        );
        assert_eq!(
            map_uri_to_workspace_relative(
                "file://evil/project/a.gd",
                "/work/project",
            ),
            None,
        );
        assert_eq!(
            map_uri_to_workspace_relative("https://x/y.gd", "/work/project"),
            None,
        );
    }

    #[test]
    fn normalizes_windows_drive_uris() {
        // The drive decoding is platform-independent, like the
        // reference.
        assert_eq!(
            file_uri_to_path("file:///C:/dir/file.gd"),
            Some("C:\\dir\\file.gd".to_owned()),
        );
    }
}
