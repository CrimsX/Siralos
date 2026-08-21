//! Robust file-URI conversion for the LSP boundary.
//!
//! The LSP adapter translates workspace-relative paths to mirror `file://`
//! URIs and back. Windows drive paths, spaces, Unicode, percent encoding,
//! and POSIX paths are handled explicitly; URIs with a host component or
//! a non-file scheme are rejected; and results never expose mirror
//! absolute paths to provider-facing models.

use std::path::MAIN_SEPARATOR;

/// Converts a `file://` URI to an absolute native path, or `None` when
/// unsafe.
#[must_use]
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
    let decoded = percent_decode(path_text)?;
    // Windows drive URIs: file:///C:/dir/file.gd
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && bytes.get(3) == Some(&b'/')
    {
        return Some(decoded[1..].replace('/', "\\"));
    }
    Some(decoded)
}

/// Converts an absolute native path to a `file://` URI.
#[must_use]
pub fn path_to_file_uri(absolute_path: &str) -> String {
    let normalized = absolute_path.replace('\\', "/");
    let with_scheme = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    };
    format!("file://{}", encode_uri_component_slash(&with_scheme))
}

/// Converts a mirror file URI to a workspace-relative path with `/`
/// separators. Returns `None` when the URI is not under the mirror root
/// or cannot be decoded safely; out-of-mirror URIs are rejected, never
/// guessed.
#[must_use]
pub fn mirror_uri_to_workspace_relative(
    uri: &str,
    mirror_root_path: &str,
) -> Option<String> {
    let absolute = file_uri_to_path(uri)?;
    let mirror_root = normalize_path(mirror_root_path);
    let normalized = normalize_path(&absolute);
    if normalized == mirror_root {
        return None;
    }
    let prefix = format!("{mirror_root}{MAIN_SEPARATOR}");
    if !normalized.starts_with(&prefix) {
        return None;
    }
    let relative = normalized[prefix.len()..].to_owned();
    if relative.is_empty() {
        return None;
    }
    // Decoded `..` segments must never escape the mirror root: the
    // decoded path is checked after percent-decoding, so
    // `file:///mirror/../secret.gd` is rejected here rather than
    // normalized away.
    if contains_escaping_segment(&relative) {
        return None;
    }
    Some(relative.replace('\\', "/"))
}

/// Converts a workspace-relative path to the mirror file URI, or `None`.
#[must_use]
pub fn workspace_relative_to_mirror_uri(
    relative_path: &str,
    mirror_root_path: &str,
) -> Option<String> {
    if relative_path.is_empty()
        || relative_path.starts_with('/')
        || is_drive_relative(relative_path)
    {
        return None;
    }
    if relative_path.contains("..") {
        return None;
    }
    let joined = format!(
        "{}{MAIN_SEPARATOR}{}",
        normalize_path(mirror_root_path),
        relative_path.replace('/', MAIN_SEPARATOR_STR)
    );
    Some(path_to_file_uri(&joined))
}

const MAIN_SEPARATOR_STR: &str =
    if MAIN_SEPARATOR == '\\' { "\\" } else { "/" };

fn is_drive_relative(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn contains_escaping_segment(relative_path: &str) -> bool {
    relative_path.split(['\\', '/']).any(|segment| segment == "..")
}

fn normalize_path(value: &str) -> String {
    let separator = MAIN_SEPARATOR_STR;
    let mut collapsed = String::with_capacity(value.len());
    let mut previous_separator = false;
    for character in value.chars() {
        if character == '/' || character == '\\' {
            if !previous_separator {
                collapsed.push_str(separator);
            }
            previous_separator = true;
        } else {
            collapsed.push(character);
            previous_separator = false;
        }
    }
    collapsed.strip_suffix(separator).unwrap_or(&collapsed).to_owned()
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex_high = *bytes.get(index + 1)?;
            let hex_low = *bytes.get(index + 2)?;
            let high = hex_value(hex_high)?;
            let low = hex_value(hex_low)?;
            out.push(high * 16 + low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// `encodeURI` semantics: unreserved ASCII and the reserved set pass
/// through, everything else (including every multi-byte UTF-8 byte) is
/// percent-encoded; the reference additionally forces `#` to `%23`.
fn encode_uri_component_slash(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        let keep = matches!(
            byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b';'
                | b','
                | b'/'
                | b'?'
                | b':'
                | b'@'
                | b'&'
                | b'='
                | b'+'
                | b'$'
                | b'-'
                | b'_'
                | b'.'
                | b'!'
                | b'~'
                | b'*'
                | b'\''
                | b'('
                | b')'
        );
        if keep && byte != b'#' {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        file_uri_to_path, mirror_uri_to_workspace_relative, path_to_file_uri,
        workspace_relative_to_mirror_uri,
    };

    #[test]
    fn decodes_local_file_uris() {
        // Non-drive paths keep their separators exactly as decoded.
        assert_eq!(
            file_uri_to_path("file:///mirror/src/player.gd").as_deref(),
            Some("/mirror/src/player.gd")
        );
        assert_eq!(
            file_uri_to_path("file://localhost/mirror/x.gd").as_deref(),
            Some("/mirror/x.gd")
        );
        assert_eq!(
            file_uri_to_path("file:///C:/dir/file.gd").as_deref(),
            Some("C:\\dir\\file.gd")
        );
    }

    #[test]
    fn rejects_hosts_schemes_and_malformed_escapes() {
        assert_eq!(file_uri_to_path("http://x/y.gd"), None);
        assert_eq!(file_uri_to_path("file://evil/x.gd"), None);
        assert_eq!(file_uri_to_path("file://"), None);
        assert_eq!(file_uri_to_path("file://%zz/x.gd"), None);
    }

    #[test]
    fn encodes_paths_and_round_trips() {
        let uri = path_to_file_uri("/mirror/my scripts/player.gd");
        assert_eq!(uri, "file:///mirror/my%20scripts/player.gd");
        assert_eq!(
            file_uri_to_path(&uri).as_deref(),
            Some("/mirror/my scripts/player.gd")
        );
        assert_eq!(path_to_file_uri("/a#b.gd"), "file:///a%23b.gd");
    }

    #[test]
    fn mirror_mapping_rejects_out_of_mirror_and_escaping() {
        if cfg!(windows) {
            assert_eq!(
                mirror_uri_to_workspace_relative(
                    "file:///C:/mirror/src/a.gd",
                    "C:\\mirror",
                ),
                Some("src/a.gd".to_owned())
            );
            assert_eq!(
                mirror_uri_to_workspace_relative(
                    "file:///C:/other/a.gd",
                    "C:\\mirror",
                ),
                None
            );
        } else {
            assert_eq!(
                mirror_uri_to_workspace_relative(
                    "file:///mirror/src/a.gd",
                    "/mirror",
                ),
                Some("src/a.gd".to_owned())
            );
            assert_eq!(
                mirror_uri_to_workspace_relative(
                    "file:///tmp/a.gd",
                    "/mirror"
                ),
                None
            );
        }
        assert_eq!(
            mirror_uri_to_workspace_relative(
                &path_to_file_uri("/mirror/../secret.gd"),
                if cfg!(windows) { "C:\\mirror" } else { "/mirror" },
            ),
            None
        );
    }

    #[test]
    fn relative_to_mirror_rejects_absolute_traversal_and_drive() {
        let mirror = if cfg!(windows) { "C:\\mirror" } else { "/mirror" };
        assert_eq!(
            workspace_relative_to_mirror_uri("src/a.gd", mirror)
                .as_deref()
                .map(str::to_owned),
            Some(path_to_file_uri(&format!(
                "{mirror}{}src{}a.gd",
                MAIN_SEPARATOR_TEST, MAIN_SEPARATOR_TEST
            )))
        );
        assert_eq!(workspace_relative_to_mirror_uri("", mirror), None);
        assert_eq!(workspace_relative_to_mirror_uri("/abs/gd", mirror), None);
        assert_eq!(
            workspace_relative_to_mirror_uri("../escape.gd", mirror),
            None
        );
        assert_eq!(
            workspace_relative_to_mirror_uri("C:\\x\\a.gd", mirror),
            None
        );
    }

    const MAIN_SEPARATOR_TEST: char = std::path::MAIN_SEPARATOR;
}
