//! Generic diagnostic model, payload normalization, and deterministic
//! set normalization (Stage 3R R5).
//!
//! Severities are a closed enum, missing locations are never fabricated,
//! messages are sanitized and bounded, per-set and per-run collections
//! are bounded with explicit truncation, and aggregation is deterministic
//! (exact duplicates collapsed, sorted by path, line, column, then
//! message using the reference UTF-16 string ordering). The vocabulary
//! and semantics match the reference behavior (ADR 0010/0011).

use crate::language::limits::LanguageLimits;
use crate::language::position::{RawRange, to_one_based_range};
use crate::language::sanitize::sanitize_control_characters;
use crate::language::truncate::truncate_utf8_bytes;

use std::cmp::Ordering;

/// Diagnostic severity vocabulary (reference: error/warning/info/unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    /// Error.
    Error,
    /// Warning.
    Warning,
    /// Information.
    Info,
    /// Unknown/unmapped severity (preserved conservatively).
    Unknown,
}

impl DiagnosticSeverity {
    /// The canonical protocol string for this severity.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
            Self::Unknown => "unknown",
        }
    }

    /// Parse a protocol severity string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            "info" => Some(Self::Info),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Map an LSP severity integer to the reference vocabulary
/// (1=error, 2=warning, 3/4=info, anything else unknown).
pub fn map_lsp_severity(value: Option<i64>) -> DiagnosticSeverity {
    match value {
        Some(1) => DiagnosticSeverity::Error,
        Some(2) => DiagnosticSeverity::Warning,
        Some(3) | Some(4) => DiagnosticSeverity::Info,
        _ => DiagnosticSeverity::Unknown,
    }
}

/// One normalized, bounded, sanitized diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Source label of the producing language service.
    pub source: String,
    /// Normalized severity.
    pub severity: DiagnosticSeverity,
    /// Workspace-relative path; None when the service carries none.
    pub path: Option<String>,
    /// One-based line; None when unknown (never fabricated).
    pub line: Option<u64>,
    /// One-based column; None when unknown (never fabricated).
    pub column: Option<u64>,
    /// Stable diagnostic code when present; else None.
    pub code: Option<String>,
    /// Bounded, control-character-sanitized message.
    pub message: String,
    /// Raw category token preserved from the service; else None.
    pub raw_category: Option<String>,
}

/// A raw diagnostic code before normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawDiagnosticCode {
    /// String code (bounded).
    Text(String),
    /// Numeric code (retained as text).
    Number(i64),
}

/// One raw LSP-shaped diagnostic entry before normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDiagnostic {
    /// Raw 0-based range; None when absent or malformed.
    pub range: Option<RawRange>,
    /// Raw LSP severity integer.
    pub severity: Option<i64>,
    /// Raw diagnostic code.
    pub code: Option<RawDiagnosticCode>,
    /// Raw message.
    pub message: Option<String>,
    /// Raw category token (LSP `source` field).
    pub source: Option<String>,
}

/// A normalized bounded diagnostic payload for one document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedDiagnosticPayload {
    /// Workspace-relative path of the document.
    pub path: String,
    /// Normalized diagnostics in payload order.
    pub diagnostics: Vec<Diagnostic>,
    /// True when the per-set bound was applied.
    pub truncated: bool,
}

fn map_code(code: &RawDiagnosticCode, max_message_bytes: usize) -> String {
    match code {
        RawDiagnosticCode::Text(text) => {
            truncate_utf8_bytes(text, max_message_bytes)
        }
        RawDiagnosticCode::Number(number) => number.to_string(),
    }
}

fn bound_message(
    message: &str,
    mask_root: Option<&str>,
    max_message_bytes: usize,
) -> String {
    let sanitized = sanitize_control_characters(message).trim().to_owned();
    let masked = match mask_root {
        Some(root) if !root.is_empty() => sanitized.replace(root, "<mirror>"),
        _ => sanitized,
    };
    truncate_utf8_bytes(&masked, max_message_bytes)
}

/// Normalize one raw LSP-shaped diagnostic payload (0-based positions,
/// unknown severities, untrusted messages) into the bounded 1-based
/// model. Malformed entries and empty messages are skipped
/// conservatively; the workspace-relative `path` is produced by the
/// adapter's URI mapping before this function is called.
pub fn normalize_diagnostic_payload(
    raw: &[RawDiagnostic],
    source: &str,
    path: &str,
    mask_root: Option<&str>,
    limits: &LanguageLimits,
) -> NormalizedDiagnosticPayload {
    let mut diagnostics = Vec::new();
    let mut truncated = false;
    for entry in raw {
        if diagnostics.len() >= limits.max_diagnostics_per_set {
            truncated = true;
            break;
        }
        let range = entry.range.and_then(to_one_based_range);
        let severity = map_lsp_severity(entry.severity);
        let code = entry
            .code
            .as_ref()
            .map(|code| map_code(code, limits.max_diagnostic_message_bytes));
        let message = match entry.message.as_deref() {
            Some(message) => bound_message(
                message,
                mask_root,
                limits.max_diagnostic_message_bytes,
            ),
            None => String::new(),
        };
        if message.is_empty() {
            continue;
        }
        diagnostics.push(Diagnostic {
            source: source.to_owned(),
            severity,
            path: Some(path.to_owned()),
            line: range.map(|range| range.start.line),
            column: range.map(|range| range.start.column),
            code,
            message,
            raw_category: entry.source.clone(),
        });
    }
    NormalizedDiagnosticPayload {
        path: path.to_owned(),
        diagnostics,
        truncated,
    }
}

/// The UTF-16 code units of a string (JavaScript string order).
fn utf16_units(text: &str) -> Vec<u16> {
    let mut units = Vec::with_capacity(text.len());
    let mut buffer = [0u16; 2];
    for character in text.chars() {
        units.extend_from_slice(character.encode_utf16(&mut buffer));
    }
    units
}

/// Compare two strings in JavaScript string order (UTF-16 code units),
/// which is the reference ordering for deterministic sorts.
pub fn utf16_cmp(left: &str, right: &str) -> Ordering {
    let left_units = utf16_units(left);
    let right_units = utf16_units(right);
    for (left_unit, right_unit) in left_units.iter().zip(right_units.iter()) {
        if left_unit != right_unit {
            return left_unit.cmp(right_unit);
        }
    }
    left_units.len().cmp(&right_units.len())
}

/// Deterministic diagnostic aggregation: exact duplicates are collapsed
/// (path, line, column, code, message), results are sorted by (path,
/// line, column, message) in reference order, and the run-wide bound is
/// applied with explicit truncation.
pub fn normalize_diagnostic_set(
    diagnostics: Vec<Diagnostic>,
    max_diagnostics: usize,
) -> (Vec<Diagnostic>, bool) {
    let mut seen = std::collections::HashSet::new();
    let mut unique = Vec::with_capacity(diagnostics.len());
    for diagnostic in diagnostics {
        let key = format!(
            "{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
            diagnostic.path.as_deref().unwrap_or(""),
            diagnostic.line.map(|line| line as i64).unwrap_or(-1),
            diagnostic.column.map(|column| column as i64).unwrap_or(-1),
            diagnostic.code.as_deref().unwrap_or(""),
            diagnostic.message,
        );
        if !seen.insert(key) {
            continue;
        }
        unique.push(diagnostic);
    }
    unique.sort_by(|left, right| {
        let left_path = left.path.as_deref().unwrap_or("");
        let right_path = right.path.as_deref().unwrap_or("");
        let by_path = utf16_cmp(left_path, right_path);
        if by_path != Ordering::Equal {
            return by_path;
        }
        // Reference treats null as -1, which sorts before line 1;
        // lines are u64, so the comparison uses i64 keys.
        let left_key: i64 = if let Some(line) = left.line {
            line.min(i64::MAX as u64) as i64
        } else {
            -1
        };
        let right_key: i64 = if let Some(line) = right.line {
            line.min(i64::MAX as u64) as i64
        } else {
            -1
        };
        if left_key != right_key {
            return left_key.cmp(&right_key);
        }
        let left_column_key: i64 = if let Some(column) = left.column {
            column.min(i64::MAX as u64) as i64
        } else {
            -1
        };
        let right_column_key: i64 = if let Some(column) = right.column {
            column.min(i64::MAX as u64) as i64
        } else {
            -1
        };
        if left_column_key != right_column_key {
            return left_column_key.cmp(&right_column_key);
        }
        let left_message = left.message.as_str();
        let right_message = right.message.as_str();
        let by_message = utf16_cmp(left_message, right_message);
        if by_message != Ordering::Equal {
            return by_message;
        }
        Ordering::Equal
    });
    let truncated = unique.len() > max_diagnostics;
    unique.truncate(max_diagnostics);
    (unique, truncated)
}

/// A bounded, revision-bound diagnostic set for one workspace document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSet {
    /// Workspace-relative path the set describes.
    pub path: String,
    /// R4 revision handle of the exact source state observed.
    pub revision: Option<String>,
    /// Normalized diagnostics (deterministic order, bounded).
    pub diagnostics: Vec<Diagnostic>,
    /// True when a bound was applied.
    pub truncated: bool,
}

/// Normalize and revision-bind one diagnostic set (per-run bound).
pub fn bind_diagnostic_set(
    path: &str,
    revision: Option<String>,
    diagnostics: Vec<Diagnostic>,
    limits: &LanguageLimits,
) -> DiagnosticSet {
    let (diagnostics, truncated) =
        normalize_diagnostic_set(diagnostics, limits.max_diagnostics_per_run);
    DiagnosticSet { path: path.to_owned(), revision, diagnostics, truncated }
}
