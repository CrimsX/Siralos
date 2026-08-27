//! Research source model, request validation, the application-owned
//! research service (denied-by-default policy gate), evidence retention,
//! and evidence view rendering (Stage 3 milestone 5; R13.3 external
//! knowledge boundaries).
//!
//! Mirrors `packages/core/src/research/*`. Research is bounded,
//! host-coordinated fetching of external reference material into typed
//! documents with provenance. The service gates on the capability policy
//! BEFORE any source port is invoked (`research.fetch` must evaluate to
//! `allow`; there is no approval protocol for research, so `ask` is
//! refused too), validates every request, and produces bounded evidence.
//! Provider output is untrusted; research never becomes knowledge without
//! an explicit propose call (owned by the knowledge coordinator).

use crate::identity::{CanonicalValue, compute_artifact_digest_hex};
use crate::identity::{canonicalize_json, sha256_hex_str};
use crate::security::{
    CapabilityPolicy, PermissionDecision, SandboxProfile, evaluate_permission,
};
use serde_json::json;
use std::cell::RefCell;
use std::sync::Arc;

/// RESEARCH_LIMITS.
pub const RESEARCH_LIMITS: ResearchLimitSet = ResearchLimitSet {
    max_download_bytes: 2 * 1024 * 1024,
    max_document_bytes: 256 * 1024,
    max_sections: 64,
    max_links: 32,
    max_heading_bytes: 512,
    max_section_text_bytes: 32 * 1024,
    max_redirects: 4,
    timeout_ms: 10_000,
    hard_lifetime_ms: 30_000,
    max_research_evidence_excerpt_bytes: 4096,
    max_retained_evidence_views: 8,
};

/// Absolute limits mirrored from `RESEARCH_LIMITS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchLimitSet {
    /// Bound mirrored from the corresponding limits table.
    pub max_download_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_document_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_sections: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_links: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_heading_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_section_text_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_redirects: usize,
    /// Epoch milliseconds (injected clock).
    pub timeout_ms: u64,
    /// Epoch milliseconds (injected clock).
    pub hard_lifetime_ms: u64,
    /// Bound mirrored from the corresponding limits table.
    pub max_research_evidence_excerpt_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_retained_evidence_views: usize,
}

/// Bounded per-service bounds (normalized at construction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchBounds {
    /// Bound mirrored from the corresponding limits table.
    pub max_download_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_document_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_sections: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_links: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_heading_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_section_text_bytes: usize,
    /// Bound mirrored from the corresponding limits table.
    pub max_redirects: usize,
    /// Epoch milliseconds (injected clock).
    pub timeout_ms: u64,
    /// Epoch milliseconds (injected clock).
    pub hard_lifetime_ms: u64,
}

#[must_use]
fn clamp(
    value: Option<usize>,
    fallback: usize,
    minimum: usize,
    maximum: usize,
) -> usize {
    match value {
        None => fallback,
        Some(value) => value.clamp(minimum, maximum),
    }
}

/// Normalize bounds exactly like `normalizeResearchBounds`.
#[must_use]
pub fn normalize_research_bounds(input: ResearchBounds) -> ResearchBounds {
    let timeout_ms = clamp(
        Some(input.timeout_ms as usize),
        RESEARCH_LIMITS.timeout_ms as usize,
        1,
        RESEARCH_LIMITS.timeout_ms as usize,
    ) as u64;
    ResearchBounds {
        max_download_bytes: clamp(
            Some(input.max_download_bytes),
            RESEARCH_LIMITS.max_download_bytes,
            1,
            RESEARCH_LIMITS.max_download_bytes,
        ),
        max_document_bytes: clamp(
            Some(input.max_document_bytes),
            RESEARCH_LIMITS.max_document_bytes,
            1,
            RESEARCH_LIMITS.max_document_bytes,
        ),
        max_sections: clamp(
            Some(input.max_sections),
            RESEARCH_LIMITS.max_sections,
            1,
            RESEARCH_LIMITS.max_sections,
        ),
        max_links: clamp(
            Some(input.max_links),
            RESEARCH_LIMITS.max_links,
            0,
            RESEARCH_LIMITS.max_links,
        ),
        max_heading_bytes: clamp(
            Some(input.max_heading_bytes),
            RESEARCH_LIMITS.max_heading_bytes,
            1,
            RESEARCH_LIMITS.max_heading_bytes,
        ),
        max_section_text_bytes: clamp(
            Some(input.max_section_text_bytes),
            RESEARCH_LIMITS.max_section_text_bytes,
            1,
            RESEARCH_LIMITS.max_section_text_bytes,
        ),
        max_redirects: clamp(
            Some(input.max_redirects),
            RESEARCH_LIMITS.max_redirects,
            0,
            RESEARCH_LIMITS.max_redirects,
        ),
        timeout_ms,
        hard_lifetime_ms: clamp(
            Some(input.hard_lifetime_ms as usize),
            RESEARCH_LIMITS.hard_lifetime_ms as usize,
            1,
            RESEARCH_LIMITS.hard_lifetime_ms as usize,
        )
        .max(timeout_ms as usize) as u64,
    }
}

#[must_use]
/// default_research_bounds.
pub fn default_research_bounds() -> ResearchBounds {
    ResearchBounds {
        max_download_bytes: RESEARCH_LIMITS.max_download_bytes,
        max_document_bytes: RESEARCH_LIMITS.max_document_bytes,
        max_sections: RESEARCH_LIMITS.max_sections,
        max_links: RESEARCH_LIMITS.max_links,
        max_heading_bytes: RESEARCH_LIMITS.max_heading_bytes,
        max_section_text_bytes: RESEARCH_LIMITS.max_section_text_bytes,
        max_redirects: RESEARCH_LIMITS.max_redirects,
        timeout_ms: RESEARCH_LIMITS.timeout_ms,
        hard_lifetime_ms: RESEARCH_LIMITS.hard_lifetime_ms,
    }
}

// ---------------------------------------------------------------------------
// Model types.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Discriminant kind.
pub enum ResearchSourceKind {
    /// Repository.
    Repository,
    /// GodotDocs.
    GodotDocs,
    /// Fake.
    Fake,
}

impl ResearchSourceKind {
    #[must_use]
    /// as_str.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repository => "repository",
            Self::GodotDocs => "godot-docs",
            Self::Fake => "fake",
        }
    }

    #[must_use]
    /// parse.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "repository" => Some(Self::Repository),
            "godot-docs" => Some(Self::GodotDocs),
            "fake" => Some(Self::Fake),
            _ => None,
        }
    }
}

/// Reference to one configured research source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchSourceRef {
    /// Discriminant kind.
    pub kind: ResearchSourceKind,
    /// Stable identifier.
    pub id: String,
    /// Human/model-facing label.
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ResearchContentType.
pub enum ResearchContentType {
    /// TextMarkdown.
    TextMarkdown,
    /// TextPlain.
    TextPlain,
    /// ApplicationJson.
    ApplicationJson,
    /// TextHtml.
    TextHtml,
}

impl ResearchContentType {
    #[must_use]
    /// as_str.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TextMarkdown => "text/markdown",
            Self::TextPlain => "text/plain",
            Self::ApplicationJson => "application/json",
            Self::TextHtml => "text/html",
        }
    }

    #[must_use]
    /// parse.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text/markdown" => Some(Self::TextMarkdown),
            "text/plain" => Some(Self::TextPlain),
            "application/json" => Some(Self::ApplicationJson),
            "text/html" => Some(Self::TextHtml),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ResearchSection.
pub struct ResearchSection {
    /// Section heading; `None` when the document has no headings.
    pub heading: Option<String>,
    /// Section text.
    pub text: String,
    /// UTF-8 bytes of `text`.
    pub byte_length: usize,
}

/// Provenance of one fetched document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchProvenance {
    /// Configured source reference.
    pub source: ResearchSourceRef,
    /// The pin as declared.
    pub requested_ref: Option<String>,
    /// Immutable revision value.
    pub resolved_revision: Option<String>,
    /// requested_version.
    pub requested_version: Option<String>,
    /// used_version.
    pub used_version: Option<String>,
    /// Whether a fallback was served.
    pub fallback: bool,
    /// Exact reference-failure reason.
    pub fallback_reason: Option<String>,
    /// Epoch milliseconds (injected clock).
    pub fetched_at_ms: u64,
    /// Resource identifier within the source (path, page id, ...).
    pub resource: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ResearchDocument.
pub struct ResearchDocument {
    /// Stable identifier.
    pub id: String,
    /// Configured source reference.
    pub source: ResearchSourceRef,
    /// Document title.
    pub title: Option<String>,
    /// Epoch milliseconds (injected clock).
    pub fetched_at_ms: u64,
    /// Classified content type.
    pub content_type: ResearchContentType,
    /// Normalized sections.
    pub sections: Vec<ResearchSection>,
    /// Extracted links (url, title).
    pub links: Vec<(String, Option<String>)>,
    /// Fetch provenance record.
    pub provenance: ResearchProvenance,
    /// Whether truncation occurred.
    pub truncated: bool,
    /// Exact reference-failure reason.
    pub truncation_reason: Option<String>,
    /// UTF-8 byte length of the content.
    pub byte_length: usize,
    /// Canonical digest of the normalized content (ADR 0028), computed
    /// AFTER final truncation.
    pub content_digest: String,
    /// SHA-256 of the raw fetched text; `None` when unavailable.
    pub raw_artifact_digest: Option<String>,
}

/// Deterministic document id: `rd_` + 24 hex chars from the source id and
/// the request digest. Identical requests to the same source produce the
/// same document id.
#[must_use]
pub fn compute_research_document_id(
    source_id: &str,
    request_digest: &str,
) -> String {
    let digest = sha256_hex_str(&canonicalize_json(&json!({
        "sourceId": source_id,
        "requestDigest": request_digest,
    })));
    format!("rd_{}", &digest[..24])
}

/// Canonical digest of the normalized research content (ADR 0028).
pub fn compute_research_document_content_digest(
    title: Option<&str>,
    content_type: ResearchContentType,
    sections: &[ResearchSection],
) -> Result<String, crate::identity::ArtifactIdentityError> {
    let payload = CanonicalValue::Object(sections_to_canonical_map(
        title,
        content_type,
        sections,
    ));
    compute_artifact_digest_hex("ResearchDocument", 1, &payload)
}

fn canonical_string_option(value: Option<&str>) -> CanonicalValue {
    match value {
        Some(text) => CanonicalValue::Str(text.to_owned()),
        None => CanonicalValue::Null,
    }
}

fn sections_to_canonical_map(
    title: Option<&str>,
    content_type: ResearchContentType,
    sections: &[ResearchSection],
) -> std::collections::BTreeMap<String, CanonicalValue> {
    use std::collections::BTreeMap;
    let mut map = BTreeMap::new();
    map.insert("title".to_owned(), canonical_string_option(title));
    map.insert(
        "contentType".to_owned(),
        CanonicalValue::Str(content_type.as_str().to_owned()),
    );
    map.insert(
        "sections".to_owned(),
        CanonicalValue::Array(
            sections
                .iter()
                .map(|section| {
                    CanonicalValue::Object(BTreeMap::from([
                        (
                            "byteLength".to_owned(),
                            CanonicalValue::U64(section.byte_length as u64),
                        ),
                        (
                            "heading".to_owned(),
                            canonical_string_option(
                                section.heading.as_deref(),
                            ),
                        ),
                        (
                            "text".to_owned(),
                            CanonicalValue::Str(section.text.clone()),
                        ),
                    ]))
                })
                .collect(),
        ),
    );
    map
}

/// Validated research request.
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchRequest {
    /// Configured source reference.
    pub source: ResearchSourceRef,
    /// Required: non-empty, <= 512 bytes.
    pub query: String,
    /// <= 256 bytes; `None` when unused.
    pub topic: Option<String>,
    /// Reference-relative resource path; relative with no "..", <= 1024 chars.
    pub path: Option<String>,
    /// Git ref pin, <= 256 chars.
    pub r#ref: Option<String>,
    /// Version pin matching the reference version pattern, <= 64 chars.
    pub version: Option<String>,
    /// Optional hard download cap override (bounded by `maxDownloadBytes`).
    pub max_bytes: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ResearchOutcome.
pub enum ResearchOutcome {
    /// Document.
    Document {
        /// document.
        document: Box<ResearchDocument>,
    },
    /// Refused.
    Refused {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// UnsupportedContent.
    UnsupportedContent {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Oversized.
    Oversized {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Timeout.
    Timeout,
    /// Cancelled.
    Cancelled,
    /// Unavailable.
    Unavailable {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Failed.
    Failed {
        /// Exact reference-failure reason.
        reason: String,
    },
}

/// Structural validation of a source reference (id/label bounds).
#[must_use]
pub fn is_valid_research_source_ref(
    kind: &str,
    id: &str,
    label: &str,
) -> bool {
    ResearchSourceKind::parse(kind).is_some()
        && !id.is_empty()
        && id.len() <= 128
        && !label.is_empty()
        && label.len() <= 128
}

fn is_valid_version(version: &str) -> bool {
    // ^[0-9]+(\.[0-9]+){0,3}([.-][A-Za-z][A-Za-z0-9.-]*)?$
    let bytes = version.as_bytes();
    if bytes.is_empty() || version.len() > 64 {
        return false;
    }
    let mut index = 0usize;
    let mut numeric_groups = 0usize;
    while numeric_groups < 4 {
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return false;
        }
        numeric_groups += 1;
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            continue;
        }
        break;
    }
    if index == bytes.len() {
        return true;
    }
    let separator = bytes[index];
    if separator != b'-' && separator != b'.' {
        return false;
    }
    index += 1;
    if index >= bytes.len() || !bytes[index].is_ascii_alphabetic() {
        return false;
    }
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-') {
            return false;
        }
        index += 1;
    }
    true
}

/// Validate an untrusted research request against the bounded model. The
/// service runs this before any source port is invoked.
pub fn validate_research_request(
    input: &serde_json::Value,
) -> Result<ResearchRequest, String> {
    let record =
        input.as_object().ok_or("A research request must be an object.")?;
    let source = record.get("source").ok_or(
        "The research request requires a valid source (kind, id, label).",
    )?;
    let (kind, id, label) = match source {
        serde_json::Value::Object(map) => {
            let kind = map
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let id = map
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let label = map
                .get("label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if !is_valid_research_source_ref(kind, id, label) {
                return Err(
                    "The research request requires a valid source (kind, id, label).".to_owned(),
                );
            }
            (
                ResearchSourceKind::parse(kind).expect("checked above"),
                id.to_owned(),
                label.to_owned(),
            )
        }
        _ => return Err(
            "The research request requires a valid source (kind, id, label)."
                .to_owned(),
        ),
    };
    let query = record
        .get("query")
        .and_then(serde_json::Value::as_str)
        .ok_or("A research request requires a non-empty query.")?;
    if query.trim().is_empty() {
        return Err(
            "A research request requires a non-empty query.".to_owned()
        );
    }
    if query.len() > 512 {
        return Err(
            "The research query exceeds the limit of 512 bytes.".to_owned()
        );
    }
    for key in ["topic", "path", "ref", "version"] {
        if let Some(value) = record.get(key) {
            if !value.is_null() && !value.is_string() {
                return Err(format!(
                    "The research {key} must be a string or null."
                ));
            }
        }
    }
    let optional_string = |key: &str| -> Option<String> {
        record.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
    };
    let topic = optional_string("topic");
    if let Some(topic) = &topic {
        if topic.len() > 256 {
            return Err("The research topic exceeds the limit of 256 bytes."
                .to_owned());
        }
    }
    let path = optional_string("path");
    if let Some(path) = &path {
        if path.chars().count() > 1024 {
            return Err(
                "The research path exceeds the limit of 1024 characters."
                    .to_owned(),
            );
        }
        if path.starts_with('/') || path.contains('\\') || path.contains('\0')
        {
            return Err(
                "The research path must be relative with forward slashes."
                    .to_owned(),
            );
        }
        if path.split('/').any(|segment| segment == ".." || segment == ".") {
            return Err(
                "The research path must not contain \"..\" or \".\" segments."
                    .to_owned(),
            );
        }
    }
    let r#ref = optional_string("ref");
    if let Some(r#ref) = &r#ref {
        if r#ref.chars().count() > 256 {
            return Err(
                "The research ref exceeds the limit of 256 characters."
                    .to_owned(),
            );
        }
    }
    let version = optional_string("version");
    if let Some(version) = &version {
        if version.chars().count() > 64 || !is_valid_version(version) {
            return Err(format!(
                "The research version \"{version}\" is malformed; versions look like 4.3 or 4.3-stable."
            ));
        }
    }
    let mut max_bytes: Option<f64> = None;
    if let Some(value) = record.get("maxBytes") {
        if !value.is_null() {
            let number = value.as_f64().ok_or(
                "The research maxBytes must be a positive number or null.",
            )?;
            if !number.is_finite() || number <= 0.0 {
                return Err(
                    "The research maxBytes must be a positive number or null."
                        .to_owned(),
                );
            }
            max_bytes = Some(number.floor());
        }
    }
    Ok(ResearchRequest {
        source: ResearchSourceRef { kind, id, label },
        query: query.to_owned(),
        topic,
        path,
        r#ref,
        version,
        max_bytes,
    })
}

// ---------------------------------------------------------------------------
// Service model.
// ---------------------------------------------------------------------------

/// Exact task identity captured around one asynchronous research request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchTaskBinding {
    /// Stable identifier.
    pub task_id: String,
    /// Immutable revision value.
    pub task_contract_revision: u64,
}

#[must_use]
/// is_valid_research_task_binding.
pub fn is_valid_research_task_binding(
    binding: Option<&ResearchTaskBinding>,
) -> bool {
    const MAX_SAFE_INTEGER: u64 = (1u64 << 53) - 1;
    let Some(binding) = binding else {
        return false;
    };
    !binding.task_id.trim().is_empty()
        && binding.task_id.len() <= 128
        && binding.task_contract_revision >= 1
        && binding.task_contract_revision <= MAX_SAFE_INTEGER
}

#[must_use]
fn same_research_task_binding(
    expected: &ResearchTaskBinding,
    current: Option<&ResearchTaskBinding>,
) -> bool {
    match current {
        Some(current) => {
            current.task_id == expected.task_id
                && current.task_contract_revision
                    == expected.task_contract_revision
        }
        None => false,
    }
}

/// Deterministic request id bound to the exact request + task + sequence.
#[must_use]
pub fn create_research_request_id(
    request: &ResearchRequest,
    task: &ResearchTaskBinding,
    sequence: u64,
) -> String {
    let request_digest = sha256_hex_str(&canonicalize_json(&json!({
        "source": {
            "kind": request.source.kind.as_str(),
            "id": request.source.id,
            "label": request.source.label,
        },
        "query": request.query,
        "topic": request.topic,
        "path": request.path,
        "ref": request.r#ref,
        "version": request.version,
    })));
    let digest = sha256_hex_str(&canonicalize_json(&json!({
        "requestDigest": request_digest,
        "taskId": task.task_id,
        "taskContractRevision": task.task_contract_revision,
        "sequence": sequence,
    })));
    format!("req_{}", &digest[..24])
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ResearchFetchResult.
pub enum ResearchFetchResult {
    /// Refused.
    Refused {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Document.
    Document {
        /// The normalized document.
        document: Box<ResearchDocument>,
        /// The retained evidence entry.
        evidence: Box<ResearchEvidence>,
    },
    /// UnsupportedContent.
    UnsupportedContent {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Oversized.
    Oversized {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Timeout.
    Timeout {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Cancelled.
    Cancelled {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Stale.
    Stale {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Unavailable.
    Unavailable {
        /// Exact reference-failure reason.
        reason: String,
    },
    /// Failed.
    Failed {
        /// Exact reference-failure reason.
        reason: String,
    },
}

/// One retained model-facing research evidence excerpt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchEvidence {
    /// Stable identifier.
    pub evidence_id: String,
    /// Stable identifier.
    pub request_id: String,
    /// Stable identifier.
    pub task_id: String,
    /// Immutable revision value.
    pub task_contract_revision: u64,
    /// Configured source reference.
    pub source: ResearchSourceRef,
    /// Epoch milliseconds (injected clock).
    pub fetched_at_ms: u64,
    /// Immutable revision value.
    pub resolved_revision: Option<String>,
    /// Optional version pin.
    pub version: Option<String>,
    /// Whether a fallback was served.
    pub fallback: bool,
    /// Bounded first-section excerpt.
    pub excerpt: String,
    /// Whether truncation occurred.
    pub truncated: bool,
    /// UTF-8 byte length of the content.
    pub byte_length: usize,
}

// ---------------------------------------------------------------------------
// Source port.
// ---------------------------------------------------------------------------

/// Read-only cancellation observation; sources can never mutate host
/// cancellation state through it.
#[derive(Debug, Clone, Copy, Default)]
pub struct CancellationSignal {
    /// Whether cancellation was observed.
    pub aborted: bool,
}

/// A research source port fetches one bounded document for one request
/// and returns a typed outcome — never throws.
pub trait ResearchSourcePort: Send + Sync {
    /// Discriminant kind.
    fn kind(&self) -> ResearchSourceKind;
    /// Stable identifier.
    fn id(&self) -> &str;
    /// Human/model-facing label.
    fn label(&self) -> &str;
    /// fetch.
    fn fetch(
        &self,
        request: &ResearchRequest,
        bounds: &ResearchBounds,
        signal: &CancellationSignal,
    ) -> ResearchOutcome;
}

// ---------------------------------------------------------------------------
// Evidence store (bounded FIFO retention).
// ---------------------------------------------------------------------------

struct EvidenceStoreState {
    entries: Vec<ResearchEvidence>,
    sequence: u64,
    total_bytes: usize,
}

/// Bounded FIFO retention for model-facing research excerpts.
struct ResearchEvidenceStore {
    max_evidence_bytes: usize,
    state: RefCell<EvidenceStoreState>,
}

impl ResearchEvidenceStore {
    /// new.
    fn new(max_evidence_bytes: usize) -> Self {
        Self {
            max_evidence_bytes,
            state: RefCell::new(EvidenceStoreState {
                entries: Vec::new(),
                sequence: 0,
                total_bytes: 0,
            }),
        }
    }

    /// record.
    fn record(
        &self,
        document: &ResearchDocument,
        request_id: &str,
        task: &ResearchTaskBinding,
    ) -> ResearchEvidence {
        let (excerpt, truncated) = crate::projection::evidence::truncate_text(
            document.sections.first().map_or("", |s| s.text.as_str()),
            RESEARCH_LIMITS.max_research_evidence_excerpt_bytes,
        );
        let mut state = self.state.borrow_mut();
        state.sequence += 1;
        let entry = ResearchEvidence {
            evidence_id: format!("ev-research-{}", state.sequence),
            request_id: request_id.to_owned(),
            task_id: task.task_id.clone(),
            task_contract_revision: task.task_contract_revision,
            source: document.source.clone(),
            fetched_at_ms: document.fetched_at_ms,
            resolved_revision: document.provenance.resolved_revision.clone(),
            version: document.provenance.used_version.clone(),
            fallback: document.provenance.fallback,
            byte_length: excerpt.len(),
            excerpt,
            truncated,
        };
        state.total_bytes += entry.byte_length;
        state.entries.push(entry);
        while state.entries.len() > RESEARCH_LIMITS.max_retained_evidence_views
            || state.total_bytes > self.max_evidence_bytes
        {
            let Some(dropped) = state.entries.first().cloned() else {
                break;
            };
            state.entries.remove(0);
            state.total_bytes -= dropped.byte_length;
        }
        state.entries.last().expect("just pushed").clone()
    }

    /// snapshots.
    fn snapshots(&self) -> Vec<ResearchEvidence> {
        self.state.borrow().entries.clone()
    }
}

// ---------------------------------------------------------------------------
// Service.
// ---------------------------------------------------------------------------

/// DEFAULT_RESEARCH_VIEW_MAX_BYTES.
pub const DEFAULT_RESEARCH_VIEW_MAX_BYTES: usize = 2 * 1024;

/// ResearchServiceOptions.
pub struct ResearchServiceOptions {
    /// Capability policy input.
    pub policy: CapabilityPolicy,
    /// Sandbox profile input.
    pub profile: SandboxProfile,
    /// Configured source ports.
    pub sources: Vec<Arc<dyn ResearchSourcePort>>,
    /// Current active task identity; `None` means research must fail closed.
    pub current_task:
        Arc<dyn Fn() -> Option<ResearchTaskBinding> + Send + Sync>,
    /// Normalized bounds.
    pub bounds: ResearchBounds,
    /// Total retained evidence-excerpt budget for the ring.
    pub max_evidence_bytes: Option<usize>,
}

/// The application-owned research coordinator — the single gate for
/// research fetches.
pub struct ResearchService {
    policy: CapabilityPolicy,
    profile: SandboxProfile,
    sources: Vec<Arc<dyn ResearchSourcePort>>,
    current_task: Arc<dyn Fn() -> Option<ResearchTaskBinding> + Send + Sync>,
    bounds: ResearchBounds,
    evidence_store: ResearchEvidenceStore,
    state: RefCell<ServiceState>,
}

struct ServiceState {
    request_counter: u64,
    active_requests: u64,
}

impl ResearchService {
    #[must_use]
    /// new.
    pub fn new(options: ResearchServiceOptions) -> Self {
        let bounds = normalize_research_bounds(options.bounds);
        let default_budget = RESEARCH_LIMITS.max_retained_evidence_views
            * RESEARCH_LIMITS.max_research_evidence_excerpt_bytes;
        let max_evidence_bytes = options
            .max_evidence_bytes
            .unwrap_or(default_budget)
            .clamp(0, default_budget);
        Self {
            policy: options.policy,
            profile: options.profile,
            sources: options.sources,
            current_task: options.current_task,
            bounds,
            evidence_store: ResearchEvidenceStore::new(max_evidence_bytes),
            state: RefCell::new(ServiceState {
                request_counter: 0,
                active_requests: 0,
            }),
        }
    }

    /// Configured source reference.
    fn find_source(
        &self,
        request: &ResearchRequest,
    ) -> Option<Arc<dyn ResearchSourcePort>> {
        for source in &self.sources {
            if source.kind() == request.source.kind
                && source.id() == request.source.id
            {
                return Some(source.clone());
            }
        }
        for source in &self.sources {
            if source.kind() == request.source.kind
                && source.label() == request.source.label
            {
                return Some(source.clone());
            }
        }
        None
    }

    /// Fetch one bounded document under the full gate order:
    /// permission → validation → configured source → active task →
    /// pre-abort → fetch → staleness → evidence retention.
    pub fn fetch(
        &self,
        request: &serde_json::Value,
        signal: &CancellationSignal,
    ) -> ResearchFetchResult {
        // 1. Policy gate FIRST — the source port is never invoked when the
        //    gate does not allow (effect-test contract).
        let permission =
            evaluate_permission("research.fetch", &self.policy, &self.profile);
        if permission != PermissionDecision::Allow {
            return ResearchFetchResult::Refused {
                reason: match permission {
                    PermissionDecision::Ask { .. } => {
                        "research requires explicit network permission"
                            .to_owned()
                    }
                    _ => "network policy denies research".to_owned(),
                },
            };
        }
        // 2. Validate the untrusted request.
        let validated = match validate_research_request(request) {
            Ok(validated) => validated,
            Err(reason) => {
                return ResearchFetchResult::Failed {
                    reason: format!("invalid research request: {reason}"),
                };
            }
        };
        // 3. The source must be configured.
        let Some(source) = self.find_source(&validated) else {
            return ResearchFetchResult::Refused {
                reason: format!(
                    "Unknown research source {}:{}; it is not configured.",
                    validated.source.kind.as_str(),
                    validated.source.id
                ),
            };
        };
        let current = (self.current_task)();
        if !is_valid_research_task_binding(current.as_ref()) {
            return ResearchFetchResult::Refused {
                reason:
                    "Research requires an active task with a valid TaskContract revision; no task-bound request was started."
                        .to_owned(),
            };
        }
        let task = {
            let current = current.expect("validated above");
            ResearchTaskBinding {
                task_id: current.task_id,
                task_contract_revision: current.task_contract_revision,
            }
        };
        // Already-aborted calls fail fast without invoking the source.
        if signal.aborted {
            return ResearchFetchResult::Cancelled {
                reason: "Research request cancelled.".to_owned(),
            };
        }
        let sequence = {
            let mut state = self.state.borrow_mut();
            state.request_counter += 1;
            state.active_requests += 1;
            state.request_counter
        };
        let source_request = ResearchRequest {
            source: ResearchSourceRef {
                kind: source.kind(),
                id: source.id().to_owned(),
                label: source.label().to_owned(),
            },
            ..validated
        };
        let request_id =
            create_research_request_id(&source_request, &task, sequence);
        let outcome = source.fetch(&source_request, &self.bounds, signal);
        {
            let mut state = self.state.borrow_mut();
            state.active_requests -= 1;
        }
        if let ResearchOutcome::Document { .. } = &outcome {
            if !same_research_task_binding(
                &task,
                (self.current_task)().as_ref(),
            ) {
                return ResearchFetchResult::Stale {
                    reason:
                        "The active task or TaskContract revision changed while research was in flight; the result was discarded before evidence retention."
                            .to_owned(),
                };
            }
        }
        match outcome {
            ResearchOutcome::Document { document } => {
                let evidence =
                    self.evidence_store.record(&document, &request_id, &task);
                ResearchFetchResult::from_parts(document, evidence)
            }
            ResearchOutcome::Refused { reason } => {
                ResearchFetchResult::Refused { reason }
            }
            ResearchOutcome::UnsupportedContent { reason } => {
                ResearchFetchResult::UnsupportedContent { reason }
            }
            ResearchOutcome::Oversized { reason } => {
                ResearchFetchResult::Oversized { reason }
            }
            ResearchOutcome::Timeout => ResearchFetchResult::Timeout {
                reason: format!(
                    "Research request timed out after {}ms.",
                    self.bounds.timeout_ms
                ),
            },
            ResearchOutcome::Cancelled => ResearchFetchResult::Cancelled {
                reason: "Research request cancelled.".to_owned(),
            },
            ResearchOutcome::Unavailable { reason } => {
                ResearchFetchResult::Unavailable { reason }
            }
            ResearchOutcome::Failed { reason } => {
                ResearchFetchResult::Failed { reason }
            }
        }
    }

    /// Retained evidence views, oldest first (bounded ring).
    #[must_use]
    pub fn latest_evidence(&self) -> Vec<ResearchEvidence> {
        self.evidence_store.snapshots()
    }

    /// In-flight fetch count.
    #[must_use]
    pub fn active_request_count(&self) -> u64 {
        self.state.borrow().active_requests
    }

    /// Configured source kinds, in configuration order (deduplicated).
    #[must_use]
    pub fn source_kinds(&self) -> Vec<ResearchSourceKind> {
        let mut kinds: Vec<ResearchSourceKind> = Vec::new();
        for source in &self.sources {
            if !kinds.contains(&source.kind()) {
                kinds.push(source.kind());
            }
        }
        kinds
    }
}

impl ResearchFetchResult {
    /// from_parts.
    fn from_parts(
        document: Box<ResearchDocument>,
        evidence: ResearchEvidence,
    ) -> Self {
        Self::Document { document, evidence: Box::new(evidence) }
    }
}

/// Pure, bounded model-facing rendering of one research evidence record.
#[must_use]
pub fn format_research_evidence_view(
    evidence: &ResearchEvidence,
    max_bytes: Option<usize>,
) -> String {
    let lines = [
        format!("Source: {}", evidence.source.label),
        format!("Request: {}", evidence.request_id),
        format!(
            "Fetched: {}",
            iso8601_from_epoch_ms(evidence.fetched_at_ms as i64)
        ),
        format!(
            "Revision: {}",
            evidence.resolved_revision.as_deref().unwrap_or("unknown")
        ),
        format!(
            "Version: {}{}",
            evidence.version.as_deref().unwrap_or("unknown"),
            if evidence.fallback { " (fallback)" } else { "" }
        ),
        format!("Excerpt: {}", evidence.excerpt),
        format!("Evidence: {}", evidence.evidence_id),
    ];
    let text = lines.join("\n");
    crate::projection::evidence::truncate_text(
        &text,
        max_bytes.unwrap_or(DEFAULT_RESEARCH_VIEW_MAX_BYTES),
    )
    .0
}

/// Minimal UTC ISO-8601 rendering of epoch milliseconds
/// (`YYYY-MM-DDTHH:MM:SS.mmmZ`), matching `new Date(ms).toISOString()`.
#[must_use]
pub fn iso8601_from_epoch_ms(epoch_ms: i64) -> String {
    let days = epoch_ms.div_euclid(86_400_000);
    let ms_of_day = epoch_ms.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = ms_of_day / 3_600_000;
    let minute = (ms_of_day % 3_600_000) / 60_000;
    let second = (ms_of_day % 60_000) / 1000;
    let millisecond = ms_of_day % 1000;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z"
    )
}

/// Days-from-civil inverse (Howard Hinnant's algorithm).
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}
