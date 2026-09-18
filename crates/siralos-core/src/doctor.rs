//! Doctor domain model (Stage 3R R13.1).
//!
//! Deterministic, read-only, offline diagnostic vocabulary mirrored from
//! the TypeScript reference: typed areas and statuses, canonical request
//! normalization, report counts and exit codes, the safe/public report
//! sanitizer, the bounded self-reference revision fingerprint, and the
//! configuration-surface summary. Checks carry structured status; the
//! doctor never repairs anything.

use crate::identity::{canonicalize_json, sha256_hex_str};
use serde_json::Value;
use serde_json::json;

/// Version of the doctor report JSON schema.
pub const DOCTOR_SCHEMA_VERSION: u64 = 1;

/// Every doctor area, in canonical order.
pub const DOCTOR_AREAS: [&str; 12] = [
    "runtime",
    "configuration",
    "providers",
    "sandbox",
    "workspace",
    "godot",
    "project",
    "references",
    "research",
    "capabilities",
    "determinism",
    "readiness",
];

/// Typed invocation failure marker (unknown area) ??? exit code 2 territory.
pub const DOCTOR_INVOCATION_ERROR: &str = "doctor_invocation";

/// One doctor check result: structured status, never prose-only.
#[derive(Debug, Clone)]
pub struct DoctorCheckResult {
    /// The stable check id.
    pub id: String,
    /// The owning doctor area.
    pub area: &'static str,
    /// One of `pass | warn | fail | skip`.
    /// `pass | warn | fail | skip`.
    pub status: &'static str,
    /// The human-readable check summary.
    pub summary: String,
}

/// Normalize a requested-area list: unknown areas fail with the
/// invocation error code, empty means every area, and the result is
/// deduplicated into canonical area order.
pub fn normalize_doctor_request(
    areas: &[&str],
) -> Result<Vec<&'static str>, &'static str> {
    for area in areas {
        if !DOCTOR_AREAS.contains(area) {
            return Err(DOCTOR_INVOCATION_ERROR);
        }
    }
    if areas.is_empty() {
        return Ok(DOCTOR_AREAS.to_vec());
    }
    Ok(DOCTOR_AREAS
        .iter()
        .copied()
        .filter(|canonical| areas.contains(canonical))
        .collect())
}

/// Deterministic per-status counts over a check set.
pub fn count_doctor_report(checks: &[DoctorCheckResult]) -> Value {
    let mut pass = 0u64;
    let mut warn = 0u64;
    let mut fail = 0u64;
    let mut skip = 0u64;
    for check in checks {
        match check.status {
            "pass" => pass += 1,
            "warn" => warn += 1,
            "fail" => fail += 1,
            _ => skip += 1,
        }
    }
    json!({ "pass": pass, "warn": warn, "fail": fail, "skip": skip, "total": checks.len() })
}

/// Exit-code contract: 0 = no failures, 1 = any failure. Warnings never
/// fail; invocation failures are reported by the caller as code 2.
pub fn doctor_exit_code_for(counts: &Value) -> u64 {
    if counts["fail"].as_u64().unwrap_or(0) > 0 { 1 } else { 0 }
}

struct Scanner<'text> {
    chars: Vec<char>,
    position: usize,
    replacement: &'text str,
}

impl<'text> Scanner<'text> {
    fn new(text: &'text str, replacement: &'text str) -> Self {
        Self { chars: text.chars().collect(), position: 0, replacement }
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.position + offset).copied()
    }

    fn starts_with(&self, prefix: &str) -> bool {
        prefix
            .chars()
            .enumerate()
            .all(|(offset, expected)| self.peek(offset) == Some(expected))
    }

    fn consume_while(&mut self, accept: impl Fn(char) -> bool) {
        while let Some(current) = self.peek(0) {
            if !accept(current) {
                break;
            }
            self.position += 1;
        }
    }

    fn consume_nonspace_quote_run(&mut self) {
        self.consume_while(|current| {
            !current.is_whitespace() && current != '"' && current != '\''
        });
    }

    /// Left-to-right global replace driven by a per-position matcher:
    /// when the matcher accepts, the replacement is emitted and scanning
    /// resumes after the consumed span; otherwise one char is copied.
    fn replace_with(
        mut self,
        mut matcher: impl FnMut(&mut Self) -> bool,
    ) -> String {
        let mut out = String::with_capacity(self.chars.len());
        while self.position < self.chars.len() {
            let start = self.position;
            if matcher(&mut self) {
                debug_assert!(self.position > start, "matcher must consume");
                out.push_str(self.replacement);
            } else {
                self.position += 1;
                out.push(self.chars[start]);
            }
        }
        out
    }
}

fn apply_scanner(
    text: &str,
    replacement: &str,
    matcher: impl FnMut(&mut Scanner) -> bool,
) -> String {
    Scanner::new(text, replacement).replace_with(matcher)
}

/// `[A-Za-z]:[\\/]` followed by a nonspace/quote run (both separators).
fn drive_path_matcher(scanner: &mut Scanner) -> bool {
    let matches_drive = matches!(scanner.peek(0), Some(first) if first.is_ascii_alphabetic())
        && scanner.peek(1) == Some(':')
        && matches!(scanner.peek(2), Some('\\') | Some('/'));
    if !matches_drive {
        return false;
    }
    scanner.position += 3;
    scanner.consume_nonspace_quote_run();
    true
}

const COMMON_ROOTS: [&str; 15] = [
    "Users",
    "home",
    "tmp",
    "var",
    "etc",
    "usr",
    "opt",
    "mnt",
    "media",
    "run",
    "srv",
    "root",
    "workspaces",
    "app",
    "data",
];

/// `/common-root` optionally followed by `/nonspace-run`. The reference
/// alternation requires no trailing boundary, so `/apps/x` shortens to
/// the `app` root plus a literal remainder.
fn common_root_matcher(scanner: &mut Scanner) -> bool {
    if scanner.peek(0) != Some('/') {
        return false;
    }
    let matched = COMMON_ROOTS.iter().find_map(|root| {
        let length = root.chars().count() + 1;
        scanner.starts_with(&format!("/{root}")).then_some(length)
    });
    match matched {
        None => false,
        Some(consumed) => {
            scanner.position += consumed;
            if scanner.peek(0) == Some('/') {
                scanner.position += 1;
                scanner.consume_nonspace_quote_run();
            }
            true
        }
    }
}

const SEGMENT_BYTE: fn(char) -> bool = |current: char| {
    current.is_ascii_alphanumeric() || matches!(current, '_' | '.' | '-')
};

/// `/segment/segment/...` with two or more segments, then a final
/// nonspace/quote run.
fn multi_segment_matcher(scanner: &mut Scanner) -> bool {
    if scanner.peek(0) != Some('/') {
        return false;
    }
    let start = scanner.position;
    scanner.position += 1;
    let mut segments = 0usize;
    loop {
        scanner.consume_while(SEGMENT_BYTE);
        if scanner.peek(0) != Some('/') {
            scanner.position = start;
            return false;
        }
        scanner.position += 1;
        segments += 1;
        if !scanner.peek(0).is_some_and(SEGMENT_BYTE) {
            break;
        }
    }
    if segments < 2 {
        scanner.position = start;
        return false;
    }
    scanner.consume_nonspace_quote_run();
    true
}

/// UNC share: `\\host\rest`.
fn unc_matcher(scanner: &mut Scanner) -> bool {
    if scanner.peek(0) != Some('\\') || scanner.peek(1) != Some('\\') {
        return false;
    }
    let start = scanner.position;
    scanner.position += 2;
    scanner.consume_while(|current| {
        current.is_ascii_alphanumeric() || matches!(current, '_' | '.' | '-')
    });
    if scanner.peek(0) == Some('\\') {
        scanner.position += 1;
        scanner.consume_nonspace_quote_run();
        true
    } else {
        scanner.position = start;
        false
    }
}

/// Home shorthand: `~` plus a nonspace/quote run.
fn tilde_matcher(scanner: &mut Scanner) -> bool {
    if scanner.peek(0) != Some('~') {
        return false;
    }
    scanner.position += 1;
    scanner.consume_nonspace_quote_run();
    true
}

/// The single secret-redaction owner: the frozen reference's `SECRET_PATTERNS`
/// list from `packages/core/src/doctor/safe-report.ts` at `5da5cde`, applied as
/// its six ordered whole-string passes.
///
/// The reference is JavaScript, and its semantics are narrower than the
/// patterns suggest:
///
/// - `\b` is ASCII-only and zero-width, so it holds where exactly one side is
///   `[A-Za-z0-9_]` — true at a string edge adjacent to a word character, false
///   at an edge adjacent to a non-word character — and two matches may share one
///   boundary;
/// - every quantifier is greedy with backtracking, so a class run gives
///   characters back from its right end until the trailing `\b` holds, and the
///   base64 padding `={0,2}` gives padding back the same way;
/// - `Bearer` carries the `i` flag on the literal only, then JavaScript `\s`
///   (`is_js_space`), then the token class.
///
/// Later passes see earlier passes' output, so the emitted
/// `SECRET_REPLACEMENT` is re-scanned; `no_rule_can_match_inside_the_replacement`
/// asserts it stays inert rather than assuming it.
fn redact_secrets(text: &str) -> String {
    let sanitized = replace_pass(text, find_sk_key);
    let sanitized = replace_pass(&sanitized, find_aws_key);
    let sanitized = replace_pass(&sanitized, find_github_token);
    let sanitized = replace_pass(&sanitized, find_bearer_token);
    let sanitized = replace_pass(&sanitized, find_long_hex_run);
    replace_pass(&sanitized, find_long_base64_run)
}

/// The marker every rule emits in place of a matched token.
const SECRET_REPLACEMENT: &str = "<secret>";

/// A half-open character range, `[start, end)`, holding one match.
type Span = (usize, usize);

/// One rule, in the shape `replace_pass` drives it: the next match at or after
/// the index it is offered.
type Rule = fn(&[char], usize) -> Option<Span>;

/// One global, non-overlapping, leftmost-first pass — the behaviour of
/// `String.prototype.replace` with a `/g` pattern.
///
/// `find` is offered the first index the pass may still match at and returns the
/// next match at or after it. Scanning resumes at a match's end rather than one
/// character past it, because the reference's `\b` is zero-width and a match may
/// therefore begin exactly where the previous one ended.
fn replace_pass(text: &str, find: Rule) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some((start, end)) = find(&chars, cursor) {
        // A rule that matched nothing would leave `cursor` where it was and spin
        // this loop forever, so the invariant is enforced rather than assumed.
        assert!(
            end > start,
            "a secret rule must consume at least one character"
        );
        out.extend(chars[cursor..start].iter().copied());
        out.push_str(SECRET_REPLACEMENT);
        cursor = end;
    }
    out.extend(chars[cursor..].iter().copied());
    out
}

/// `[A-Za-z0-9_]`: the JavaScript `\w` set, and ASCII-only like it.
fn is_word_char(current: char) -> bool {
    current.is_ascii_alphanumeric() || current == '_'
}

/// JavaScript `\b` at `index`.
///
/// Zero-width and ASCII-only: true where exactly one side is a word character,
/// which makes it true at a string edge adjacent to a word character and false
/// at an edge adjacent to a non-word character.
fn boundary_at(chars: &[char], index: usize) -> bool {
    let before = index > 0 && is_word_char(chars[index - 1]);
    let after = index < chars.len() && is_word_char(chars[index]);
    before != after
}

/// JavaScript `\s`, which is neither `char::is_whitespace` nor
/// `char::is_ascii_whitespace`.
///
/// It includes U+00A0, U+1680, U+2000..=U+200A, U+2028, U+2029, U+202F, U+205F,
/// U+3000 and U+FEFF, and excludes U+0085. Both predicates the two former
/// implementations used got at least one of those wrong.
fn is_js_space(current: char) -> bool {
    matches!(
        current,
        '\u{9}'..='\u{d}'
            | ' '
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    )
}

/// Whether `chars[index..]` starts with `literal`.
fn starts_with(chars: &[char], index: usize, literal: &str) -> bool {
    literal
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(index + offset) == Some(&expected))
}

/// The same, case-insensitively: the reference gives `Bearer` the `i` flag.
fn starts_with_ignore_case(
    chars: &[char],
    index: usize,
    literal: &str,
) -> bool {
    literal.chars().enumerate().all(|(offset, expected)| {
        chars
            .get(index + offset)
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&expected))
    })
}

/// Consume the longest `accept` run at `from`, then give characters back from
/// its right end until the trailing `\b` holds, failing once it is shorter than
/// `minimum`. That is a greedy quantifier plus the reference's backtracking.
fn class_run(
    chars: &[char],
    from: usize,
    minimum: usize,
    accept: fn(char) -> bool,
) -> Option<usize> {
    let mut end = from;
    while end < chars.len() && accept(chars[end]) {
        end += 1;
    }
    while end >= from + minimum {
        if boundary_at(chars, end) {
            return Some(end);
        }
        end -= 1;
    }
    None
}

/// `[A-Za-z0-9_-]`: the `sk-` key body.
fn is_key_body(current: char) -> bool {
    current.is_ascii_alphanumeric() || current == '_' || current == '-'
}

/// `[A-Za-z0-9_]`: the `gh[pso]_` token body.
fn is_token_body(current: char) -> bool {
    current.is_ascii_alphanumeric() || current == '_'
}

/// `[A-Za-z0-9._~+/=-]`: the `Bearer` token body.
fn is_bearer_body(current: char) -> bool {
    current.is_ascii_alphanumeric()
        || matches!(current, '.' | '_' | '~' | '+' | '/' | '=' | '-')
}

/// `[A-Za-z0-9+/]`: the base64 body, which admits no `-`, `.` or `_`.
fn is_base64_body(current: char) -> bool {
    current.is_ascii_alphanumeric() || current == '+' || current == '/'
}

/// The reference's first pattern, `\bsk-[A-Za-z0-9_-]{8,}\b`.
fn find_sk_key(chars: &[char], from: usize) -> Option<Span> {
    let mut start = from;
    while start + 3 <= chars.len() {
        if boundary_at(chars, start) && starts_with(chars, start, "sk-") {
            if let Some(end) = class_run(chars, start + 3, 8, is_key_body) {
                return Some((start, end));
            }
        }
        start += 1;
    }
    None
}

/// The reference's second pattern, `\bAKIA[0-9A-Z]{16}\b`.
///
/// The body is fixed-width, so there is no run to give back; `{16}` also means a
/// longer uppercase run cannot match at all.
fn find_aws_key(chars: &[char], from: usize) -> Option<Span> {
    let mut start = from;
    while start + 20 <= chars.len() {
        let end = start + 20;
        if boundary_at(chars, start)
            && starts_with(chars, start, "AKIA")
            && chars[start + 4..end].iter().all(|current| {
                current.is_ascii_digit() || current.is_ascii_uppercase()
            })
            && boundary_at(chars, end)
        {
            return Some((start, end));
        }
        start += 1;
    }
    None
}

/// The reference's third pattern, `\bgh[pso]_[A-Za-z0-9_]{20,}\b`.
fn find_github_token(chars: &[char], from: usize) -> Option<Span> {
    let mut start = from;
    while start + 4 <= chars.len() {
        if boundary_at(chars, start)
            && starts_with(chars, start, "gh")
            && matches!(chars[start + 2], 'p' | 's' | 'o')
            && chars[start + 3] == '_'
        {
            if let Some(end) = class_run(chars, start + 4, 20, is_token_body) {
                return Some((start, end));
            }
        }
        start += 1;
    }
    None
}

/// The reference's fourth pattern, `\bBearer\s+[A-Za-z0-9._~+/=-]{12,}\b`.
///
/// `\s+` is greedy, and giving a space back cannot help: the token class admits
/// no whitespace, so a shorter run would have to start on a character the class
/// rejects.
fn find_bearer_token(chars: &[char], from: usize) -> Option<Span> {
    let literal = "Bearer".len();
    let mut start = from;
    while start + literal <= chars.len() {
        if boundary_at(chars, start)
            && starts_with_ignore_case(chars, start, "Bearer")
        {
            let mut cursor = start + literal;
            while cursor < chars.len() && is_js_space(chars[cursor]) {
                cursor += 1;
            }
            if cursor > start + literal {
                if let Some(end) = class_run(chars, cursor, 12, is_bearer_body)
                {
                    return Some((start, end));
                }
            }
        }
        start += 1;
    }
    None
}

/// The reference's fifth pattern, `\b[0-9a-fA-F]{32,}\b`.
fn find_long_hex_run(chars: &[char], from: usize) -> Option<Span> {
    let mut start = from;
    while start < chars.len() {
        if boundary_at(chars, start) {
            if let Some(end) = class_run(chars, start, 32, |current| {
                current.is_ascii_hexdigit()
            }) {
                return Some((start, end));
            }
        }
        start += 1;
    }
    None
}

/// The reference's sixth pattern, `\b[A-Za-z0-9+/]{40,}={0,2}\b`.
///
/// The class run is greedy, then the padding is greedy, then the trailing `\b`
/// decides; a failure gives padding back first and shortens the run only after
/// every padding count has failed.
fn find_long_base64_run(chars: &[char], from: usize) -> Option<Span> {
    let mut start = from;
    while start < chars.len() {
        if boundary_at(chars, start) && is_base64_body(chars[start]) {
            let mut end = start;
            while end < chars.len() && is_base64_body(chars[end]) {
                end += 1;
            }
            while end >= start + 40 {
                for padding in (0..=2).rev() {
                    let candidate = end + padding;
                    if candidate <= chars.len()
                        && chars[end..candidate]
                            .iter()
                            .all(|byte| *byte == '=')
                        && boundary_at(chars, candidate)
                    {
                        return Some((start, candidate));
                    }
                }
                end -= 1;
            }
        }
        start += 1;
    }
    None
}

/// Conservative sanitizer for doctor text: redacts absolute paths and
/// credential-shaped tokens. Deterministic and bounded.
pub fn sanitize_safe_doctor_text(text: &str) -> String {
    let sanitized = apply_scanner(text, "<path>", drive_path_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", common_root_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", multi_segment_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", unc_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", tilde_matcher);
    redact_secrets(&sanitized)
}

/// Secret-only redaction (no path rewriting), delegating to the single owner.
pub fn sanitize_secrets_only(text: &str) -> String {
    redact_secrets(text)
}

/// Render one check's sanitized safe-report entry.
pub fn to_safe_check(
    id: &str,
    area: &str,
    status: &str,
    summary: &str,
) -> Value {
    json!({
        "id": id,
        "area": area,
        "status": status,
        "summary": sanitize_safe_doctor_text(summary),
    })
}

/// Stable runtime revision/fingerprint of the self-reference.
#[allow(clippy::too_many_arguments)]
pub fn compute_self_reference_revision(
    version: &str,
    node_major: u64,
    platform: &str,
    command_catalog_revision: &str,
    config_schema_revision: &str,
    capability_schema_revision: &str,
    tool_abi_revision_value: &str,
) -> String {
    sha256_hex_str(&canonicalize_json(&json!({
        "version": version,
        "nodeMajor": node_major,
        "platform": platform,
        "commandCatalogRevision": command_catalog_revision,
        "configSchemaRevision": config_schema_revision,
        "capabilitySchemaRevision": capability_schema_revision,
        "toolAbiRevision": tool_abi_revision_value,
    })))
}

/// Stable revision over the registered tool surface (name, description,
/// input schema, capability), capped like the reference.
pub fn tool_abi_revision(tools: &[Value]) -> String {
    let capped: Vec<Value> = tools.iter().take(512).cloned().collect();
    sha256_hex_str(&canonicalize_json(&Value::Array(capped)))
}

/// The built-in configuration-surface section names, in order.
pub const CONFIG_SCHEMA_SECTION_NAMES: [&str; 4] =
    ["sandbox", "godot", "quality", "references"];

/// The built-in configuration-surface summary document, mirroring the
/// reference structure verbatim so the revision digest binds the same
/// bytes.
pub fn config_schema_summary() -> Value {
    json!([
        {
            "name": "sandbox",
            "description": "Session sandbox profile and backend selection.",
            "keys": [
                { "name": "profile", "description": "Session sandbox profile.", "allowed": ["inspect", "develop-offline"], "shape": "string" },
                { "name": "backend", "description": "Sandbox backend selection.", "allowed": ["auto", "anthropic-runtime"], "shape": "string" }
            ]
        },
        {
            "name": "godot",
            "description": "Trusted user-level Godot installation configuration. Project files cannot select or broaden executables.",
            "keys": [
                { "name": "activeInstallation", "description": "Installation id used by default; must reference a configured or discovered installation.", "shape": "string" },
                { "name": "installations", "description": "Map of installation id to { path (absolute), editionHint: standard|dotnet|unknown }.", "shape": "object" },
                { "name": "discoverOnPath", "description": "Whether fixed-name PATH discovery is enabled (default true).", "shape": "boolean" }
            ]
        },
        {
            "name": "quality",
            "description": "Trusted user-level development-quality configuration. An untrusted repository cannot alter these settings.",
            "keys": [
                { "name": "reviewProvider", "description": "Provider profile used for the independent change reviewer; must reference an existing configured provider.", "shape": "string" }
            ]
        },
        {
            "name": "references",
            "description": "Declared external read-only references, alias to declaration. Aliases match ^[a-z][a-z0-9._-]{1,63}$; at most 16 references. Unknown keys are rejected at every level so credential fields cannot hide.",
            "keys": [
                { "name": "<alias>", "description": "One reference declaration: kind (local-directory|repository), path, repository, optional ref { kind: commit|tag|branch, ... }, optional description.", "allowed": ["local-directory", "repository"], "shape": "object" }
            ]
        }
    ])
}

/// The stable configuration-surface revision digest.
pub fn config_schema_revision() -> String {
    sha256_hex_str(&canonicalize_json(&config_schema_summary()))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_normalization_is_canonical_and_fail_closed() {
        assert_eq!(
            normalize_doctor_request(&[]).expect("empty"),
            DOCTOR_AREAS.to_vec()
        );
        assert_eq!(
            normalize_doctor_request(&["godot", "runtime"]).expect("valid"),
            vec!["runtime", "godot"]
        );
        assert_eq!(
            normalize_doctor_request(&["not-an-area"]),
            Err(DOCTOR_INVOCATION_ERROR)
        );
    }

    #[test]
    fn counts_and_exit_codes_follow_the_contract() {
        let checks = vec![
            DoctorCheckResult {
                id: "a".into(),
                area: "runtime",
                status: "pass",
                summary: "ok".into(),
            },
            DoctorCheckResult {
                id: "b".into(),
                area: "runtime",
                status: "warn",
                summary: "w".into(),
            },
            DoctorCheckResult {
                id: "c".into(),
                area: "runtime",
                status: "fail",
                summary: "f".into(),
            },
            DoctorCheckResult {
                id: "d".into(),
                area: "runtime",
                status: "skip",
                summary: "s".into(),
            },
        ];
        let counts = count_doctor_report(&checks);
        assert_eq!(counts["total"], 4);
        assert_eq!(doctor_exit_code_for(&counts), 1);
        let clean = count_doctor_report(&checks[0..1]);
        assert_eq!(doctor_exit_code_for(&clean), 0);
    }

    #[test]
    fn sanitizer_redacts_paths_and_secrets_like_the_reference() {
        // Windows drive paths, common roots, multi-segment POSIX paths,
        // UNC shares, and home shorthand all collapse to <path>.
        assert_eq!(
            sanitize_safe_doctor_text("see C:\\Users\\x\\f.txt"),
            "see <path>"
        );
        assert_eq!(
            sanitize_safe_doctor_text("under /home/someone/repo"),
            "under <path>"
        );
        // The reference alternation shortens /apps/x at the `app` root.
        assert_eq!(sanitize_safe_doctor_text("src/app.ts"), "src<path>.ts");
        assert_eq!(
            sanitize_safe_doctor_text("\\\\server\\share\\x"),
            "<path>"
        );
        assert_eq!(sanitize_safe_doctor_text("in ~/notes.txt"), "in <path>");
        // Single-segment slash tokens survive (no root match).
        assert_eq!(
            sanitize_safe_doctor_text("/doctor stays"),
            "/doctor stays"
        );
        // Secrets.
        assert_eq!(
            sanitize_safe_doctor_text("key sk-abcdef123456 here"),
            "key <secret> here"
        );
        assert_eq!(
            sanitize_safe_doctor_text("id AKIAIOSFODNN7EXAMPLE"),
            "id <secret>"
        );
        assert_eq!(
            sanitize_safe_doctor_text("Bearer abc.def.ghi_jkl-123"),
            "<secret>"
        );
        assert_eq!(
            sanitize_safe_doctor_text(&format!("hash {} end", "a".repeat(32))),
            "hash <secret> end"
        );
        assert_eq!(
            sanitize_secrets_only(
                "keep src/app.ts; drop Bearer abcdefghijkl1234567890"
            ),
            "keep src/app.ts; drop <secret>"
        );
    }

    #[test]
    fn self_reference_revision_is_stable_and_version_sensitive() {
        let parts = (
            "1.2.3".to_string(),
            24u64,
            "test".to_string(),
            "c".repeat(64),
            "k".repeat(64),
            "a".repeat(64),
            "t".repeat(64),
        );
        let revision = |version: &str| {
            compute_self_reference_revision(
                version, parts.1, &parts.2, &parts.3, &parts.4, &parts.5,
                &parts.6,
            )
        };
        let first = revision(&parts.0);
        assert_eq!(first, revision(&parts.0));
        assert_ne!(first, revision("9.9.9"));
    }

    #[test]
    fn config_schema_summary_binds_its_sections() {
        assert_eq!(
            CONFIG_SCHEMA_SECTION_NAMES,
            ["sandbox", "godot", "quality", "references"]
        );
        assert_eq!(config_schema_revision(), config_schema_revision());
    }

    #[test]
    fn tool_abi_revision_tracks_the_surface() {
        let tools = vec![
            json!({ "name": "workspace.list", "description": "List entries", "inputSchema": { "type": "object" }, "capability": "workspace.read" }),
        ];
        assert_eq!(tool_abi_revision(&tools), tool_abi_revision(&tools));
        let different = json!({
            "name": "workspace.read",
            "description": "Read a file",
            "inputSchema": { "type": "object" },
            "capability": "workspace.read"
        });
        assert_ne!(tool_abi_revision(&tools), tool_abi_revision(&[different]));
    }
}

/// The durable corpus for the single secret-redaction owner (`ROADMAP.md` §10
/// item 3).
///
/// PROVENANCE: every `expected` value below was produced by **executing** the
/// recovered reference, not by reading it or deriving it by hand.
/// `SECRET_PATTERNS` is extracted verbatim from
/// `packages/core/src/doctor/safe-report.ts` at `5da5cde` — six patterns, applied
/// in order by that file's `sanitizeSecretsOnly` through
/// `String.prototype.replace` — and was run in Node over exactly these inputs.
/// The four-seat panel reported its own independent execution of the same
/// patterns; the two agreed on all forty rows the panel stated.
///
/// The spaces this corpus pins:
///
/// - every rule at, below and above its minimum (the `AKIA` body at 15, 16, 17
///   and 18 characters among them);
/// - every member of the JavaScript `\s` set, one row per character, and the
///   characters that resemble members and are not (U+0085, U+180E, U+200B and an
///   ordinary letter) — so a regression to `char::is_whitespace` or to
///   `is_ascii_whitespace` fails here;
/// - `Bearer` case-insensitivity in three spellings, with two negatives;
/// - the boundary shapes the two former implementations disagreed on: a class run
///   ending in `-`, `+` or `/` before a non-word character, `=` padding of one,
///   two and three characters, and a mid-word `AKIA`;
/// - `\b` at index 0, at end of input, and against `_`, `-`, `.`, `/` and `=`;
/// - rule order, where two rules could claim the same text;
/// - adjacency to non-ASCII text (é and CJK);
/// - the pinned redaction inputs of the `capability-doctor-injected` and
///   `brief-render-bounded` records.
///
/// Anything outside those spaces is unmeasured, and coverage is by construction
/// of the row list below rather than by a generator: a new or altered rule is
/// unproven until a row pins it, and passing this module is not an equivalence
/// proof against the reference.
///
/// The AWS-shaped rows use the documented AWS sample key instead of a synthetic
/// `AKIA` body. A key-shaped literal in this file is indistinguishable from a
/// real credential to the secret-hygiene gate, which scans every repository
/// file and exempts exactly that documented sample; the rule's behaviour depends
/// only on the character classes, which the sample satisfies. Nothing here is a
/// credential.
#[cfg(test)]
mod secret_corpus {
    use super::sanitize_safe_doctor_text;
    use super::{SECRET_REPLACEMENT, is_word_char, redact_secrets};

    /// `(label, input, expected)`, in the order the oracle emitted them.
    const CASES: &[(&str, &str, &str)] = &[
        ("sk-7", "sk-aaaaaaa x", "sk-aaaaaaa x"),
        ("sk-8", "sk-aaaaaaaa x", "<secret> x"),
        ("sk-9", "sk-aaaaaaaaa x", "<secret> x"),
        ("sk-8-dash-bang", "sk-aaaaaaaa-!", "<secret>-!"),
        ("sk-8-dash-space", "sk-aaaaaaaa- x", "<secret>- x"),
        ("akia16", "AKIAIOSFODNN7EXAMPLE x", "<secret> x"),
        ("akia17", "AKIAIOSFODNN7EXAMPLEX x", "AKIAIOSFODNN7EXAMPLEX x"),
        ("akia-midword", "xAKIAIOSFODNN7EXAMPLE", "xAKIAIOSFODNN7EXAMPLE"),
        ("gh19", "ghp_aaaaaaaaaaaaaaaaaaa x", "ghp_aaaaaaaaaaaaaaaaaaa x"),
        ("gh20", "ghp_aaaaaaaaaaaaaaaaaaaa x", "<secret> x"),
        ("bearer11", "Bearer aaaaaaaaaaa x", "Bearer aaaaaaaaaaa x"),
        ("bearer12", "Bearer aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space", "Bearer aaaaaaaaaaaaaaaaaaaaaa x", "<secret> x"),
        (
            "bearer-overlap",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/=",
            "<secret>/=",
        ),
        (
            "b64-41-plusbang",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz+!",
            "<secret>+!",
        ),
        (
            "b64-40-eq-eof",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz=",
            "<secret>=",
        ),
        (
            "b64-40-eq-x",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz=x",
            "<secret>x",
        ),
        (
            "b64-40-eqeq-eof",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz==",
            "<secret>==",
        ),
        (
            "b64-40-eqeqeq-eof",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz===",
            "<secret>===",
        ),
        (
            "hex31",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa x",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa x",
        ),
        ("hex32", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa x", "<secret> x"),
        (
            "eacute-flank",
            "\u{e9}AKIAIOSFODNN7EXAMPLE\u{e9}",
            "\u{e9}<secret>\u{e9}",
        ),
        (
            "PINNED-secrets",
            "found sk-abcdef123456 and AKIAIOSFODNN7EXAMPLE and Bearer abc.def.ghi_jkl-123",
            "found <secret> and <secret> and <secret>",
        ),
        (
            "PINNED-brief-secret",
            "never embed tokens like sk-abcd12345678 in output",
            "never embed tokens like <secret> in output",
        ),
        (
            "PINNED-secretsOnly",
            "see src/app.ts for Bearer abcdefghijkl1234567890",
            "see src/app.ts for <secret>",
        ),
        (
            "bearer-40hex-overlap",
            "Bearer aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "<secret>",
        ),
        (
            "b64-containing-sk",
            "zzzzzzzzzzzzzzzzzzzzsk-aaaaaaaazzzzzzzzzzzzzzzzzzzz",
            "zzzzzzzzzzzzzzzzzzzzsk-aaaaaaaazzzzzzzzzzzzzzzzzzzz",
        ),
        ("sk-at-index-0", "sk-aaaaaaaa x", "<secret> x"),
        ("sk-at-eof", "x sk-aaaaaaaa", "x <secret>"),
        ("sk-glued-underscore", "_sk-aaaaaaaa_", "_sk-aaaaaaaa_"),
        ("sk-glued-dash", "-sk-aaaaaaaa-", "-<secret>-"),
        ("sk-glued-dot", ".sk-aaaaaaaa.", ".<secret>."),
        (
            "sk-then-akia",
            "sk-aaaaaaaa AKIAIOSFODNN7EXAMPLE",
            "<secret> <secret>",
        ),
        ("ghp-at-eof", "x ghp_aaaaaaaaaaaaaaaaaaaa", "x <secret>"),
        (
            "hex32-midword",
            "zaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaz",
            "zaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaz",
        ),
        (
            "two-adjacent-hex",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "<secret>",
        ),
        (
            "hex-sep-one-word-char",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaxbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "<secret>",
        ),
        (
            "secret-marker-input",
            "<secret> AKIAIOSFODNN7EXAMPLE",
            "<secret> <secret>",
        ),
        ("empty", "", ""),
        ("whitespace", "   ", "   "),
        ("gho-20", "gho_aaaaaaaaaaaaaaaaaaaa x", "<secret> x"),
        ("ghs-20", "ghs_aaaaaaaaaaaaaaaaaaaa x", "<secret> x"),
        ("ghp-21", "ghp_aaaaaaaaaaaaaaaaaaaaa x", "<secret> x"),
        ("bearer13", "Bearer aaaaaaaaaaaaa x", "<secret> x"),
        // The reference's `i` flag covers the literal only.
        ("bearer-upper", "BEARER aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-mixed", "bEaReR aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-mixed-2", "BeArEr aaaaaaaaaaaa x", "<secret> x"),
        (
            "bearer-wrong-literal",
            "Beare aaaaaaaaaaaa x",
            "Beare aaaaaaaaaaaa x",
        ),
        (
            "bearer-upper-nowhitespace",
            "BEARERaaaaaaaaaaaa x",
            "BEARERaaaaaaaaaaaa x",
        ),
        // Every member of the JavaScript `\s` set, one row each: it must
        // separate the literal from the token.
        ("bearer-space-u0009", "Bearer\u{9}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u000a", "Bearer\u{a}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u000b", "Bearer\u{b}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u000c", "Bearer\u{c}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u000d", "Bearer\u{d}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u0020", "Bearer aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u00a0", "Bearer\u{a0}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u1680", "Bearer\u{1680}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2000", "Bearer\u{2000}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2001", "Bearer\u{2001}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2002", "Bearer\u{2002}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2003", "Bearer\u{2003}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2004", "Bearer\u{2004}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2005", "Bearer\u{2005}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2006", "Bearer\u{2006}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2007", "Bearer\u{2007}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2008", "Bearer\u{2008}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2009", "Bearer\u{2009}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u200a", "Bearer\u{200a}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2028", "Bearer\u{2028}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u2029", "Bearer\u{2029}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u202f", "Bearer\u{202f}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u205f", "Bearer\u{205f}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-u3000", "Bearer\u{3000}aaaaaaaaaaaa x", "<secret> x"),
        ("bearer-space-ufeff", "Bearer\u{feff}aaaaaaaaaaaa x", "<secret> x"),
        // Characters that look like JavaScript `\s` and are not in it.
        (
            "bearer-space-u0085",
            "Bearer\u{85}aaaaaaaaaaaa x",
            "Bearer\u{85}aaaaaaaaaaaa x",
        ),
        (
            "bearer-space-u180e",
            "Bearer\u{180e}aaaaaaaaaaaa x",
            "Bearer\u{180e}aaaaaaaaaaaa x",
        ),
        (
            "bearer-space-u200b",
            "Bearer\u{200b}aaaaaaaaaaaa x",
            "Bearer\u{200b}aaaaaaaaaaaa x",
        ),
        (
            "bearer-space-letter",
            "Beareraaaaaaaaaaaaa x",
            "Beareraaaaaaaaaaaaa x",
        ),
        ("bearer-midword", "xBearer aaaaaaaaaaaa x", "xBearer aaaaaaaaaaaa x"),
        ("sk-eof-underscore", "x sk-aaaaaaaa_", "x <secret>"),
        ("sk-bang-delimited", "!sk-aaaaaaaa!", "!<secret>!"),
        ("sk-hyphen-body", "sk--------- x", "sk--------- x"),
        ("sk-underscore-body", "sk-________ x", "<secret> x"),
        (
            "hex-slash-flank",
            "/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            "/<secret>/",
        ),
        ("hex-at-eof", "x aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "x <secret>"),
        (
            "b64-39-plus-tail",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz+?",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz+?",
        ),
        (
            "b64-39-slash-tail",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/?",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/?",
        ),
        (
            "b64-40-plus-tail",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz+?",
            "<secret>+?",
        ),
        (
            "b64-40-slash-tail",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz/?",
            "<secret>/?",
        ),
        (
            "b64-40-plus-bang",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz+!",
            "<secret>+!",
        ),
        (
            "b64-abutting",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz=zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            "<secret><secret>",
        ),
        (
            "han-flank-hex",
            "\u{6f22}aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{6f22}",
            "\u{6f22}<secret>\u{6f22}",
        ),
        (
            "han-flank-bearer",
            "\u{6f22}Bearer aaaaaaaaaaaa\u{6f22}",
            "\u{6f22}<secret>\u{6f22}",
        ),
        ("marker-alone", "<secret>", "<secret>"),
        ("marker-after-sk", "sk-aaaaaaaa <secret>", "<secret> <secret>"),
        ("whitespace-tab", "\t\n", "\t\n"),
        ("akia18", "AKIAIOSFODNN7EXAMPLEXX x", "AKIAIOSFODNN7EXAMPLEXX x"),
        ("akia15", "AKIAIOSFODNN7EXAMPL x", "AKIAIOSFODNN7EXAMPL x"),
        (
            "akia-lowercase-body",
            "AKIAaaaaaaaaaaaaaaaa x",
            "AKIAaaaaaaaaaaaaaaaa x",
        ),
    ];

    #[test]
    fn the_owner_reproduces_the_executed_reference() {
        let mut diverged: Vec<String> = Vec::new();
        for &(label, input, expected) in CASES {
            let actual = redact_secrets(input);
            if actual != expected {
                diverged.push(format!(
                    "  {label}\n    input:    {input:?}\n    expected: {expected:?}\n    actual:   {actual:?}"
                ));
            }
        }
        assert!(
            diverged.is_empty(),
            "{} of {} corpus row(s) diverge from the reference:\n{}",
            diverged.len(),
            CASES.len(),
            diverged.join("\n"),
        );
    }

    #[test]
    fn no_rule_can_match_inside_the_replacement() {
        // A later pass re-scans the marker an earlier pass emitted, so the marker
        // must be inert. Its longest word run is `secret`, far below the shortest
        // minimum (`sk-` plus eight), and it carries none of the rule literals.
        let longest = SECRET_REPLACEMENT
            .split(|current| !is_word_char(current))
            .map(str::len)
            .max()
            .unwrap_or(0);
        assert!(
            longest < 8,
            "`{SECRET_REPLACEMENT}` carries a {longest}-character word run"
        );
        assert_eq!(redact_secrets(SECRET_REPLACEMENT), SECRET_REPLACEMENT);
    }

    #[test]
    fn the_path_stage_still_runs_first_and_is_unchanged() {
        // `sanitizeSafeDoctorText` is five path passes and then the owner, so a
        // redaction change must not disturb the path records pinned by the
        // `capability-doctor-injected` subject.
        assert_eq!(
            sanitize_safe_doctor_text(
                "relative src/app.ts stays intact; /doctor stays intact"
            ),
            "relative src<path>.ts stays intact; /doctor stays intact"
        );
        assert_eq!(
            sanitize_safe_doctor_text(
                "found sk-abcdef123456 and AKIAIOSFODNN7EXAMPLE and Bearer abc.def.ghi_jkl-123"
            ),
            "found <secret> and <secret> and <secret>"
        );
    }
}
