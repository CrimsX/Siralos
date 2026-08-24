//! Doctor domain model (Stage 3R R13.1).
//!
//! Deterministic, read-only, offline diagnostic vocabulary mirrored from
//! the TypeScript reference: typed areas and statuses, canonical request
//! normalization, report counts and exit codes, the safe/public report
//! sanitizer, the bounded self-reference revision fingerprint, and the
//! configuration-surface summary. Checks carry structured status; the
//! doctor never repairs anything.

use crate::godot::digest::{canonicalize_json, sha256_hex_str};
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

    fn word_boundary_at(&self, index: usize) -> bool {
        let before = index > 0 && {
            let previous = self.chars[index - 1];
            previous.is_ascii_alphanumeric() || previous == '_'
        };
        let after = match self.chars.get(index) {
            None => false,
            Some(current) => {
                current.is_ascii_alphanumeric() || *current == '_'
            }
        };
        before != after
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

/// Credential-shaped token matchers, applied in the reference order:
/// `sk-` keys, AKIA access keys, `gh[pso]_` tokens, case-insensitive
/// Bearer headers, long hex runs, and long base64 runs.
fn secret_matcher(scanner: &mut Scanner) -> bool {
    if !scanner.word_boundary_at(scanner.position) {
        return false;
    }
    if scanner.starts_with("sk-") {
        let start = scanner.position;
        scanner.position += 3;
        scanner.consume_while(|current| {
            current.is_ascii_alphanumeric() || current == '_' || current == '-'
        });
        if scanner.position - start >= 3 + 8
            && scanner.word_boundary_at(scanner.position)
        {
            return true;
        }
        scanner.position = start;
    }
    if scanner.starts_with("AKIA") {
        let start = scanner.position;
        scanner.position += 4;
        scanner.consume_while(|current| {
            current.is_ascii_uppercase() || current.is_ascii_digit()
        });
        if scanner.position - start == 4 + 16
            && scanner.word_boundary_at(scanner.position)
        {
            return true;
        }
        scanner.position = start;
    }
    for prefix in ["ghp_", "gho_", "ghs_"] {
        if scanner.starts_with(prefix) {
            let start = scanner.position;
            scanner.position += 4;
            scanner.consume_while(|current| {
                current.is_ascii_alphanumeric() || current == '_'
            });
            if scanner.position - start >= 4 + 20
                && scanner.word_boundary_at(scanner.position)
            {
                return true;
            }
            scanner.position = start;
        }
    }
    const BEARER_LOWER: [char; 6] = ['b', 'e', 'a', 'r', 'e', 'r'];
    if scanner
        .chars
        .iter()
        .skip(scanner.position)
        .zip(BEARER_LOWER.iter())
        .all(|(actual, expected)| actual.to_ascii_lowercase() == *expected)
    {
        let start = scanner.position;
        scanner.position += BEARER_LOWER.len();
        let whitespace_start = scanner.position;
        scanner.consume_while(char::is_whitespace);
        if scanner.position > whitespace_start {
            let token_start = scanner.position;
            scanner.consume_while(|current| {
                current.is_ascii_alphanumeric()
                    || matches!(
                        current,
                        '.' | '_' | '~' | '+' | '/' | '=' | '-'
                    )
            });
            if scanner.position - token_start >= 12
                && scanner.word_boundary_at(scanner.position)
            {
                return true;
            }
        }
        scanner.position = start;
    }
    let long_run =
        |scanner: &mut Scanner, accept: fn(char) -> bool, minimum: usize| {
            let start = scanner.position;
            scanner.consume_while(accept);
            let matched = scanner.position - start >= minimum
                && scanner.word_boundary_at(scanner.position);
            if matched {
                true
            } else {
                scanner.position = start;
                false
            }
        };
    if long_run(scanner, |current| current.is_ascii_hexdigit(), 32) {
        return true;
    }
    if long_run(
        scanner,
        |current| {
            current.is_ascii_alphanumeric() || current == '+' || current == '/'
        },
        40,
    ) {
        return true;
    }
    false
}

/// Conservative sanitizer for doctor text: redacts absolute paths and
/// credential-shaped tokens. Deterministic and bounded.
pub fn sanitize_safe_doctor_text(text: &str) -> String {
    let sanitized = apply_scanner(text, "<path>", drive_path_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", common_root_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", multi_segment_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", unc_matcher);
    let sanitized = apply_scanner(&sanitized, "<path>", tilde_matcher);
    apply_scanner(&sanitized, "<secret>", secret_matcher)
}

/// Secret-only redaction (no path rewriting).
pub fn sanitize_secrets_only(text: &str) -> String {
    apply_scanner(text, "<secret>", secret_matcher)
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
