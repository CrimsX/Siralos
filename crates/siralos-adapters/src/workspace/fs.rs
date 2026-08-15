//! Shared bounded filesystem and text primitives for workspace
//! adapters (R4).
//!
//! `read_file_bounded` mirrors the reference single-shot bounded read:
//! the target is lstat-verified first (a non-regular file or a symbolic
//! link is rejected without being opened, so a FIFO can never block),
//! and the read itself is capped at `max_bytes + 1`, so a file grown or
//! swapped after the lstat is never fully materialized. Every failure
//! returns `Ok(None)` exactly like the reference.

use std::io::Read;
use std::path::Path;

/// The reference default excluded directory names.
pub const DEFAULT_EXCLUDED_DIRECTORIES: [&str; 4] =
    ["node_modules", ".git", "dist", "coverage"];

/// Prefix of mutation staging entries excluded from listings.
pub const MUTATION_TEMP_PREFIX: &str = ".siralos-mutation-";

/// Case-folding policy: Windows and macOS fold (macOS volumes are
/// treated conservatively as case-insensitive), matching the
/// reference `foldPathComponent`.
pub fn is_case_insensitive_platform() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// Fold one path component under the platform policy.
pub fn fold_path_component(value: &str, fold: bool) -> String {
    if fold { value.to_lowercase() } else { value.to_owned() }
}

/// Join the canonical root with a validated relative request and
/// normalize `.`/`..` components (equivalent of `path.resolve`).
pub fn normalize_join(root: &Path, requested: &str) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::from(root);
    for component in requested.split(['/', '\\']) {
        match component {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            name => out.push(name),
        }
    }
    out
}

/// Bounded single-shot file read mirroring `readFileBounded`: returns
/// `Ok(None)` for missing, linked, non-regular, or oversized targets
/// and for any I/O failure; a single read is never treated as EOF.
pub fn read_file_bounded(
    path: &Path,
    max_bytes: usize,
) -> std::io::Result<Option<Vec<u8>>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(None);
    }
    let declared = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if declared > max_bytes {
        return Ok(None);
    }
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return Ok(None),
    };
    let mut buffer = vec![0u8; max_bytes + 1];
    let bytes_read = match file.read(&mut buffer) {
        Ok(bytes_read) => bytes_read,
        Err(_) => return Ok(None),
    };
    if bytes_read > max_bytes {
        return Ok(None);
    }
    buffer.truncate(bytes_read);
    Ok(Some(buffer))
}
/// Binary probe: a NUL byte within the first 8192 bytes marks binary
/// content, mirroring the reference `looksBinary`.
pub fn looks_binary(bytes: &[u8]) -> bool {
    let probe_length = bytes.len().min(8192);
    bytes[..probe_length].contains(&0)
}

/// Strict UTF-8 decoding (fatal on invalid sequences).
pub fn decode_utf8(bytes: &[u8]) -> Option<String> {
    String::from_utf8(bytes.to_vec()).ok()
}

/// Split text into lines mirroring `splitIntoLines`: a single trailing
/// newline is dropped, then lines split on `\n` with a trailing `\r`
/// removed per line.
pub fn split_into_lines(text: &str) -> Vec<&str> {
    let without_trailing = text.strip_suffix('\n').unwrap_or(text);
    without_trailing
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// UTF-16 code-unit length of a string (the reference measures JS
/// string lengths in UTF-16 code units).
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// Slice a string to at most `limit` UTF-16 code units without
/// splitting a surrogate pair.
pub fn utf16_slice(text: &str, limit: usize) -> &str {
    let mut units = 0;
    for (index, character) in text.char_indices() {
        let next = units + character.len_utf16();
        if next > limit {
            return &text[..index];
        }
        units = next;
    }
    text
}

/// UTF-16 code-unit index of the first occurrence of `query` in
/// `text`, mirroring JavaScript `String.prototype.indexOf` semantics.
pub fn utf16_index_of(text: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let byte_index = text.find(query)?;
    Some(utf16_len(&text[..byte_index]))
}
#[cfg(test)]
mod tests {
    use super::{
        decode_utf8, fold_path_component, looks_binary, read_file_bounded,
        split_into_lines, utf16_index_of, utf16_len, utf16_slice,
    };

    #[test]
    fn binary_probe_only_inspects_the_first_8192_bytes() {
        let mut bytes = vec![b'a'; 9000];
        bytes[9000 - 1] = 0;
        assert!(looks_binary(&[0, 1, 2]));
        assert!(!looks_binary(&bytes));
    }

    #[test]
    fn line_splitting_matches_the_reference() {
        assert_eq!(split_into_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(split_into_lines("a\r\nb\r\n"), vec!["a", "b"]);
        assert_eq!(split_into_lines("hello"), vec!["hello"]);
        assert_eq!(split_into_lines(""), vec![""]);
    }

    #[test]
    fn utf16_helpers_are_surrogate_pair_safe() {
        assert_eq!(utf16_len("a"), 1);
        assert_eq!(utf16_len("\u{1f600}"), 2);
        assert_eq!(utf16_slice("ab\u{1f600}c", 2), "ab");
        assert_eq!(utf16_index_of("ab\u{1f600}c", "c"), Some(4));
    }

    #[test]
    fn decoding_and_folding_are_deterministic() {
        assert_eq!(decode_utf8(b"hello"), Some("hello".to_owned()));
        assert_eq!(decode_utf8(&[0xc3, 0x28]), None);
        assert_eq!(fold_path_component("Node_Modules", true), "node_modules");
        assert_eq!(fold_path_component("Node_Modules", false), "Node_Modules");
    }

    #[test]
    fn bounded_read_rejects_linked_and_oversized_targets() {
        let dir = std::env::temp_dir()
            .join(format!("siralos-fs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ok.txt"), b"hello").unwrap();
        std::fs::write(dir.join("big.txt"), vec![b'x'; 64]).unwrap();
        assert_eq!(
            read_file_bounded(&dir.join("ok.txt"), 16).unwrap().unwrap(),
            b"hello".to_vec(),
        );
        assert!(
            read_file_bounded(&dir.join("missing"), 16).unwrap().is_none()
        );
        assert!(
            read_file_bounded(&dir.join("big.txt"), 16).unwrap().is_none()
        );
        assert!(read_file_bounded(&dir, 16).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
