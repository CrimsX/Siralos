//! Bounded exact source reads (R4, reference `workspace.read`).
//!
//! The authoritative read path: containment-safe resolution, excluded
//! directories, regular-file verification, bounded single-shot read,
//! binary probe, strict UTF-8 decoding, whole-file SHA-256, revision
//! issuance, and deterministic line-range slicing with UTF-16-aware
//! content truncation (whole-file identity is never derived from
//! truncated returned text). Structural/summary modes are R5-owned
//! language surfaces: for non-GDScript files the reference returns an
//! explicit `supported: false` success; for GDScript files this R4
//! adapter reports the typed unsupported disposition and R5 owns the
//! language extraction.

use crate::workspace::fs::{
    DEFAULT_EXCLUDED_DIRECTORIES, decode_utf8, looks_binary,
    read_file_bounded, split_into_lines, utf16_len, utf16_slice,
};
use crate::workspace::list::excluded_component;
use crate::workspace::resolve::resolve_workspace_path;

use siralos_core::workspace::bounds::WorkspaceLimits;
use siralos_core::workspace::revision::{
    ObservedReadMode, WorkspaceRevisionRegistry,
};

use std::path::Path;

/// Read mode requested by the caller (protocol vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadMode {
    /// Authoritative exact source read.
    Exact,
    /// Structural mode (R5-owned language surface).
    Structural,
    /// Summary mode (R5-owned language surface).
    Summary,
}

impl ReadMode {
    /// The canonical protocol string for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Structural => "structural",
            Self::Summary => "summary",
        }
    }

    /// Parse a protocol mode string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "exact" => Some(Self::Exact),
            "structural" => Some(Self::Structural),
            "summary" => Some(Self::Summary),
            _ => None,
        }
    }
}

/// Validated read request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadInput {
    /// Requested workspace-relative path.
    pub path: String,
    /// One-based start line (default 1).
    pub start_line: u64,
    /// Inclusive one-based end line (default: last line).
    pub end_line: Option<u64>,
    /// Read mode (default exact).
    pub mode: ReadMode,
}

/// Why a read request was rejected as invalid input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadInputError {
    /// The tool input is not a JSON object.
    NotAnObject,
    /// `path` is missing or empty.
    MissingPath,
    /// `path` is not a string.
    PathNotString,
    /// A line number is not a positive integer.
    InvalidLineNumber(String),
    /// `endLine` precedes `startLine`.
    EndBeforeStart,
    /// `mode` is not a read mode.
    InvalidMode,
}

/// Parse and validate a read request against the reference input
/// schema (JSON object, required non-empty path, positive integer line
/// bounds, read-mode enum).
pub fn parse_read_input(
    input: &serde_json::Value,
) -> Result<ReadInput, ReadInputError> {
    let object = match input {
        serde_json::Value::Object(object) => object,
        _ => return Err(ReadInputError::NotAnObject),
    };
    let path = match object.get("path") {
        Some(serde_json::Value::String(value)) if !value.is_empty() => {
            value.clone()
        }
        Some(serde_json::Value::String(_)) => {
            return Err(ReadInputError::MissingPath);
        }
        Some(_) => return Err(ReadInputError::PathNotString),
        None => return Err(ReadInputError::MissingPath),
    };
    let positive = |key: &str| -> Result<Option<u64>, ReadInputError> {
        match object.get(key) {
            None => Ok(None),
            Some(serde_json::Value::Number(number)) => {
                let value = number.as_u64().filter(|value| *value >= 1);
                match value {
                    Some(value) => Ok(Some(value)),
                    None => {
                        Err(ReadInputError::InvalidLineNumber(key.to_owned()))
                    }
                }
            }
            Some(_) => Err(ReadInputError::InvalidLineNumber(key.to_owned())),
        }
    };
    let start_line = positive("startLine")?.unwrap_or(1);
    let end_line = positive("endLine")?;
    if end_line.is_some_and(|end| end < start_line) {
        return Err(ReadInputError::EndBeforeStart);
    }
    let mode = match object.get("mode") {
        None => ReadMode::Exact,
        Some(serde_json::Value::String(value)) => {
            ReadMode::parse(value).ok_or(ReadInputError::InvalidMode)?
        }
        Some(_) => return Err(ReadInputError::InvalidMode),
    };
    Ok(ReadInput { path, start_line, end_line, mode })
}

/// Outcome of one exact read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The input was invalid.
    InvalidInput {
        /// Stable validation message.
        message: String,
    },
    /// The path was rejected (denied).
    Denied {
        /// Stable rejection message.
        message: String,
    },
    /// The operation was cancelled.
    Cancelled,
    /// The target failed inspection or decoding (failed).
    Failed {
        /// Stable failure message.
        message: String,
    },
    /// Successful read with exact identity.
    Success {
        /// Canonical workspace-relative path.
        path: String,
        /// Whole-file SHA-256 of the exact bytes.
        sha256: String,
        /// Issued revision handle (when a registry is provided).
        revision: Option<String>,
        /// Returned content (line range, UTF-16-truncated).
        content: String,
        /// One-based start line actually returned.
        start_line: u64,
        /// Inclusive one-based end line actually returned.
        end_line: u64,
        /// Total line count of the file.
        total_lines: u64,
        /// True when returned content was truncated to the char bound.
        truncated: bool,
    },
    /// Explicit typed unsupported disposition for language modes: the
    /// reference returns `supported: false` success for non-GDScript
    /// files, and GDScript structural/summary extraction is R5-owned.
    Unsupported {
        /// Canonical workspace-relative path.
        path: String,
        /// The requested language mode.
        mode: ReadMode,
        /// Issued revision handle (when a registry is provided).
        revision: Option<String>,
        /// True when the file type is structurally supported (`.gd`);
        /// extraction itself is not ported at R4.
        supported: bool,
        /// Stable reason string.
        reason: String,
    },
}

/// Read one text file inside the workspace with the reference bounds.
pub fn read_file(
    root: &Path,
    input: &ReadInput,
    limits: &WorkspaceLimits,
    mut revisions: Option<&mut WorkspaceRevisionRegistry>,
    cancelled: bool,
) -> ReadOutcome {
    let resolved = match resolve_workspace_path(root, &input.path) {
        Ok(resolved) => resolved,
        Err(rejection) => {
            return ReadOutcome::Denied { message: rejection.to_string() };
        }
    };
    if let Some(component) = excluded_component(
        &resolved.workspace_relative_path,
        &DEFAULT_EXCLUDED_DIRECTORIES,
    ) {
        return ReadOutcome::Denied {
            message: format!(
                "Path is inside the excluded directory {component}."
            ),
        };
    }
    if cancelled {
        return ReadOutcome::Cancelled;
    }
    let metadata = match std::fs::symlink_metadata(&resolved.absolute_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            return ReadOutcome::Failed {
                message: format!("Cannot inspect file: {error}"),
            };
        }
    };
    if !metadata.is_file() {
        return ReadOutcome::Failed {
            message: "Target is not a regular file.".to_owned(),
        };
    }
    let size = metadata.len();
    if size > limits.max_read_file_size_bytes as u64 {
        return ReadOutcome::Failed {
            message: format!(
                "File is too large ({size} bytes; limit {}).",
                limits.max_read_file_size_bytes
            ),
        };
    }
    let bytes = match read_file_bounded(
        &resolved.absolute_path,
        limits.max_read_file_size_bytes,
    ) {
        Ok(Some(bytes)) => bytes,
        _ => {
            return ReadOutcome::Failed {
                message: format!(
                    "Cannot read file: it is missing, not a regular file, or exceeds the {}-byte limit.",
                    limits.max_read_file_size_bytes
                ),
            };
        }
    };
    if cancelled {
        return ReadOutcome::Cancelled;
    }
    if looks_binary(&bytes) {
        return ReadOutcome::Failed {
            message: "File appears to be binary.".to_owned(),
        };
    }
    let text = match decode_utf8(&bytes) {
        Some(text) => text,
        None => {
            return ReadOutcome::Failed {
                message: "File is not valid UTF-8 text.".to_owned(),
            };
        }
    };
    let sha256 = siralos_core::identity::sha256_hex(&bytes);
    let relative = resolved.workspace_relative_path.clone();
    let revision =
        revisions.as_mut().map(|registry| registry.issue(&relative, &sha256));
    if let Some(registry) = revisions.as_mut() {
        if let Some(handle) = &revision {
            let mode = match input.mode {
                ReadMode::Exact => ObservedReadMode::Exact,
                ReadMode::Structural => ObservedReadMode::Structural,
                ReadMode::Summary => ObservedReadMode::Summary,
            };
            registry.observe_read(&relative, handle, mode);
        }
    }
    match input.mode {
        ReadMode::Structural | ReadMode::Summary => {
            let mode = input.mode;
            if !relative.to_lowercase().ends_with(".gd") {
                return ReadOutcome::Unsupported {
                    path: relative,
                    mode,
                    revision,
                    supported: false,
                    reason: "Structural and summary modes support GDScript (.gd) files only."
                        .to_owned(),
                };
            }
            return ReadOutcome::Unsupported {
                path: relative,
                mode,
                revision,
                supported: true,
                reason: "GDScript structural/summary extraction is R5-owned language intelligence and is not ported at R4.".to_owned(),
            };
        }
        ReadMode::Exact => {}
    }
    let lines = split_into_lines(&text);
    let total_lines = lines.len() as u64;
    if input.start_line > total_lines {
        return ReadOutcome::Failed {
            message: format!(
                "\"startLine\" ({}) is beyond the end of the file ({total_lines} lines).",
                input.start_line
            ),
        };
    }
    let end_line = input.end_line.unwrap_or(total_lines).min(total_lines);
    let mut content = lines
        .iter()
        .skip((input.start_line - 1) as usize)
        .take((end_line - input.start_line + 1) as usize)
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    let mut truncated = false;
    if utf16_len(&content) > limits.max_read_content_chars {
        content =
            utf16_slice(&content, limits.max_read_content_chars).to_owned();
        truncated = true;
    }
    ReadOutcome::Success {
        path: relative,
        sha256,
        revision,
        content,
        start_line: input.start_line,
        end_line,
        total_lines,
        truncated,
    }
}
#[cfg(test)]
mod tests {
    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
    use super::{
        ReadInput, ReadMode, ReadOutcome, parse_read_input, read_file,
    };
    use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

    fn workspace() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "siralos-read-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("dir")).unwrap();
        std::fs::write(base.join("a.txt"), b"line one\nline two\n").unwrap();
        std::fs::write(base.join("dir/f.txt"), b"hello").unwrap();
        std::fs::write(base.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(
            base.join("README.md"),
            b"# Fixture\nPlain markdown.\n",
        )
        .unwrap();
        base
    }

    #[test]
    fn exact_read_returns_identity_and_content() {
        let base = workspace();
        let input = ReadInput {
            path: "a.txt".to_owned(),
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        };
        let outcome = read_file(&base, &input, &WORKSPACE_LIMITS, None, false);
        let ReadOutcome::Success {
            path,
            content,
            start_line,
            end_line,
            total_lines,
            truncated,
            ..
        } = outcome
        else {
            panic!("read failed: {outcome:?}");
        };
        assert_eq!(path, "a.txt");
        assert_eq!(content, "line one\nline two");
        assert_eq!(start_line, 1);
        assert_eq!(end_line, 2);
        assert_eq!(total_lines, 2);
        assert!(!truncated);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn line_ranges_clamp_and_fail_at_boundaries() {
        let base = workspace();
        let ranged = ReadInput {
            path: "a.txt".to_owned(),
            start_line: 2,
            end_line: Some(99),
            mode: ReadMode::Exact,
        };
        let ReadOutcome::Success { content, end_line, .. } =
            read_file(&base, &ranged, &WORKSPACE_LIMITS, None, false)
        else {
            panic!("ranged read failed");
        };
        assert_eq!(content, "line two");
        assert_eq!(end_line, 2);
        let beyond = ReadInput {
            path: "a.txt".to_owned(),
            start_line: 9,
            end_line: None,
            mode: ReadMode::Exact,
        };
        assert!(matches!(
            read_file(&base, &beyond, &WORKSPACE_LIMITS, None, false),
            ReadOutcome::Failed { .. },
        ));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_binary_directories_and_excluded_paths() {
        let base = workspace();
        std::fs::create_dir_all(base.join("node_modules/pkg")).unwrap();
        std::fs::write(base.join("node_modules/pkg/x.js"), b"x").unwrap();
        let binary = ReadInput {
            path: "bin.dat".to_owned(),
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        };
        assert!(matches!(
            read_file(&base, &binary, &WORKSPACE_LIMITS, None, false),
            ReadOutcome::Failed { .. },
        ));
        let directory = ReadInput {
            path: "dir".to_owned(),
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        };
        assert!(matches!(
            read_file(&base, &directory, &WORKSPACE_LIMITS, None, false),
            ReadOutcome::Failed { .. },
        ));
        let excluded = ReadInput {
            path: "node_modules/pkg/x.js".to_owned(),
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        };
        assert!(matches!(
            read_file(&base, &excluded, &WORKSPACE_LIMITS, None, false),
            ReadOutcome::Denied { .. },
        ));
        let escape = ReadInput {
            path: "../secret".to_owned(),
            start_line: 1,
            end_line: None,
            mode: ReadMode::Exact,
        };
        assert!(matches!(
            read_file(&base, &escape, &WORKSPACE_LIMITS, None, false),
            ReadOutcome::Denied { .. },
        ));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn unsupported_modes_report_the_typed_disposition() {
        let base = workspace();
        let summary = ReadInput {
            path: "README.md".to_owned(),
            start_line: 1,
            end_line: None,
            mode: ReadMode::Summary,
        };
        let outcome =
            read_file(&base, &summary, &WORKSPACE_LIMITS, None, false);
        let ReadOutcome::Unsupported { supported, mode, .. } = outcome else {
            panic!("expected unsupported disposition: {outcome:?}");
        };
        assert!(!supported);
        assert_eq!(mode, ReadMode::Summary);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn input_validation_mirrors_the_reference() {
        let object = serde_json::json!({ "path": "a.txt" });
        assert!(parse_read_input(&object).is_ok());
        assert_eq!(
            parse_read_input(&serde_json::json!({})),
            Err(super::ReadInputError::MissingPath),
        );
        assert_eq!(
            parse_read_input(
                &serde_json::json!({ "path": "a.txt", "startLine": 0 })
            ),
            Err(super::ReadInputError::InvalidLineNumber(
                "startLine".to_owned()
            )),
        );
        assert_eq!(
            parse_read_input(
                &serde_json::json!({ "path": "a.txt", "startLine": 3, "endLine": 2 })
            ),
            Err(super::ReadInputError::EndBeforeStart),
        );
        assert_eq!(
            parse_read_input(
                &serde_json::json!({ "path": "a.txt", "mode": "fancy" })
            ),
            Err(super::ReadInputError::InvalidMode),
        );
        assert_eq!(
            parse_read_input(&serde_json::json!([1, 2])),
            Err(super::ReadInputError::NotAnObject),
        );
    }
}
