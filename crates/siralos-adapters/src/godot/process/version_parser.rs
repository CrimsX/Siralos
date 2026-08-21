//! Flexible, adversarial-safe parser for Godot `--version` output.
//!
//! Supported forms include `4.7.1.stable.official`, `4.7.2.rc1.official`,
//! `4.8.dev2.custom_build`, patchless versions, and versions carrying a
//! commit-hash token. Non-numeric major/minor values are rejected, empty
//! and non-Godot output fails, control characters are sanitized, and
//! unknown suffixes are preserved rather than failing.

use siralos_core::godot::{GodotVersion, GodotVersionStatus};

/// Failure of a `--version` text parse with its bounded message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotVersionParseFailure {
    /// Bounded failure message.
    pub message: String,
}

const SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;

/// Parse the first line of sanitized `--version` output into an exact
/// [`GodotVersion`].
pub fn parse_godot_version_text(
    raw: &str,
) -> Result<GodotVersion, GodotVersionParseFailure> {
    let sanitized = sanitize_control_characters(raw);
    let first_line = first_line_of(&sanitized).trim().to_owned();
    if first_line.is_empty() {
        return Err(failure("The Godot version output is empty."));
    }
    let body = strip_leading_godot_prefix(&first_line);
    if body.is_empty() {
        return Err(failure("The Godot version output is not recognizable."));
    }
    let segments: Vec<&str> = body.split('.').collect();
    if !is_segment_integer(segments.first().copied())
        || !is_segment_integer(segments.get(1).copied())
    {
        return Err(failure(
            "The Godot version has a non-numeric major or minor.",
        ));
    }
    let major = parse_safe_integer(segments[0])?;
    let minor = parse_safe_integer(segments[1])?;
    let mut rest: Vec<&str> = segments[2..].to_vec();
    let mut patch: Option<u64> = None;
    if !rest.is_empty() && is_segment_integer(Some(rest[0])) {
        patch = rest[0].parse::<u64>().ok();
        rest.remove(0);
    }
    let mut status = GodotVersionStatus::Unknown;
    let mut status_number: Option<u64> = None;
    let mut build: Option<String> = None;
    let mut commit: Option<String> = None;
    for token in rest {
        if status == GodotVersionStatus::Unknown
            && let Some((parsed_status, number)) = match_status_token(token)
        {
            status = parsed_status;
            status_number = number;
            if parsed_status == GodotVersionStatus::Custom && build.is_none() {
                build = Some(token.to_owned());
            }
            continue;
        }
        if commit.is_none() && is_commit_token(token) {
            commit = Some(token.to_owned());
            continue;
        }
        if build.is_none() {
            build = Some(token.to_owned());
        }
    }
    Ok(GodotVersion {
        raw: first_line,
        major,
        minor,
        patch,
        status,
        status_number,
        build,
        commit,
    })
}

fn failure(message: &str) -> GodotVersionParseFailure {
    GodotVersionParseFailure { message: message.to_owned() }
}

fn first_line_of(text: &str) -> &str {
    match text.find('\n') {
        Some(index) => {
            let line = &text[..index];
            line.strip_suffix('\r').unwrap_or(line)
        }
        None => text,
    }
}

fn is_segment_integer(segment: Option<&str>) -> bool {
    segment.is_some_and(|value| {
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn parse_safe_integer(segment: &str) -> Result<u64, GodotVersionParseFailure> {
    match segment.parse::<u64>() {
        Ok(value) if value <= SAFE_INTEGER_MAX => Ok(value),
        _ => Err(failure("The Godot version numbers are out of range.")),
    }
}

fn match_status_token(
    token: &str,
) -> Option<(GodotVersionStatus, Option<u64>)> {
    const CANDIDATES: &[(&str, GodotVersionStatus)] = &[
        ("stable", GodotVersionStatus::Stable),
        ("rc", GodotVersionStatus::Rc),
        ("beta", GodotVersionStatus::Beta),
        ("alpha", GodotVersionStatus::Alpha),
        ("dev", GodotVersionStatus::Dev),
        ("custom_build", GodotVersionStatus::Custom),
        ("custom", GodotVersionStatus::Custom),
    ];
    for (word, parsed_status) in CANDIDATES {
        if let Some(remainder) = token.strip_prefix(*word) {
            if remainder.is_empty() {
                return Some((*parsed_status, None));
            }
            if remainder.bytes().all(|byte| byte.is_ascii_digit()) {
                let number = remainder.parse::<u64>().ok()?;
                return Some((*parsed_status, Some(number)));
            }
        }
    }
    None
}

fn is_commit_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    (7..=40).contains(&bytes.len())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn strip_leading_godot_prefix(line: &str) -> &str {
    if !starts_with_ignore_case(line, 0, "godot") {
        return line;
    }
    let mut position = 5;
    let rest = &line[position..];
    let after_whitespace = skip_whitespace(rest);
    if after_whitespace.len() < rest.len()
        && starts_with_ignore_case(after_whitespace, 0, "engine")
    {
        position += rest.len() - after_whitespace.len() + "engine".len();
    }
    let rest = &line[position..];
    let after_whitespace = skip_whitespace(rest);
    if after_whitespace.len() < rest.len()
        && starts_with_ignore_case(after_whitespace, 0, "v")
    {
        position += rest.len() - after_whitespace.len() + 1;
    }
    let rest = &line[position..];
    let trimmed = skip_whitespace(rest);
    &line[position + (rest.len() - trimmed.len())..]
}

fn starts_with_ignore_case(text: &str, start: usize, word: &str) -> bool {
    let text = &text.as_bytes()[start.min(text.len())..];
    text.len() >= word.len()
        && text[..word.len()].eq_ignore_ascii_case(word.as_bytes())
}

fn skip_whitespace(text: &str) -> &str {
    text.trim_start_matches(char::is_whitespace)
}

/// Replace control characters with U+FFFD except tab, carriage return,
/// and newline.
pub fn sanitize_control_characters(text: &str) -> String {
    text.chars()
        .map(|character| {
            let code = character as u32;
            if code < 0x20 || code == 0x7f {
                if character == '\n' || character == '\r' || character == '\t'
                {
                    character
                } else {
                    '\u{FFFD}'
                }
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_godot_version_text, sanitize_control_characters};
    use siralos_core::godot::GodotVersionStatus as Status;

    #[test]
    fn parses_stable_official() {
        let parsed = parse_godot_version_text("4.7.1.stable.official\n")
            .expect("parses");
        assert_eq!(parsed.major, 4);
        assert_eq!(parsed.minor, 7);
        assert_eq!(parsed.patch, Some(1));
        assert_eq!(parsed.status, Status::Stable);
        assert_eq!(parsed.build.as_deref(), Some("official"));
        assert_eq!(parsed.raw, "4.7.1.stable.official");
    }

    #[test]
    fn parses_rc_and_dev_and_custom_build() {
        let rc = parse_godot_version_text("4.7.2.rc1.official").expect("rc");
        assert_eq!(rc.status, Status::Rc);
        assert_eq!(rc.status_number, Some(1));
        let dev =
            parse_godot_version_text("4.8.dev2.custom_build").expect("dev");
        assert_eq!(dev.status, Status::Dev);
        assert_eq!(dev.status_number, Some(2));
        assert_eq!(dev.build.as_deref(), Some("custom_build"));
    }

    #[test]
    fn parses_patchless_with_commit_token() {
        let parsed = parse_godot_version_text("4.3.0a1b2c3d4e5f6a7")
            .expect("commit form");
        assert_eq!(parsed.patch, None);
        assert_eq!(parsed.commit.as_deref(), Some("0a1b2c3d4e5f6a7"));
    }

    #[test]
    fn strips_leading_godot_prefix() {
        let parsed = parse_godot_version_text("Godot Engine v4.2.stable\n")
            .expect("prefixed");
        assert_eq!(parsed.major, 4);
        assert_eq!(parsed.minor, 2);
    }

    #[test]
    fn preserves_unknown_suffixes_as_build() {
        let parsed = parse_godot_version_text("4.3.mystery.official")
            .expect("unknown suffix");
        assert_eq!(parsed.status, Status::Unknown);
        assert_eq!(parsed.build.as_deref(), Some("mystery"));
        assert_eq!(parsed.commit, None);
    }

    #[test]
    fn rejects_empty_and_unrecognizable_output() {
        let empty = parse_godot_version_text("").unwrap_err();
        assert_eq!(empty.message, "The Godot version output is empty.");
        let blank = parse_godot_version_text("\n\n").unwrap_err();
        assert_eq!(blank.message, "The Godot version output is empty.");
        let bare = parse_godot_version_text("godot\n").unwrap_err();
        assert_eq!(
            bare.message,
            "The Godot version output is not recognizable."
        );
        let prefixed =
            parse_godot_version_text("Godot Engine v\n").unwrap_err();
        assert_eq!(
            prefixed.message,
            "The Godot version output is not recognizable."
        );
    }

    #[test]
    fn undotted_output_reports_non_numeric_major_minor() {
        let alien = parse_godot_version_text("not-a-version").unwrap_err();
        assert_eq!(
            alien.message,
            "The Godot version has a non-numeric major or minor."
        );
    }

    #[test]
    fn rejects_non_numeric_major_minor_and_out_of_range() {
        let bad = parse_godot_version_text("four.7.stable").unwrap_err();
        assert_eq!(
            bad.message,
            "The Godot version has a non-numeric major or minor."
        );
        let huge = parse_godot_version_text("99999999999999999999.7.stable")
            .unwrap_err();
        assert_eq!(
            huge.message,
            "The Godot version numbers are out of range."
        );
    }

    #[test]
    fn sanitizes_control_characters() {
        assert_eq!(
            sanitize_control_characters("a\u{0}b\u{7f}c\td"),
            "a\u{FFFD}b\u{FFFD}c\td"
        );
        assert_eq!(
            sanitize_control_characters("4.3.stable\u{1}"),
            "4.3.stable\u{FFFD}"
        );
    }
}
