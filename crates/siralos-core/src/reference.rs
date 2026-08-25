//! External reference model, declaration parsing, registry, ports, and
//! evidence views (Stage 3 milestone 5; R13.3 external knowledge
//! boundaries).
//!
//! Mirrors `packages/core/src/reference/*`. A reference is a first-class,
//! read-only external source the application may consult — a local
//! directory outside the workspace namespace, or a remote repository
//! pinned to an immutable commit. Core never touches the network or the
//! filesystem: resolution and materialization happen through typed ports
//! implemented by adapters. The registry is the SINGLE owner of reference
//! identity; revisions are immutable and only `refresh` replaces them,
//! failing closed (invalidating the current revision) when resolution
//! fails.

use crate::godot::digest::{canonicalize_json, sha256_hex_str};
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Opaque reference-id prefix: `ref_` + 24 hex chars derived from the alias.
pub const REFERENCE_ID_PREFIX: &str = "ref_";

/// MUTABLE_REF_REFUSAL.
pub const MUTABLE_REF_REFUSAL: &str =
    "mutable repository ref requires an explicit pinned commit/tag";
/// WORKSPACE_CONTAINMENT_REFUSAL.
pub const WORKSPACE_CONTAINMENT_REFUSAL: &str =
    "reference root must be outside the workspace namespace";
/// Exact reference-failure reason.
pub const DUPLICATE_ALIAS_REASON: &str = "duplicate alias";
/// Exact reference-failure reason.
pub const INVALID_ALIAS_REASON: &str = "invalid alias";

/// Bounded limits mirrored from `REFERENCE_LIMITS`.
pub const REFERENCE_LIMITS: ReferenceLimits = ReferenceLimits {
    max_references: 16,
    max_alias_length: 64,
    max_description_bytes: 512,
    max_repository_length: 2048,
    max_local_directory_path_length: 4096,
    max_commit_length: 64,
    max_tag_length: 128,
    max_branch_length: 128,
    max_manifest_entries: 10_000,
    max_manifest_bytes: 8 * 1024 * 1024,
    max_file_sha256_bytes: 1024 * 1024,
    max_revision_bindings: 64,
};

/// Deterministic reference id from the alias (`ref_` + 24 hex chars).
/// Ids identify the declaration slot; the current revision carries the
/// resolved identity.
#[must_use]
pub fn create_reference_id(alias: &str) -> String {
    let digest =
        sha256_hex_str(&canonicalize_json(&json!({ "alias": alias })));
    format!("{REFERENCE_ID_PREFIX}{}", &digest[..24])
}

/// Validate a reference alias (`^[a-z][a-z0-9._-]{1,63}$`).
#[must_use]
pub fn validate_reference_alias(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes.len() > 64 {
        return None;
    }
    if !bytes[0].is_ascii_lowercase() {
        return None;
    }
    for byte in &bytes[1..] {
        if !(byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'.' | b'_' | b'-'))
        {
            return None;
        }
    }
    Some(value)
}

/// Model-facing display name of an alias.
#[must_use]
pub fn format_reference_alias(alias: &str) -> String {
    format!("@reference/{alias}")
}

/// Bounded limits mirrored from `REFERENCE_LIMITS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceLimits {
    /// Bound mirrored from the corresponding limits table.
    pub max_references: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_alias_length: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_description_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_repository_length: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_local_directory_path_length: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_commit_length: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_tag_length: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_branch_length: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_manifest_entries: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_manifest_bytes: usize,
    /// Per-file SHA-256 cap for reference content hashing.
    pub max_file_sha256_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_revision_bindings: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Discriminant kind.
pub enum ReferenceKind {
    /// LocalDirectory.
    LocalDirectory,
    /// Repository.
    Repository,
}

impl ReferenceKind {
    #[must_use]
    /// as_str.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalDirectory => "local-directory",
            Self::Repository => "repository",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// RepositoryRef.
pub enum RepositoryRef {
    /// Resolved commit.
    Commit {
        /// Resolved commit.
        commit: String,
    },
    /// Tag pin.
    Tag {
        /// Tag pin.
        tag: String,
    },
    /// Branch pin.
    Branch {
        /// Branch pin.
        branch: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Configured source reference.
pub enum ReferenceSource {
    /// LocalDirectory.
    LocalDirectory {
        /// Path relative to the owning root.
        path: String,
    },
    /// Repository.
    Repository {
        /// repository.
        repository: String,
        /// r#ref.
        r#ref: RepositoryRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Resolved identity.
pub enum ResolvedReferenceIdentity {
    /// LocalDirectory.
    LocalDirectory {
        /// Canonical absolute path.
        canonical_path: String,
        /// Manifest fingerprint digest.
        fingerprint: String,
    },
    /// Repository.
    Repository {
        /// Canonical repository origin.
        origin: String,
        /// Resolved commit.
        commit: String,
        /// The pin as declared.
        requested_ref: RepositoryRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Immutable revision value.
pub struct ReferenceRevision {
    /// Resolved identity.
    pub identity: ResolvedReferenceIdentity,
    /// Epoch milliseconds (injected clock).
    pub resolved_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ReferenceStatus.
pub enum ReferenceStatus {
    /// Ready.
    Ready,
    /// ResolutionFailed.
    ResolutionFailed,
    /// Unavailable.
    Unavailable,
    /// Declined.
    Declined,
}

impl ReferenceStatus {
    #[must_use]
    /// as_str.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ResolutionFailed => "resolution-failed",
            Self::Unavailable => "unavailable",
            Self::Declined => "declined",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// MaterializationStatus.
pub enum MaterializationStatus {
    /// NotRequired.
    NotRequired,
    /// NotMaterialized.
    NotMaterialized,
    /// Materialized.
    Materialized,
    /// Unavailable.
    Unavailable,
    /// Failed.
    Failed,
}

impl MaterializationStatus {
    #[must_use]
    /// as_str.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "not-required",
            Self::NotMaterialized => "not-materialized",
            Self::Materialized => "materialized",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ReferenceTrustClass.
pub enum ReferenceTrustClass {
    /// ExplicitUser.
    ExplicitUser,
    /// TrustedProject.
    TrustedProject,
    /// UntrustedProject.
    UntrustedProject,
    /// Managed.
    Managed,
}

#[derive(Debug, Clone)]
/// Reference.
pub struct Reference {
    /// Stable identifier.
    pub id: String,
    /// Validated alias.
    pub alias: String,
    /// Discriminant kind.
    pub kind: ReferenceKind,
    /// Configured source reference.
    pub source: ReferenceSource,
    /// Host-assigned trust class.
    pub trust: ReferenceTrustClass,
    /// Optional bounded description.
    pub description: Option<String>,
    /// Lifecycle status.
    pub status: ReferenceStatus,
    /// Precise reason for non-ready status; `None` when ready.
    pub failure_reason: Option<String>,
}

/// Immutable snapshot of the revisions a task started with.
#[derive(Debug, Clone)]
pub struct ReferenceTaskBinding {
    /// Stable identifier.
    pub task_id: String,
    /// ReferenceId -> revision, for every reference ready at bind time.
    pub revisions: HashMap<String, ReferenceRevision>,
    /// Epoch milliseconds (injected clock).
    pub bound_at_ms: u64,
}

// ---------------------------------------------------------------------------
// Declaration parsing (untrusted configuration JSON).
// ---------------------------------------------------------------------------

/// One parsed, canonical reference declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceDeclaration {
    /// Validated alias.
    pub alias: String,
    /// Discriminant kind.
    pub kind: ReferenceKind,
    /// Configured source reference.
    pub source: ReferenceSource,
    /// Optional bounded description.
    pub description: Option<String>,
}

const ALLOWED_DECLARATION_KEYS: [&str; 4] =
    ["alias", "kind", "source", "description"];
const ALLOWED_SOURCE_KEYS_LOCAL: [&str; 2] = ["kind", "path"];
const ALLOWED_SOURCE_KEYS_REPOSITORY: [&str; 3] =
    ["kind", "repository", "ref"];
const ALLOWED_REF_KEYS: [&str; 4] = ["kind", "commit", "tag", "branch"];

fn reject_unknown_keys(
    record: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
    subject: &str,
) -> Option<String> {
    for key in record.keys() {
        if !allowed.contains(&key.as_str()) {
            return Some(format!(
                "Unknown key \"{key}\" in {subject}; unknown keys are rejected."
            ));
        }
    }
    None
}

fn require_string(
    record: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    subject: &str,
) -> Result<String, String> {
    match record.get(key) {
        Some(serde_json::Value::String(value)) => Ok(value.clone()),
        _ => Err(format!("{subject} requires a string \"{key}\".")),
    }
}

/// Normalize a repository origin to its canonical GitHub form.
/// Accepts `owner/repo`, `https://github.com/owner/repo`, and
/// `https://github.com/owner/repo.git`; rejects every other host, http,
/// credentials, queries, fragments, and extra path segments.
pub fn normalize_repository_origin(input: &str) -> Result<String, String> {
    if input.trim().is_empty() {
        return Err("A repository origin is required.".to_owned());
    }
    let trimmed = input.trim();
    if trimmed.len() > REFERENCE_LIMITS.max_repository_length {
        return Err(format!(
            "Repository origin exceeds the limit of {} characters.",
            REFERENCE_LIMITS.max_repository_length
        ));
    }
    if trimmed.contains('\0') {
        return Err(
            "Repository origins must not contain null bytes.".to_owned()
        );
    }
    if trimmed.contains('#') {
        return Err(
            "Repository origins must not contain a fragment.".to_owned()
        );
    }
    if trimmed.contains('?') {
        return Err(
            "Repository origins must not contain a query string.".to_owned()
        );
    }
    if trimmed.contains('@') {
        return Err(
            "Repository origins must not contain credentials (userinfo is rejected).".to_owned(),
        );
    }
    if trimmed.starts_with("http://") {
        return Err("Repository origins must use https, not http.".to_owned());
    }
    let mut rest = trimmed;
    if let Some(stripped) = rest.strip_prefix("https://") {
        rest = stripped;
        if !rest.starts_with("github.com/") {
            let host = rest.split('/').next().unwrap_or("");
            return Err(format!(
                "Unsupported repository host \"{host}\"; only github.com is supported."
            ));
        }
        rest = &rest["github.com/".len()..];
    }
    let without_trailing = rest.trim_end_matches('/');
    let without_git =
        without_trailing.strip_suffix(".git").unwrap_or(without_trailing);
    if without_git.is_empty() {
        return Err(
            "A repository origin must be exactly owner/repo.".to_owned()
        );
    }
    let segments: Vec<&str> = without_git.split('/').collect();
    if segments.len() != 2 {
        return Err(
            "A repository origin must be exactly owner/repo with no additional path segments."
                .to_owned(),
        );
    }
    let owner = segments[0];
    let repo = segments[1];
    let owner_ok = !owner.is_empty()
        && owner.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !owner_ok {
        return Err(format!("Invalid repository owner \"{owner}\"."));
    }
    let repo_ok = !repo.is_empty()
        && repo.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
        });
    if !repo_ok {
        return Err(format!("Invalid repository name \"{repo}\"."));
    }
    Ok(format!("https://github.com/{owner}/{repo}"))
}

/// POSIX absolute path, Windows drive path, or Windows UNC path.
#[must_use]
pub fn is_absolute_path(path: &str) -> bool {
    if path.starts_with('/') {
        return true;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // UNC: \\server\share (also covers the \\?\ verbatim prefix).
    if let Some(rest) = path.strip_prefix("\\\\") {
        if let Some(index) = rest.find(['\\', '/']) {
            return index > 0 && index + 1 < rest.len();
        }
    }
    false
}

fn is_commit_shape(commit: &str) -> bool {
    let bytes = commit.as_bytes();
    (7..=64).contains(&bytes.len()) && bytes.iter().all(u8::is_ascii_hexdigit)
}

fn is_branch_or_tag_shape(pin: &str) -> bool {
    !pin.is_empty()
        && pin.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/')
        })
}

fn parse_repository_ref(
    value: &serde_json::Value,
) -> Result<RepositoryRef, String> {
    let record =
        value.as_object().ok_or("A repository ref must be an object.")?;
    if let Some(reason) =
        reject_unknown_keys(record, &ALLOWED_REF_KEYS, "reference ref")
    {
        return Err(reason);
    }
    let kind = record.get("kind").and_then(serde_json::Value::as_str).ok_or(
        "A repository ref requires \"kind\" of \"commit\", \"tag\", or \"branch\".",
    )?;
    match kind {
        "commit" => {
            let commit = require_string(record, "commit", "A commit ref")
                .map_err(|_| "A commit ref requires a commit string.".to_owned())?;
            if !is_commit_shape(&commit) {
                return Err(format!(
                    "The commit \"{commit}\" is malformed; commits are 7-64 hexadecimal characters."
                ));
            }
            Ok(RepositoryRef::Commit { commit })
        }
        "tag" => {
            let tag = require_string(record, "tag", "A tag ref")
                .map_err(|_| "A tag ref requires a tag string.".to_owned())?;
            if tag.is_empty()
                || tag.chars().count() > REFERENCE_LIMITS.max_tag_length
                || !is_branch_or_tag_shape(&tag)
            {
                return Err(format!(
                    "The tag \"{tag}\" is malformed; tags use letters, digits, \".\", \"_\", \"-\", \"/\" and are at most {} characters.",
                    REFERENCE_LIMITS.max_tag_length
                ));
            }
            Ok(RepositoryRef::Tag { tag })
        }
        "branch" => {
            let branch = require_string(record, "branch", "A branch ref")
                .map_err(|_| "A branch ref requires a branch string.".to_owned())?;
            if branch.is_empty()
                || branch.chars().count() > REFERENCE_LIMITS.max_branch_length
                || !is_branch_or_tag_shape(&branch)
            {
                return Err(format!(
                    "The branch \"{branch}\" is malformed; branchs use letters, digits, \".\", \"_\", \"-\", \"/\" and are at most {} characters.",
                    REFERENCE_LIMITS.max_branch_length
                ));
            }
            Ok(RepositoryRef::Branch { branch })
        }
        _ => Err(
            "A repository ref requires \"kind\" of \"commit\", \"tag\", or \"branch\".".to_owned(),
        ),
    }
}

fn display_value(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Object(_))
        | Some(serde_json::Value::Array(_)) => "[object Object]".to_owned(),
        Some(other) => other.to_string(),
        None => "undefined".to_owned(),
    }
}

/// Parse one untrusted reference declaration. Rejects unknown keys,
/// missing or malformed required fields, relative local paths, and
/// out-of-bound values. A repository declaration without `ref` defaults
/// to the mutable branch `main`.
pub fn parse_reference_declaration(
    value: &serde_json::Value,
) -> Result<ReferenceDeclaration, String> {
    let record = value
        .as_object()
        .ok_or("A reference declaration must be a plain JSON object.")?;
    if let Some(reason) = reject_unknown_keys(
        record,
        &ALLOWED_DECLARATION_KEYS,
        "reference declaration",
    ) {
        return Err(reason);
    }
    let alias_value = record.get("alias");
    let alias = alias_value
        .and_then(serde_json::Value::as_str)
        .and_then(validate_reference_alias);
    let alias = alias.ok_or_else(|| {
        let display = display_value(alias_value);
        format!(
            "The alias \"{display}\" is malformed; aliases match ^[a-z][a-z0-9._-]{{1,63}}$."
        )
    })?;
    let kind = match record.get("kind").and_then(serde_json::Value::as_str) {
        Some("local-directory") => ReferenceKind::LocalDirectory,
        Some("repository") => ReferenceKind::Repository,
        _ => {
            return Err(
                "A reference declaration requires \"kind\" of \"local-directory\" or \"repository\"."
                    .to_owned(),
            )
        }
    };
    let mut description: Option<String> = None;
    if let Some(description_value) = record.get("description") {
        let text = description_value
            .as_str()
            .ok_or("The reference description must be a string.")?;
        if text.len() > REFERENCE_LIMITS.max_description_bytes {
            return Err(format!(
                "The reference description exceeds the limit of {} bytes.",
                REFERENCE_LIMITS.max_description_bytes
            ));
        }
        description = Some(text.to_owned());
    }
    let source_value = record
        .get("source")
        .and_then(serde_json::Value::as_object)
        .ok_or("A reference declaration requires a source object.")?;
    let source_kind =
        source_value.get("kind").and_then(serde_json::Value::as_str);
    match source_kind {
        Some("local-directory") => {
            if let Some(reason) =
                reject_unknown_keys(source_value, &ALLOWED_SOURCE_KEYS_LOCAL, "reference source")
            {
                return Err(reason);
            }
            let path = require_string(source_value, "path", "A local-directory reference")
                .map_err(|_| "A local-directory reference requires a path string.".to_owned())?;
            if path.is_empty() {
                return Err("A local-directory reference requires a non-empty path.".to_owned());
            }
            if path.chars().count() > REFERENCE_LIMITS.max_local_directory_path_length {
                return Err(format!(
                    "The local-directory path exceeds the limit of {} characters.",
                    REFERENCE_LIMITS.max_local_directory_path_length
                ));
            }
            if path.contains('\0') {
                return Err("The local-directory path must not contain null bytes.".to_owned());
            }
            if !is_absolute_path(&path) {
                return Err(format!(
                    "The local-directory path \"{path}\" is not absolute; relative paths are not resolved."
                ));
            }
            Ok(ReferenceDeclaration {
                alias: alias.to_owned(),
                kind,
                source: ReferenceSource::LocalDirectory { path },
                description,
            })
        }
        Some("repository") => {
            if let Some(reason) = reject_unknown_keys(
                source_value,
                &ALLOWED_SOURCE_KEYS_REPOSITORY,
                "reference source",
            ) {
                return Err(reason);
            }
            let repository =
                require_string(source_value, "repository", "A repository reference")
                    .map_err(|_| {
                        "A repository reference requires a repository string.".to_owned()
                    })?;
            let normalized = normalize_repository_origin(&repository)?;
            let r#ref = match source_value.get("ref") {
                None | Some(serde_json::Value::Null) => RepositoryRef::Branch {
                    branch: "main".to_owned(),
                },
                Some(ref_value) => parse_repository_ref(ref_value)?,
            };
            Ok(ReferenceDeclaration {
                alias: alias.to_owned(),
                kind,
                source: ReferenceSource::Repository {
                    repository: normalized,
                    r#ref,
                },
                description,
            })
        }
        _ => Err(
            "A reference source requires \"kind\" of \"local-directory\" or \"repository\"."
                .to_owned(),
        ),
    }
}

/// Parse the untrusted `reference` config section: a plain object mapping
/// alias -> declaration; each value must repeat its key as `alias`.
pub fn parse_reference_declarations_section(
    value: &serde_json::Value,
    limits: Option<&ReferenceLimits>,
) -> Result<Vec<ReferenceDeclaration>, String> {
    let max_references =
        limits.map_or(REFERENCE_LIMITS.max_references, |l| l.max_references);
    let record = value.as_object().ok_or(
        "The \"reference\" config section must be a plain object mapping alias to declaration."
            .to_owned(),
    )?;
    let keys: Vec<&String> = record.keys().collect();
    if keys.len() > max_references {
        return Err(format!(
            "The \"reference\" section declares {} references; the limit is {max_references}.",
            keys.len()
        ));
    }
    let mut declarations = Vec::with_capacity(keys.len());
    for key in keys {
        let alias = validate_reference_alias(key).ok_or_else(|| {
            format!("The reference key \"{key}\" is not a valid alias (^[a-z][a-z0-9._-]{{1,63}}$).")
        })?;
        let parsed = parse_reference_declaration(&record[key])
            .map_err(|reason| format!("Reference \"{key}\": {reason}"))?;
        if parsed.alias != alias {
            return Err(format!(
                "Reference \"{key}\": the declared alias \"{}\" does not match its key.",
                parsed.alias
            ));
        }
        declarations.push(parsed);
    }
    Ok(declarations)
}

// ---------------------------------------------------------------------------
// Ports (core defines; adapters implement).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
/// ReferenceResolutionOutcome.
pub enum ReferenceResolutionOutcome {
    /// Resolved.
    Resolved {
        /// Resolved identity.
        identity: ResolvedReferenceIdentity,
    },
    /// Unavailable.
    Unavailable {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Refused.
    Refused {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Failed.
    Failed {
        /// Exact reference-failure reason.
        reason: String,
    },
}

/// Maps a declared source to a resolved immutable identity.
pub trait ReferenceResolverPort: Send + Sync {
    /// Resolved identity.
    fn resolve_identity(
        &self,
        source: &ReferenceSource,
        allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome;
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// MaterializationOutcome.
pub enum MaterializationOutcome {
    /// Materialized.
    Materialized {
        /// Private cache root (internal).
        root: String,
    },
    /// Unavailable.
    Unavailable {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Refused.
    Refused {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Failed.
    Failed {
        /// Exact reference-failure reason.
        reason: String,
    },
}

/// Manages the Siralos-owned private cache for references. A materialized
/// root is INTERNAL — never model-facing.
pub trait ReferenceMaterializerPort: Send + Sync {
    /// materialize.
    fn materialize(
        &self,
        reference_id: &str,
        identity: &ResolvedReferenceIdentity,
    ) -> MaterializationOutcome;
    /// Lifecycle status.
    fn status(&self, reference_id: &str) -> MaterializationStatus;
}

// ---------------------------------------------------------------------------
// Pure containment semantics.
// ---------------------------------------------------------------------------

fn collapse_slashes(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut last_was_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !last_was_slash {
                out.push(ch);
            }
            last_was_slash = true;
        } else {
            out.push(ch);
            last_was_slash = false;
        }
    }
    out
}

#[must_use]
fn normalize_absolute_path(path: &str) -> String {
    let collapsed = collapse_slashes(&path.replace('\\', "/"));
    let (prefix, body) = if collapsed.len() >= 2
        && collapsed.as_bytes()[1] == b':'
        && collapsed.as_bytes()[0].is_ascii_alphabetic()
    {
        (collapsed[..2].to_owned(), collapsed[2..].to_owned())
    } else if let Some(stripped) = collapsed.strip_prefix("//") {
        ("//".to_owned(), stripped.to_owned())
    } else {
        (String::new(), collapsed)
    };
    let mut out: Vec<&str> = Vec::new();
    for segment in body.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    if prefix == "//" {
        format!("//{}", out.join("/"))
    } else {
        format!("{prefix}/{}", out.join("/"))
    }
}

fn is_absolute_form(path: &str) -> bool {
    path.starts_with('/') || is_windows_form(path)
}

fn is_windows_form(path: &str) -> bool {
    let drive = path.len() >= 3
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'/'
        && path.as_bytes()[0].is_ascii_alphabetic();
    drive || path.starts_with("//")
}

/// Pure path containment check with `path.resolve`-like semantics for
/// absolute paths. Windows-form paths compare case-insensitively;
/// relative inputs fail closed (not within).
#[must_use]
pub fn is_path_within(root: &str, target: &str) -> bool {
    let root_norm = normalize_absolute_path(root);
    let target_norm = normalize_absolute_path(target);
    if !is_absolute_form(&root_norm) || !is_absolute_form(&target_norm) {
        return false;
    }
    if is_windows_form(&root_norm) && is_windows_form(&target_norm) {
        let root_lower = root_norm.to_lowercase();
        let target_lower = target_norm.to_lowercase();
        return target_lower == root_lower
            || target_lower.starts_with(&format!("{root_lower}/"));
    }
    target_norm == root_norm
        || target_norm.starts_with(&format!("{root_norm}/"))
}

fn identities_equal(
    a: &ResolvedReferenceIdentity,
    b: &ResolvedReferenceIdentity,
) -> bool {
    match (a, b) {
        (
            ResolvedReferenceIdentity::LocalDirectory {
                canonical_path: path_a,
                fingerprint: fp_a,
            },
            ResolvedReferenceIdentity::LocalDirectory {
                canonical_path: path_b,
                fingerprint: fp_b,
            },
        ) => path_a == path_b && fp_a == fp_b,
        (
            ResolvedReferenceIdentity::Repository {
                origin: origin_a,
                commit: commit_a,
                ..
            },
            ResolvedReferenceIdentity::Repository {
                origin: origin_b,
                commit: commit_b,
                ..
            },
        ) => origin_a == origin_b && commit_a == commit_b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Registry.
// ---------------------------------------------------------------------------

/// TrustForFn.
pub type TrustForFn =
    Arc<dyn Fn(&ReferenceDeclaration) -> ReferenceTrustClass + Send + Sync>;
/// NowFn.
pub type NowFn = Arc<dyn Fn() -> u64 + Send + Sync>;

/// ReferenceRegistryOptions.
pub struct ReferenceRegistryOptions {
    /// Pre-parsed declarations.
    pub declarations: Vec<ReferenceDeclaration>,
    /// Classify each declaration's trust (host policy input).
    pub trust_for: TrustForFn,
    /// Canonicalized workspace root; local-directory references must stay
    /// outside it.
    pub workspace_root: String,
    /// Injected resolver port.
    pub resolver: Arc<dyn ReferenceResolverPort>,
    /// When false (default), mutable repository refs are refused before
    /// the resolver runs.
    pub allow_mutable_refs: bool,
    /// Injected clock.
    pub now: NowFn,
    /// Effective limits.
    pub limits: ReferenceLimits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ReferenceRefreshResult.
pub enum ReferenceRefreshResult {
    /// Refreshed.
    Refreshed {
        /// Immutable revision value.
        revision: ReferenceRevision,
    },
    /// Unchanged.
    Unchanged {
        /// Immutable revision value.
        revision: ReferenceRevision,
    },
    /// Unavailable.
    Unavailable {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Refused.
    Refused {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Failed.
    Failed {
        /// Exact reference-failure reason.
        reason: String,
    },
}

struct ReferenceRecordInner {
    id: String,
    reference: Reference,
    revision: Option<ReferenceRevision>,
}

struct RegistryInner {
    records: Vec<ReferenceRecordInner>,
    by_alias: HashMap<String, usize>,
    by_id: HashMap<String, usize>,
    bindings: Vec<ReferenceTaskBinding>,
    limits: ReferenceLimits,
    workspace_root: String,
    allow_mutable_refs: bool,
    resolver: Arc<dyn ReferenceResolverPort>,
    now: NowFn,
}

/// The SINGLE application-owned owner of reference identity.
pub struct ReferenceRegistry {
    inner: RefCell<RegistryInner>,
}

impl ReferenceRegistry {
    /// record_index.
    fn record_index(inner: &RegistryInner, selector: &str) -> Option<usize> {
        inner
            .by_alias
            .get(selector)
            .or_else(|| inner.by_id.get(selector))
            .copied()
    }

    /// All declared references, in declaration order (declined included).
    #[must_use]
    pub fn list(&self) -> Vec<Reference> {
        let inner = self.inner.borrow();
        inner.records.iter().map(|r| r.reference.clone()).collect()
    }

    #[must_use]
    /// get.
    pub fn get(&self, selector: &str) -> Option<Reference> {
        let inner = self.inner.borrow();
        let index = Self::record_index(&inner, selector)?;
        Some(inner.records[index].reference.clone())
    }

    /// The CURRENT revision; never changes except via [`Self::refresh`].
    #[must_use]
    pub fn revision(&self, selector: &str) -> Option<ReferenceRevision> {
        let inner = self.inner.borrow();
        let index = Self::record_index(&inner, selector)?;
        inner.records[index].revision.clone()
    }

    /// Snapshot current revisions for all ready references (immutable).
    #[must_use]
    pub fn bind_task(&self, task_id: &str) -> ReferenceTaskBinding {
        let mut inner = self.inner.borrow_mut();
        let mut revisions = HashMap::new();
        for record in &inner.records {
            if record.reference.status == ReferenceStatus::Ready {
                if let Some(revision) = &record.revision {
                    revisions.insert(record.id.clone(), revision.clone());
                }
            }
        }
        let bound_at_ms = (inner.now)();
        let binding = ReferenceTaskBinding {
            task_id: task_id.to_owned(),
            revisions,
            bound_at_ms,
        };
        inner.bindings.push(binding.clone());
        while inner.bindings.len() > inner.limits.max_revision_bindings {
            inner.bindings.remove(0);
        }
        binding
    }

    /// Evicted bindings are no longer authoritative: they never serve
    /// their task-start revisions again.
    #[must_use]
    pub fn bound_revision(
        &self,
        binding: &ReferenceTaskBinding,
        selector: &str,
    ) -> Option<ReferenceRevision> {
        let inner = self.inner.borrow();
        if !inner.bindings.iter().any(|live| live.task_id == binding.task_id) {
            return None;
        }
        let index = Self::record_index(&inner, selector)?;
        let record = &inner.records[index];
        binding.revisions.get(&record.id).cloned()
    }

    /// Precise reason a declaration was declined; `None` otherwise.
    #[must_use]
    pub fn decline_reason(&self, selector: &str) -> Option<String> {
        let inner = self.inner.borrow();
        let index = Self::record_index(&inner, selector)?;
        let reference = &inner.records[index].reference;
        if reference.status == ReferenceStatus::Declined {
            reference.failure_reason.clone()
        } else {
            None
        }
    }

    #[must_use]
    /// size.
    pub fn size(&self) -> usize {
        self.inner.borrow().records.len()
    }

    /// The ONLY way a reference revision changes. A failed refresh
    /// invalidates the current revision (fail closed).
    pub fn refresh(&self, selector: &str) -> ReferenceRefreshResult {
        let index = {
            let inner = self.inner.borrow();
            match Self::record_index(&inner, selector) {
                Some(index) => index,
                None => {
                    return ReferenceRefreshResult::Failed {
                        reason: format!("Unknown reference: {selector}"),
                    };
                }
            }
        };
        let (status, decline) = {
            let inner = self.inner.borrow();
            let reference = &inner.records[index].reference;
            (reference.status, reference.failure_reason.clone())
        };
        if status == ReferenceStatus::Declined {
            return ReferenceRefreshResult::Refused {
                reason: decline
                    .unwrap_or_else(|| "the reference is declined".to_owned()),
            };
        }
        let (source, allow_mutable_refs, workspace_root, resolver, now) = {
            let inner = self.inner.borrow();
            (
                inner.records[index].reference.source.clone(),
                inner.allow_mutable_refs,
                inner.workspace_root.clone(),
                inner.resolver.clone(),
                inner.now.clone(),
            )
        };
        let outcome = resolver.resolve_identity(&source, allow_mutable_refs);
        match outcome {
            ReferenceResolutionOutcome::Resolved { identity } => {
                if let (
                    ReferenceSource::LocalDirectory { .. },
                    ResolvedReferenceIdentity::LocalDirectory {
                        canonical_path,
                        ..
                    },
                ) = (&source, &identity)
                {
                    if is_path_within(&workspace_root, canonical_path) {
                        let mut inner = self.inner.borrow_mut();
                        let record = &mut inner.records[index];
                        record.reference = Reference {
                            status: ReferenceStatus::Declined,
                            failure_reason: Some(
                                WORKSPACE_CONTAINMENT_REFUSAL.to_owned(),
                            ),
                            ..record.reference.clone()
                        };
                        record.revision = None;
                        return ReferenceRefreshResult::Refused {
                            reason: WORKSPACE_CONTAINMENT_REFUSAL.to_owned(),
                        };
                    }
                }
                let revision =
                    ReferenceRevision { identity, resolved_at_ms: (now)() };
                let mut inner = self.inner.borrow_mut();
                let record = &mut inner.records[index];
                if let Some(current) = &record.revision {
                    if identities_equal(&current.identity, &revision.identity)
                    {
                        record.reference = Reference {
                            status: ReferenceStatus::Ready,
                            failure_reason: None,
                            ..record.reference.clone()
                        };
                        return ReferenceRefreshResult::Unchanged {
                            revision: current.clone(),
                        };
                    }
                }
                record.revision = Some(revision.clone());
                record.reference = Reference {
                    status: ReferenceStatus::Ready,
                    failure_reason: None,
                    ..record.reference.clone()
                };
                ReferenceRefreshResult::Refreshed { revision }
            }
            ReferenceResolutionOutcome::Unavailable { reason } => {
                Self::apply_failure(
                    &mut self.inner.borrow_mut(),
                    index,
                    ReferenceStatus::Unavailable,
                    &reason,
                );
                ReferenceRefreshResult::Unavailable { reason }
            }
            ReferenceResolutionOutcome::Refused { reason } => {
                Self::apply_failure(
                    &mut self.inner.borrow_mut(),
                    index,
                    ReferenceStatus::Declined,
                    &reason,
                );
                ReferenceRefreshResult::Refused { reason }
            }
            ReferenceResolutionOutcome::Failed { reason } => {
                Self::apply_failure(
                    &mut self.inner.borrow_mut(),
                    index,
                    ReferenceStatus::ResolutionFailed,
                    &reason,
                );
                ReferenceRefreshResult::Failed { reason }
            }
        }
    }

    /// apply_failure.
    fn apply_failure(
        inner: &mut RegistryInner,
        index: usize,
        status: ReferenceStatus,
        reason: &str,
    ) {
        let record = &mut inner.records[index];
        record.reference = Reference {
            status,
            failure_reason: Some(reason.to_owned()),
            ..record.reference.clone()
        };
        record.revision = None;
    }
}

fn resolve_declaration(
    declaration: &ReferenceDeclaration,
    trust_for: &TrustForFn,
    workspace_root: &str,
    allow_mutable_refs: bool,
    resolver: &Arc<dyn ReferenceResolverPort>,
    now: &NowFn,
) -> (Reference, Option<ReferenceRevision>) {
    let id = create_reference_id(&declaration.alias);
    let trust = trust_for(declaration);
    let declined = |reason: &str| Reference {
        id: id.clone(),
        alias: declaration.alias.clone(),
        kind: declaration.kind,
        source: declaration.source.clone(),
        trust,
        description: declaration.description.clone(),
        status: ReferenceStatus::Declined,
        failure_reason: Some(reason.to_owned()),
    };
    if validate_reference_alias(&declaration.alias).is_none() {
        return (declined(INVALID_ALIAS_REASON), None);
    }
    if let ReferenceSource::Repository {
        r#ref: RepositoryRef::Branch { .. },
        ..
    } = &declaration.source
    {
        if !allow_mutable_refs {
            return (declined(MUTABLE_REF_REFUSAL), None);
        }
    }
    let outcome =
        resolver.resolve_identity(&declaration.source, allow_mutable_refs);
    match outcome {
        ReferenceResolutionOutcome::Resolved { identity } => {
            if let (
                ReferenceSource::LocalDirectory { .. },
                ResolvedReferenceIdentity::LocalDirectory {
                    canonical_path,
                    ..
                },
            ) = (&declaration.source, &identity)
            {
                if is_path_within(workspace_root, canonical_path) {
                    return (declined(WORKSPACE_CONTAINMENT_REFUSAL), None);
                }
            }
            let reference = Reference {
                id,
                alias: declaration.alias.clone(),
                kind: declaration.kind,
                source: declaration.source.clone(),
                trust,
                description: declaration.description.clone(),
                status: ReferenceStatus::Ready,
                failure_reason: None,
            };
            let revision =
                ReferenceRevision { identity, resolved_at_ms: now() };
            (reference, Some(revision))
        }
        ReferenceResolutionOutcome::Unavailable { reason } => {
            let mut reference = declined(&reason);
            reference.status = ReferenceStatus::Unavailable;
            (reference, None)
        }
        ReferenceResolutionOutcome::Refused { reason } => {
            (declined(&reason), None)
        }
        ReferenceResolutionOutcome::Failed { reason } => {
            let mut reference = declined(&reason);
            reference.status = ReferenceStatus::ResolutionFailed;
            (reference, None)
        }
    }
}

/// Resolve every declared reference, record outcomes in declaration
/// order, and decline duplicate aliases (first occurrence wins and stays
/// addressable; duplicates stay listed for audit without occupying the
/// lookup maps).
pub fn create_reference_registry(
    options: ReferenceRegistryOptions,
) -> ReferenceRegistry {
    let ReferenceRegistryOptions {
        declarations,
        trust_for,
        workspace_root,
        resolver,
        allow_mutable_refs,
        now,
        limits,
    } = options;

    let mut records: Vec<ReferenceRecordInner> = Vec::new();
    let mut by_alias: HashMap<String, usize> = HashMap::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    let mut seen_aliases: HashSet<String> = HashSet::new();
    for declaration in &declarations {
        let (reference, revision) = resolve_declaration(
            declaration,
            &trust_for,
            &workspace_root,
            allow_mutable_refs,
            &resolver,
            &now,
        );
        let duplicate = !seen_aliases.insert(declaration.alias.clone());
        let declined_duplicate = duplicate.then(|| {
            let mut declined = reference.clone();
            declined.status = ReferenceStatus::Declined;
            declined.failure_reason = Some(DUPLICATE_ALIAS_REASON.to_owned());
            declined
        });
        let stored = declined_duplicate.unwrap_or(reference);
        if !duplicate {
            let index = records.len();
            by_alias.insert(stored.alias.clone(), index);
            by_id.insert(stored.id.clone(), index);
        }
        records.push(ReferenceRecordInner {
            id: stored.id.clone(),
            reference: stored,
            revision: if duplicate { None } else { revision },
        });
    }

    ReferenceRegistry {
        inner: RefCell::new(RegistryInner {
            records,
            by_alias,
            by_id,
            bindings: Vec::new(),
            limits,
            workspace_root,
            allow_mutable_refs,
            resolver,
            now,
        }),
    }
}

// ---------------------------------------------------------------------------
// Evidence views.
// ---------------------------------------------------------------------------

/// Model-facing bounded view of one reference observation.
#[derive(Debug, Clone)]
pub struct ReferenceEvidenceView {
    /// Stable identifier.
    pub reference_id: String,
    /// Validated alias.
    pub alias: String,
    /// Immutable revision value.
    pub revision: ReferenceRevision,
    /// Path relative to the owning root.
    pub path: String,
    /// Observation operation.
    pub operation: String,
    /// Read mode when applicable.
    pub mode: Option<String>,
    /// Content hash when applicable.
    pub sha256: Option<String>,
    /// Stable identifier.
    pub evidence_id: Option<String>,
}

/// Deterministic anchor of a revision: resolved commit for repositories,
/// fingerprint for local directories.
#[must_use]
pub fn reference_identity_anchor(revision: &ReferenceRevision) -> String {
    match &revision.identity {
        ResolvedReferenceIdentity::Repository { commit, .. } => commit.clone(),
        ResolvedReferenceIdentity::LocalDirectory { fingerprint, .. } => {
            fingerprint.clone()
        }
    }
}

/// One-line bounded rendering:
/// `@reference/<alias> @ <anchor> <path> (<operation>[, mode])`.
#[must_use]
pub fn format_reference_evidence_line(view: &ReferenceEvidenceView) -> String {
    let mode = view
        .mode
        .as_deref()
        .map_or_else(String::new, |mode| format!(", {mode}"));
    format!(
        "@reference/{} @ {} {} ({}{})",
        view.alias,
        reference_identity_anchor(&view.revision),
        view.path,
        view.operation,
        mode
    )
}

/// Default bound for the two-line reference evidence view.
pub const DEFAULT_REFERENCE_VIEW_MAX_BYTES: usize = 1024;

/// Two-line bounded rendering:
/// `@reference/<alias> @ <anchor> <path> (<operation>[, mode])\nEvidence: <id>`.
#[must_use]
pub fn format_reference_evidence_view(
    view: &ReferenceEvidenceView,
    max_bytes: Option<usize>,
) -> String {
    let text = format!(
        "{}\nEvidence: {}",
        format_reference_evidence_line(view),
        view.evidence_id.as_deref().unwrap_or("-")
    );
    crate::projection::evidence::truncate_text(
        &text,
        max_bytes.unwrap_or(DEFAULT_REFERENCE_VIEW_MAX_BYTES),
    )
    .0
}
