//! Conservative normalization of Godot `--check-only` console output.
//!
//! Godot's console output is not a formally versioned machine protocol,
//! so this parser recognizes the stable `ERROR:`/`SCRIPT ERROR:`/
//! `WARNING:`/`SCRIPT WARNING:` prefixes and their `at:` continuation
//! locations, recognizes inline `res://<path>:<line>:<column>` locations,
//! normalizes mirror-absolute paths to workspace-relative paths, preserves
//! unmatched error-like lines as generic diagnostics instead of silently
//! discarding them, never fabricates line/column values, sanitizes control
//! characters and bounds every message, and never classifies a warning as
//! an error unless the engine output explicitly says so.
//!
//! A script parse failure is a VALID diagnostic result; exit-status
//! semantics live in the service, not here.

use siralos_core::language::{
    sanitize_control_characters, truncate_utf8_bytes,
};
use siralos_godot::godot::{
    GODOT_LIMITS, GdScriptDiagnosticSource, GdScriptSeverity,
    GodotGdScriptDiagnostic,
};

/// Input console output of one check-only run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCheckOutputInput<'a> {
    /// Engine stdout.
    pub stdout: &'a str,
    /// Engine stderr.
    pub stderr: &'a str,
    /// Absolute mirror project path; mirror-absolute location prefixes
    /// are normalized away and never leak.
    pub mirror_project_path: Option<&'a str>,
}

/// Normalized bounded diagnostics for one check run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotCheckOutputNormalization {
    /// Diagnostics in engine order.
    pub diagnostics: Vec<GodotGdScriptDiagnostic>,
    /// True when the per-script diagnostic bound was applied.
    pub truncated: bool,
    /// Count of ignored banner/unmatched lines (never silently dropped as
    /// errors).
    pub unmatched_line_count: u64,
}

struct PendingDiagnostic {
    severity: GdScriptSeverity,
    raw_category: Option<String>,
    message: String,
    location: Option<(String, Option<u32>, Option<u32>)>,
}

/// Normalize the combined stdout/stderr of one check-only run.
pub fn normalize_godot_check_output(
    input: GodotCheckOutputInput<'_>,
) -> GodotCheckOutputNormalization {
    normalize_with_limits(input, GODOT_LIMITS.max_diagnostics_per_script)
}

/// Normalize with explicit diagnostic bounds.
pub fn normalize_with_limits(
    input: GodotCheckOutputInput<'_>,
    max_diagnostics: usize,
) -> GodotCheckOutputNormalization {
    let combined = format!("{}\n{}", input.stdout, input.stderr);
    let mut diagnostics: Vec<GodotGdScriptDiagnostic> = Vec::new();
    let mut pending: Option<PendingDiagnostic> = None;
    let mut unmatched_line_count: u64 = 0;
    let mut overflowed = false;
    for raw_line in split_lines(&combined) {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(current) = &mut pending {
            if trimmed.starts_with("at:") {
                if current.location.is_none()
                    && let Some(location) =
                        extract_location(trimmed, input.mirror_project_path)
                {
                    current.location = Some(location);
                }
                continue;
            }
        }
        if pending.is_some() {
            flush(&mut pending, &mut diagnostics, input.mirror_project_path);
        }
        let entry = match_prefixed(trimmed, input.mirror_project_path)
            .or_else(|| {
                match_inline_location(trimmed, input.mirror_project_path)
            })
            .or_else(|| match_generic(trimmed));
        match entry {
            Some(entry) => {
                if diagnostics.len() >= max_diagnostics {
                    overflowed = true;
                    flush(
                        &mut pending,
                        &mut diagnostics,
                        input.mirror_project_path,
                    );
                } else {
                    pending = Some(entry);
                }
            }
            None => unmatched_line_count += 1,
        }
    }
    flush(&mut pending, &mut diagnostics, input.mirror_project_path);
    let truncated = overflowed || diagnostics.len() > max_diagnostics;
    diagnostics.truncate(max_diagnostics);
    GodotCheckOutputNormalization {
        diagnostics,
        truncated,
        unmatched_line_count,
    }
}

fn flush(
    pending: &mut Option<PendingDiagnostic>,
    diagnostics: &mut Vec<GodotGdScriptDiagnostic>,
    mirror_project_path: Option<&str>,
) {
    if let Some(entry) = pending.take() {
        diagnostics.push(to_diagnostic(entry, mirror_project_path));
    }
}

fn to_diagnostic(
    pending: PendingDiagnostic,
    mirror_project_path: Option<&str>,
) -> GodotGdScriptDiagnostic {
    let code = extract_code(&pending.message);
    let message = bound_message(&pending.message, mirror_project_path);
    GodotGdScriptDiagnostic {
        source: GdScriptDiagnosticSource::CheckOnly,
        severity: pending.severity,
        path: pending.location.as_ref().map(|(path, _, _)| path.clone()),
        line: pending.location.as_ref().and_then(|(_, line, _)| *line),
        column: pending.location.as_ref().and_then(|(_, _, column)| *column),
        code,
        message,
        raw_category: pending.raw_category,
    }
}

fn match_prefixed(
    trimmed: &str,
    mirror_project_path: Option<&str>,
) -> Option<PendingDiagnostic> {
    const PREFIXES: [(&str, &str); 4] = [
        ("SCRIPT ERROR", "script-error"),
        ("SCRIPT WARNING", "script-warning"),
        ("ERROR", "error"),
        ("WARNING", "warning"),
    ];
    for (prefix, raw_category) in PREFIXES {
        let bytes = trimmed.as_bytes();
        if bytes.len() > prefix.len()
            && bytes[prefix.len()] == b':'
            && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
            let rest = &trimmed[prefix.len() + 1..];
            let message = rest.strip_prefix([' ', '\t']).unwrap_or(rest);
            let severity = if prefix.contains("WARNING") {
                GdScriptSeverity::Warning
            } else {
                GdScriptSeverity::Error
            };
            let mut final_message = message.to_owned();
            let mut location = None;
            if let Some(span) = inline_parenthesized_res_suffix(message) {
                if let Some(found) = extract_location(
                    &message[span.clone()],
                    mirror_project_path,
                ) {
                    location = Some(found);
                    final_message = message[..span.start].trim().to_owned();
                }
            }
            return Some(PendingDiagnostic {
                severity,
                raw_category: Some(raw_category.to_owned()),
                message: final_message,
                location,
            });
        }
    }
    None
}

fn match_inline_location(
    trimmed: &str,
    mirror_project_path: Option<&str>,
) -> Option<PendingDiagnostic> {
    let message = split_inline_location(trimmed)?;
    let location = extract_location(trimmed, mirror_project_path)?;
    let severity = if starts_with_ci(message, "warning") {
        GdScriptSeverity::Warning
    } else if starts_with_ci(message, "error") {
        GdScriptSeverity::Error
    } else {
        GdScriptSeverity::Unknown
    };
    let raw_category = match severity {
        GdScriptSeverity::Unknown => None,
        GdScriptSeverity::Error => Some("error".to_owned()),
        GdScriptSeverity::Warning => Some("warning".to_owned()),
        GdScriptSeverity::Info => None,
    };
    Some(PendingDiagnostic {
        severity,
        raw_category,
        message: message.to_owned(),
        location: Some(location),
    })
}

fn match_generic(trimmed: &str) -> Option<PendingDiagnostic> {
    if contains_word_ci(trimmed, "error") {
        return Some(PendingDiagnostic {
            severity: GdScriptSeverity::Unknown,
            raw_category: None,
            message: trimmed.to_owned(),
            location: None,
        });
    }
    if contains_word_ci(trimmed, "warning") {
        return Some(PendingDiagnostic {
            severity: GdScriptSeverity::Warning,
            raw_category: Some("warning".to_owned()),
            message: trimmed.to_owned(),
            location: None,
        });
    }
    None
}

/// Find `<mirror|res://|parenthesized> leaf.gd :N(:M)?` in text; returns
/// `(workspace-relative path, line, column)`.
fn extract_location(
    text: &str,
    mirror_project_path: Option<&str>,
) -> Option<(String, Option<u32>, Option<u32>)> {
    if let Some(mirror) = mirror_project_path
        && !mirror.is_empty()
        && let Some(found) = find_mirror_location(text, mirror)
    {
        return Some(found);
    }
    if let Some(found) = find_res_location(text) {
        return Some(found);
    }
    find_parenthesized_location(text)
}

fn find_mirror_location(
    text: &str,
    mirror: &str,
) -> Option<(String, Option<u32>, Option<u32>)> {
    let needle: Vec<u8> = mirror
        .bytes()
        .map(|byte| if byte == b'\\' { b'/' } else { byte })
        .collect();
    let hay = text.as_bytes();
    for start in 0..hay.len() {
        if !matches_slash_tolerant(hay, start, &needle) {
            continue;
        }
        let mut cursor = start + needle.len();
        if cursor >= hay.len()
            || !(hay[cursor] == b'/' || hay[cursor] == b'\\')
        {
            continue;
        }
        cursor += 1;
        let leaf_end = leaf_end_from(hay, cursor);
        if !is_script_leaf(&text[cursor..leaf_end]) {
            continue;
        }
        let path = normalize_leaf(&text[cursor..leaf_end]);
        if let Some(position) = parse_position(hay, leaf_end) {
            return Some((path, position.0, position.1));
        }
    }
    None
}

fn find_res_location(
    text: &str,
) -> Option<(String, Option<u32>, Option<u32>)> {
    const PREFIX: &[u8] = b"res://";
    let hay = text.as_bytes();
    for start in find_all_case_insensitive(hay, PREFIX) {
        let cursor = start + PREFIX.len();
        let leaf_end = leaf_end_from(hay, cursor);
        if !is_script_leaf(&text[cursor..leaf_end]) {
            continue;
        }
        let path = normalize_leaf(&text[cursor..leaf_end]);
        if let Some(position) = parse_position(hay, leaf_end) {
            return Some((path, position.0, position.1));
        }
    }
    None
}

fn find_parenthesized_location(
    text: &str,
) -> Option<(String, Option<u32>, Option<u32>)> {
    let hay = text.as_bytes();
    for open in 0..hay.len() {
        if hay[open] != b'(' {
            continue;
        }
        let cursor = open + 1;
        let leaf_end = leaf_end_from(hay, cursor);
        if !is_script_leaf(&text[cursor..leaf_end]) {
            continue;
        }
        let close = text[leaf_end..].find(')');
        if let Some(position) = parse_position(hay, leaf_end) {
            let within_parens = close.is_some_and(|relative| {
                text[leaf_end..leaf_end + relative]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b':')
            });
            if within_parens {
                return Some((
                    normalize_leaf(&text[cursor..leaf_end]),
                    position.0,
                    position.1,
                ));
            }
        }
    }
    None
}

fn matches_slash_tolerant(hay: &[u8], start: usize, needle: &[u8]) -> bool {
    hay.len() >= start + needle.len()
        && hay[start..start + needle.len()].iter().zip(needle).all(
            |(left, right)| {
                left.eq_ignore_ascii_case(right)
                    || (*left == b'\\' && *right == b'/')
                    || (*left == b'/' && *right == b'\\')
            },
        )
}

fn find_all_case_insensitive(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    for start in 0..hay.len() {
        if hay[start..].len() >= needle.len()
            && hay[start..start + needle.len()]
                .iter()
                .zip(needle)
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        {
            found.push(start);
        }
    }
    found
}

fn leaf_end_from(hay: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < hay.len()
        && !matches!(hay[end], b'\t' | b' ' | b':' | b'(' | b')')
    {
        end += 1;
    }
    end
}

fn is_script_leaf(leaf: &str) -> bool {
    !leaf.is_empty() && leaf.to_lowercase().ends_with(".gd")
}

fn normalize_leaf(leaf: &str) -> String {
    leaf.replace('\\', "/")
}

/// Parse `:(\d+)(?::(\d+))?` starting exactly at `position`.
fn parse_position(
    hay: &[u8],
    position: usize,
) -> Option<(Option<u32>, Option<u32>)> {
    let mut cursor = position;
    if cursor >= hay.len() || hay[cursor] != b':' {
        return None;
    }
    cursor += 1;
    let line_start = cursor;
    while cursor < hay.len() && hay[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == line_start {
        return None;
    }
    let line = text_number(&hay[line_start..cursor])?;
    let mut column = None;
    if cursor < hay.len() && hay[cursor] == b':' {
        let column_start = cursor + 1;
        let mut end = column_start;
        while end < hay.len() && hay[end].is_ascii_digit() {
            end += 1;
        }
        if end > column_start {
            column = text_number(&hay[column_start..end]);
        }
    }
    Some((Some(line), column))
}

fn text_number(bytes: &[u8]) -> Option<u32> {
    std::str::from_utf8(bytes).ok().and_then(|text| text.parse::<u32>().ok())
}

/// Match a trailing `(res://…)` suffix on a prefixed message
/// (case-sensitive `res://`, like the reference); returns the span from
/// the opening parenthesis to the end of the closing parenthesis run.
fn inline_parenthesized_res_suffix(
    message: &str,
) -> Option<std::ops::Range<usize>> {
    let trimmed_end = message.trim_end().len();
    if trimmed_end < "(res://x)".len() || !message.ends_with(')') {
        return None;
    }
    let bytes = message.as_bytes();
    let mut depth = 0usize;
    for index in (0..trimmed_end).rev() {
        match bytes[index] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    let inner = &message[index + 1..trimmed_end];
                    if inner.starts_with("res://") && !inner.contains(')') {
                        return Some(index..trimmed_end);
                    }
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

/// Validate `<res://path|path>:N(:M)? - message`; returns the message
/// when the left side has the inline-location shape. The actual location
/// values are extracted separately from the whole line.
fn split_inline_location(trimmed: &str) -> Option<&str> {
    let dash = find_dash_separator(trimmed)?;
    let left = &trimmed[..dash];
    let after = &trimmed[dash + 1..];
    let message = after.trim_start();
    if message.is_empty() || left.is_empty() {
        return None;
    }
    let body = if starts_with_ci(left, "res://") {
        &left["res://".len()..]
    } else if left.contains("://") {
        return None;
    } else {
        left
    };
    let colon = body.find(':')?;
    let leaf = &body[..colon];
    if leaf.is_empty() || leaf.chars().any(char::is_whitespace) {
        return None;
    }
    parse_position(body.as_bytes(), colon)?;
    Some(message)
}

/// Find `\s+-\s+`; returns the byte offset of the `-` with trailing
/// whitespace consumed into the separator.
fn find_dash_separator(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    for index in 0..bytes.len() {
        if bytes[index] != b'-' {
            continue;
        }
        let before = index > 0
            && (bytes[index - 1] == b' ' || bytes[index - 1] == b'\t');
        let mut end = index + 1;
        while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t')
        {
            end += 1;
        }
        if before && end > index + 1 {
            return Some(index);
        }
    }
    None
}

fn starts_with_ci(message: &str, word: &str) -> bool {
    message.len() >= word.len()
        && message[..word.len()].eq_ignore_ascii_case(word)
}

/// Case-insensitive whole-word search with ASCII word boundaries
/// (`[A-Za-z0-9_]` counts as word characters).
fn contains_word_ci(text: &str, word: &str) -> bool {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let pattern = word.as_bytes();
    let is_word_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut start = 0;
    while let Some(relative) = lower[start..].find(word) {
        let begin = start + relative;
        let end = begin + pattern.len();
        let boundary_before = begin == 0 || !is_word_byte(bytes[begin - 1]);
        let boundary_after = end >= bytes.len() || !is_word_byte(bytes[end]);
        if boundary_before && boundary_after {
            return true;
        }
        start = begin + 1;
    }
    false
}

/// Stable diagnostic code extraction: an optional `Parse Error:` prefix
/// followed by an undeclared-identifier report wins; otherwise any
/// leading `Parse Error` becomes `parse-error`.
fn extract_code(message: &str) -> Option<String> {
    let lowered = message.to_lowercase();
    let body = lowered
        .strip_prefix("parse error:")
        .map(str::trim_start)
        .unwrap_or(&lowered);
    if body.starts_with("identifier ") && body.contains(" not declared") {
        return Some("undeclared-identifier".to_owned());
    }
    if lowered.starts_with("parse error") {
        return Some("parse-error".to_owned());
    }
    None
}

fn bound_message(message: &str, mirror_project_path: Option<&str>) -> String {
    let mut text = message.to_owned();
    if let Some(mirror) = mirror_project_path
        && !mirror.is_empty()
    {
        text = text.replace(mirror, "<mirror>");
    }
    let sanitized = sanitize_control_characters(&text);
    truncate_utf8_bytes(
        sanitized.trim(),
        GODOT_LIMITS.max_diagnostic_message_bytes,
    )
}

fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line))
}

#[cfg(test)]
mod tests {
    use super::{
        GodotCheckOutputInput, GodotGdScriptDiagnostic,
        normalize_godot_check_output, normalize_with_limits,
    };
    use siralos_godot::godot::{GODOT_LIMITS, GdScriptSeverity};

    fn normalize(
        stdout: &str,
        mirror: Option<&str>,
    ) -> super::GodotCheckOutputNormalization {
        normalize_godot_check_output(GodotCheckOutputInput {
            stdout,
            stderr: "",
            mirror_project_path: mirror,
        })
    }

    #[test]
    fn parser_error_with_at_location_normalizes() {
        let mirror = if cfg!(windows) {
            "C:\\tmp\\siralos-mirror-1"
        } else {
            "/tmp/siralos-mirror-1"
        };
        let output = "Godot Engine v4.7.1.stable.official\n\
             ERROR: Parse Error: Expected \"end of file\" after declaration.\n\
             at: GDScript::reload (modules/gdscript/gdscript.cpp:1205)\n";
        let result = normalize(output, Some(mirror));
        assert_eq!(result.diagnostics.len(), 1);
        let diagnostic = &result.diagnostics[0];
        assert_eq!(diagnostic.severity, GdScriptSeverity::Error);
        assert_eq!(diagnostic.code.as_deref(), Some("parse-error"));
        assert_eq!(diagnostic.raw_category.as_deref(), Some("error"));
        assert_eq!(diagnostic.line, None);
        let serialized = format!("{result:?}");
        assert!(!serialized.contains("gdscript.cpp"));
        assert!(!serialized.contains(mirror));
    }

    #[test]
    fn undeclared_identifier_gets_stable_code() {
        let result = normalize(
            "SCRIPT ERROR: Parse Error: Identifier \"velocityy\" not declared in the current scope.\n",
            None,
        );
        assert_eq!(
            result.diagnostics[0].code.as_deref(),
            Some("undeclared-identifier")
        );
        assert!(result.diagnostics[0].message.contains("\"velocityy\""));
    }

    #[test]
    fn warnings_never_become_errors() {
        let result = normalize(
            "SCRIPT WARNING: The integer division operator is deprecated.\n",
            None,
        );
        assert_eq!(result.diagnostics[0].severity, GdScriptSeverity::Warning);
        assert_eq!(
            result.diagnostics[0].raw_category.as_deref(),
            Some("script-warning")
        );
    }

    #[test]
    fn multiple_diagnostics_preserve_order() {
        let result =
            normalize("ERROR: one\nERROR: two\nWARNING: three\n", None);
        assert_eq!(result.diagnostics.len(), 3);
        let severities: Vec<GdScriptSeverity> =
            result.diagnostics.iter().map(|entry| entry.severity).collect();
        assert_eq!(
            severities,
            [
                GdScriptSeverity::Error,
                GdScriptSeverity::Error,
                GdScriptSeverity::Warning
            ]
        );
    }

    #[test]
    fn inline_res_locations_extract_line_and_column() {
        let result = normalize(
            "res://src/player/player.gd:34:17 - Identifier \"velocityy\" not declared in the current scope.\n\
             res://src/player/player.gd:81:9 - Error checking script.\n",
            None,
        );
        assert_eq!(result.diagnostics.len(), 2);
        assert_eq!(
            result.diagnostics[0].path.as_deref(),
            Some("src/player/player.gd")
        );
        assert_eq!(result.diagnostics[0].line, Some(34));
        assert_eq!(result.diagnostics[0].column, Some(17));
        assert_eq!(result.diagnostics[0].severity, GdScriptSeverity::Unknown);
        assert_eq!(result.diagnostics[1].line, Some(81));
        assert_eq!(result.diagnostics[1].severity, GdScriptSeverity::Error);
    }

    #[test]
    fn unmatched_error_like_lines_stay_generic_and_counted() {
        let result = normalize(
            "Godot Engine v4.7.1.stable.official\nsomething weird with error inside\n",
            None,
        );
        let generic = result
            .diagnostics
            .iter()
            .find(|entry| entry.message.contains("weird"))
            .expect("generic preserved");
        assert_eq!(generic.severity, GdScriptSeverity::Unknown);
        assert_eq!(generic.raw_category, None);
        assert_eq!(generic.line, None);
        assert!(result.unmatched_line_count > 0);
    }

    #[test]
    fn control_characters_are_sanitized() {
        let result = normalize(
            "ERROR: Parse Error: Control \u{1b}[31mcharacter\u{0} bad.\n",
            None,
        );
        let message = &result.diagnostics[0].message;
        assert!(!message.contains('\u{1b}'));
        assert!(!message.contains('\u{0}'));
    }

    #[test]
    fn mirror_roots_scrubbed_from_message_bodies() {
        let mirror = if cfg!(windows) {
            "C:\\tmp\\siralos-mirror-1"
        } else {
            "/tmp/siralos-mirror-1"
        };
        let output = format!(
            "ERROR: Parse Error: cannot load {mirror}/src/player/player.gd\n"
        );
        let result = normalize(&output, Some(mirror));
        let serialized = format!("{result:?}");
        assert!(!serialized.contains(mirror));
        assert!(result.diagnostics[0].message.contains("<mirror>"));
    }

    #[test]
    fn never_leaks_mirror_absolute_at_locations() {
        let mirror = if cfg!(windows) {
            "C:\\tmp\\siralos-mirror-1"
        } else {
            "/tmp/siralos-mirror-1"
        };
        let output = format!(
            "ERROR: Parse Error: boom.\n   at: {mirror}/src/player/player.gd:34:17\n"
        );
        let result = normalize(&output, Some(mirror));
        let serialized = format!("{result:?}");
        assert!(!serialized.contains(mirror));
        assert_eq!(
            result.diagnostics[0].path.as_deref(),
            Some("src/player/player.gd")
        );
        assert_eq!(result.diagnostics[0].line, Some(34));
        assert_eq!(result.diagnostics[0].column, Some(17));
    }

    #[test]
    fn diagnostics_are_bounded_with_explicit_truncation() {
        let many: Vec<String> = (0..GODOT_LIMITS.max_diagnostics_per_script
            + 10)
            .map(|index| format!("ERROR: Parse Error: issue {index}."))
            .collect();
        let output = many.join("\n");
        let result = normalize(&output, None);
        assert_eq!(
            result.diagnostics.len(),
            GODOT_LIMITS.max_diagnostics_per_script
        );
        assert!(result.truncated);
    }

    #[test]
    fn messages_are_individually_bounded() {
        let huge = format!(
            "ERROR: {}",
            "y".repeat(GODOT_LIMITS.max_diagnostic_message_bytes + 1024)
        );
        let result = normalize(&huge, None);
        assert!(
            result.diagnostics[0].message.len()
                <= GODOT_LIMITS.max_diagnostic_message_bytes
        );
    }

    #[test]
    fn explicit_limits_override_the_default_bound() {
        let output = "ERROR: one\nERROR: two\nERROR: three\n";
        let result = normalize_with_limits(
            GodotCheckOutputInput {
                stdout: output,
                stderr: "",
                mirror_project_path: None,
            },
            2,
        );
        assert_eq!(result.diagnostics.len(), 2);
        assert!(result.truncated);
        let _ = std::mem::size_of::<GodotGdScriptDiagnostic>();
    }
}
