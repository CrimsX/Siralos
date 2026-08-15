//! Shared bounded filesystem and text primitives for workspace
//! adapters (R4).
//!
//! The authoritative read primitive is a bounded complete read: the
//! target is lstat-verified first (a non-regular file or a symbolic
//! link is rejected without being opened, so a FIFO can never block),
//! the declared size may reject early, and the read itself continues
//! until EOF or until more than `max_bytes` bytes are observed. One
//! short read is never treated as EOF, and a partial prefix is never
//! returned as complete content: the caller receives either the exact
//! complete bytes or a typed too-large outcome. Every failure class is
//! distinguishable at the primitive (`NotReadable` vs `IoError` vs
//! `TooLarge`), while the reference protocol continues to collapse
//! them into its stable `failed` vocabulary exactly like the
//! TypeScript oracle.

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

/// Outcome of one bounded complete read of a byte stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedReadOutcome {
    /// EOF was reached and the stream is at most `max_bytes` long; the
    /// bytes are the complete stream content.
    Complete(Vec<u8>),
    /// More than `max_bytes` bytes exist; no partial data is returned.
    TooLarge,
}

/// Read one byte stream with the bounded whole-file contract:
///
/// 1. at most `max_bytes + 1` bytes are ever read;
/// 2. reading continues until EOF or until the cap is reached, so one
///    short read is never treated as EOF;
/// 3. the complete stream is returned only when EOF was reached at a
///    size <= `max_bytes`;
/// 4. more than `max_bytes` bytes yields `TooLarge`, never a partial
///    prefix presented as complete;
/// 5. allocation grows incrementally with the observed stream length;
///    no `max_bytes + 1` buffer is reserved up front.
///
/// `Read::take` caps the total readable bytes and `read_to_end`
/// repeatedly reads until EOF (retrying `Interrupted` errors), so the
/// EOF/size determination is made from the actual read result, never
/// from file metadata. For `max_bytes == usize::MAX` the cap is the
/// address-space maximum and `TooLarge` is unreachable by
/// construction; every practical caller uses a small explicit bound.
pub fn read_complete_bounded<R: Read>(
    mut reader: R,
    max_bytes: usize,
) -> std::io::Result<BoundedReadOutcome> {
    let limit = max_bytes.saturating_add(1) as u64;
    let mut bytes = Vec::new();
    reader.by_ref().take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > max_bytes {
        return Ok(BoundedReadOutcome::TooLarge);
    }
    Ok(BoundedReadOutcome::Complete(bytes))
}

/// Outcome of one bounded complete read of a file path.
#[derive(Debug)]
pub enum BoundedFileRead {
    /// EOF was reached at a size <= `max_bytes`; the exact complete
    /// bytes.
    Complete(Vec<u8>),
    /// More than `max_bytes` bytes exist; no partial data is returned.
    TooLarge,
    /// The target is missing, a symbolic link, or not a regular file.
    NotReadable,
    /// An I/O failure occurred while inspecting, opening, or reading.
    IoError(std::io::Error),
}

impl PartialEq for BoundedFileRead {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Complete(a), Self::Complete(b)) => a == b,
            (Self::TooLarge, Self::TooLarge) => true,
            (Self::NotReadable, Self::NotReadable) => true,
            (Self::IoError(a), Self::IoError(b)) => a.kind() == b.kind(),
            _ => false,
        }
    }
}

/// Bounded complete read of one regular file, mirroring the reference
/// `readFileBounded` null behavior while never treating one short read
/// as EOF and never returning a partial prefix as complete content.
///
/// The target is lstat-verified first (a non-regular file or a symbolic
/// link is rejected without being opened, so a FIFO can never block the
/// read), the declared size is used only as an early too-large
/// rejection, and the authoritative completion decision is made from
/// the bounded read loop itself, so a file grown or swapped after the
/// lstat is never fully materialized and never reported as complete.
pub fn read_complete_file_bounded(
    path: &Path,
    max_bytes: usize,
) -> BoundedFileRead {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return BoundedFileRead::NotReadable,
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return BoundedFileRead::NotReadable;
    }
    if metadata.len() > max_bytes as u64 {
        return BoundedFileRead::TooLarge;
    }
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return BoundedFileRead::IoError(error),
    };
    match read_complete_bounded(&mut file, max_bytes) {
        Ok(BoundedReadOutcome::Complete(bytes)) => {
            BoundedFileRead::Complete(bytes)
        }
        Ok(BoundedReadOutcome::TooLarge) => BoundedFileRead::TooLarge,
        Err(error) => BoundedFileRead::IoError(error),
    }
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
        BoundedFileRead, BoundedReadOutcome, decode_utf8, fold_path_component,
        looks_binary, read_complete_bounded, read_complete_file_bounded,
        split_into_lines, utf16_index_of, utf16_len, utf16_slice,
    };
    use std::io::Read;

    /// Deterministic test reader that yields at most `chunk_size` bytes
    /// per `read` call (a short read is always legal before EOF) and may
    /// inject exactly one `Interrupted` error at a chosen position.
    /// This is the seam that proves the bounded complete read
    /// reconstructs the whole logical stream from arbitrarily small
    /// chunks without relying on OS read behavior.
    struct ChunkedReader {
        data: Vec<u8>,
        position: usize,
        chunk_size: usize,
        interrupt_at: Option<usize>,
        interrupted: bool,
    }

    impl ChunkedReader {
        fn new(
            data: Vec<u8>,
            chunk_size: usize,
            interrupt_at: Option<usize>,
        ) -> Self {
            Self {
                data,
                position: 0,
                chunk_size,
                interrupt_at,
                interrupted: false,
            }
        }
    }

    impl Read for ChunkedReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if let Some(at) = self.interrupt_at {
                if !self.interrupted && self.position == at {
                    self.interrupted = true;
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "injected interruption",
                    ));
                }
            }
            if self.position >= self.data.len() {
                return Ok(0);
            }
            let count = (self.data.len() - self.position)
                .min(self.chunk_size)
                .min(buffer.len());
            buffer[..count].copy_from_slice(
                &self.data[self.position..self.position + count],
            );
            self.position += count;
            Ok(count)
        }
    }

    fn complete(outcome: BoundedReadOutcome) -> Vec<u8> {
        match outcome {
            BoundedReadOutcome::Complete(bytes) => bytes,
            BoundedReadOutcome::TooLarge => {
                panic!("expected complete bytes, got TooLarge")
            }
        }
    }

    #[test]
    fn deterministic_short_reads_reconstruct_the_complete_stream() {
        // For every legal short-read chunking (1..=11 bytes per read) of
        // several logical inputs at or below the bound, the bounded
        // complete read must return exactly the original stream.
        let inputs: Vec<Vec<u8>> = vec![
            Vec::new(),
            vec![b'a'],
            b"ab".to_vec(),
            b"abc".to_vec(),
            b"the quick brown fox".to_vec(),
            (0..300).map(|index| (index % 251) as u8).collect(),
        ];
        for input in &inputs {
            for chunk_size in 1..=11 {
                let outcome = read_complete_bounded(
                    ChunkedReader::new(input.clone(), chunk_size, None),
                    input.len(),
                )
                .unwrap();
                assert_eq!(
                    complete(outcome),
                    *input,
                    "chunk_size {chunk_size}, len {}",
                    input.len()
                );
            }
        }
    }

    #[test]
    fn too_large_is_deterministic_across_short_read_chunking() {
        // A stream longer than the bound must always yield TooLarge, no
        // matter how the reads are chunked: a partial prefix is never
        // returned as complete content.
        let input: Vec<u8> =
            (0..64).map(|index| (index % 251) as u8).collect();
        for chunk_size in 1..=11 {
            assert_eq!(
                read_complete_bounded(
                    ChunkedReader::new(input.clone(), chunk_size, None),
                    32,
                )
                .unwrap(),
                BoundedReadOutcome::TooLarge,
                "chunk_size {chunk_size}"
            );
        }
        // Exactly one byte over the bound is still TooLarge.
        for chunk_size in 1..=5 {
            assert_eq!(
                read_complete_bounded(
                    ChunkedReader::new(vec![b'x'; 33], chunk_size, None),
                    32,
                )
                .unwrap(),
                BoundedReadOutcome::TooLarge,
                "chunk_size {chunk_size}"
            );
        }
    }

    #[test]
    fn interrupted_reads_are_retried_by_the_complete_read_loop() {
        // `read_to_end` retries `Interrupted` errors; prove it
        // deterministically with an injected interruption before the
        // first byte and at EOF.
        let data = b"retry me".to_vec();
        for interrupt_at in [Some(0), Some(data.len())] {
            let outcome = read_complete_bounded(
                ChunkedReader::new(data.clone(), 2, interrupt_at),
                data.len(),
            )
            .unwrap();
            assert_eq!(
                complete(outcome),
                data,
                "interrupt_at {interrupt_at:?}"
            );
        }
    }

    #[test]
    fn exact_boundary_sizes_on_real_files() {
        let dir = std::env::temp_dir()
            .join(format!("siralos-fs-bounds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let limit = 16usize;
        let cases: Vec<(&str, usize, bool)> = vec![
            ("empty.txt", 0, true),
            ("one.txt", 1, true),
            ("small.txt", 7, true),
            ("exact.txt", limit, true),
            ("over.txt", limit + 1, false),
            ("large.txt", limit * 8, false),
        ];
        for (name, size, expect_complete) in cases {
            let path = dir.join(name);
            std::fs::write(&path, vec![b'z'; size]).unwrap();
            let outcome = read_complete_file_bounded(&path, limit);
            match outcome {
                BoundedFileRead::Complete(bytes) => {
                    assert!(expect_complete, "{name} must be TooLarge");
                    assert_eq!(bytes.len(), size, "{name}");
                    assert!(bytes.iter().all(|byte| *byte == b'z'));
                }
                BoundedFileRead::TooLarge => {
                    assert!(!expect_complete, "{name} must be complete");
                }
                other => panic!("{name}: unexpected outcome {other:?}"),
            }
        }
        // Exactly one byte over the bound is TooLarge and never partial.
        assert!(matches!(
            read_complete_file_bounded(&dir.join("over.txt"), limit),
            BoundedFileRead::TooLarge
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

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
            read_complete_file_bounded(&dir.join("ok.txt"), 16),
            BoundedFileRead::Complete(b"hello".to_vec()),
        );
        assert_eq!(
            read_complete_file_bounded(&dir.join("missing"), 16),
            BoundedFileRead::NotReadable,
        );
        assert_eq!(
            read_complete_file_bounded(&dir.join("big.txt"), 16),
            BoundedFileRead::TooLarge,
        );
        assert_eq!(
            read_complete_file_bounded(&dir, 16),
            BoundedFileRead::NotReadable,
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
