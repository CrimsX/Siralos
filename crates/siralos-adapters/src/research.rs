//! Research normalization and deterministic fake sources (Stage 3
//! milestone 5; R13.3).
//!
//! Mirrors `packages/adapters/src/research/{normalization,fake-sources}*`.
//! Pure, bounded conversion of raw fetched text into `ResearchSection`s
//! and full `ResearchDocument`s, shared by every research source adapter.
//! Provider output is untrusted: every normalizer operates on
//! already-bounded input, caps sections/text/headings, and DISCLOSES every
//! truncation. The final document is additionally capped on its
//! serialized size (`maxDocumentBytes`). No network exists here: the fake
//! sources are the deterministic stand-ins used by tests and the behavior
//! harness.

use serde_json::json;
use siralos_core::godot::digest::{canonicalize_json, sha256_hex_str};
use siralos_core::reference::normalize_repository_origin;
use siralos_core::research::{
    CancellationSignal, ResearchBounds, ResearchContentType, ResearchDocument,
    ResearchOutcome, ResearchProvenance, ResearchRequest, ResearchSection,
    ResearchSourceKind, ResearchSourcePort, ResearchSourceRef,
    compute_research_document_content_digest, compute_research_document_id,
};
use std::collections::BTreeMap;

/// TRUNCATION_MARKER.
pub const TRUNCATION_MARKER: &str = "\u{2026} [truncated]";

/// UTF-8-safe truncation: never splits a multi-byte character. Returns
/// the whole text when it already fits within `max_bytes`.
#[must_use]
pub fn byte_slice(text: &str, max_bytes: usize) -> String {
    if max_bytes == 0 {
        return String::new();
    }
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut end = max_bytes;
    while end > 0 && (bytes[end] & 0xc0) == 0x80 {
        end -= 1;
    }
    if end > 0 && end < bytes.len() && (bytes[end] & 0xc0) == 0xc0 {
        end -= 1;
    }
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn byte_length_of(text: &str) -> usize {
    text.len()
}

/// Cap `text` to `max_bytes`, appending the explicit truncation marker
/// when text was actually cut.
#[must_use]
pub fn truncate_with_marker(text: &str, max_bytes: usize) -> String {
    let marker_bytes = byte_length_of(TRUNCATION_MARKER);
    let budget = max_bytes.saturating_sub(marker_bytes);
    let cut = byte_slice(text, budget);
    if cut == text {
        return text.to_owned();
    }
    if budget == 0 {
        return String::new();
    }
    format!("{cut}{TRUNCATION_MARKER}")
}

/// Bounded normalization output shared by the content normalizers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationResult {
    /// Normalized sections.
    pub sections: Vec<ResearchSection>,
    /// Whether truncation occurred.
    pub truncated: bool,
    /// Exact reference-failure reason.
    pub reason: Option<String>,
}

struct SectionBuilderState {
    sections: Vec<ResearchSection>,
    bounds: ResearchBounds,
    truncated: bool,
    reason: Option<String>,
    stopped: bool,
}

const SECTION_LIMIT_REASON: &str =
    "document exceeds the section limit; the last section is truncated";
const SECTION_TEXT_LIMIT_REASON: &str =
    "section text exceeds the byte limit; the section is truncated";
const HEADING_LIMIT_REASON: &str =
    "heading exceeds the byte limit; the heading is truncated";

/// Push one section under the bounds. When the section cap is hit, the
/// LAST section receives the explicit truncation marker and further
/// content is dropped.
fn append_section(
    state: &mut SectionBuilderState,
    heading: Option<String>,
    text: String,
) {
    if state.stopped {
        return;
    }
    if state.sections.len() >= state.bounds.max_sections {
        state.truncated = true;
        state.reason = Some(SECTION_LIMIT_REASON.to_owned());
        if let Some(last) = state.sections.last_mut() {
            if !last.text.is_empty() {
                let marker_bytes = byte_length_of(TRUNCATION_MARKER);
                let room = state
                    .bounds
                    .max_section_text_bytes
                    .saturating_sub(byte_length_of(&last.text));
                if room >= marker_bytes {
                    last.text = format!("{}{TRUNCATION_MARKER}", last.text);
                    last.byte_length = byte_length_of(&last.text);
                }
            }
        }
        state.stopped = true;
        return;
    }
    let mut section_text = text;
    if byte_length_of(&section_text) > state.bounds.max_section_text_bytes {
        section_text = truncate_with_marker(
            &section_text,
            state.bounds.max_section_text_bytes,
        );
        state.truncated = true;
        state.reason = Some(SECTION_TEXT_LIMIT_REASON.to_owned());
    }
    let mut section_heading = heading;
    if let Some(heading_text) = &section_heading {
        if byte_length_of(heading_text) > state.bounds.max_heading_bytes {
            section_heading =
                Some(byte_slice(heading_text, state.bounds.max_heading_bytes));
            state.truncated = true;
            state.reason = Some(HEADING_LIMIT_REASON.to_owned());
        }
    }
    state.sections.push(ResearchSection {
        heading: section_heading,
        byte_length: byte_length_of(&section_text),
        text: section_text,
    });
}

/// ATX heading match for `^(#{1,6})\s+(.*)$`; returns the heading text.
fn atx_heading(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut hashes = 0usize;
    while hashes < bytes.len() && hashes < 6 && bytes[hashes] == b'#' {
        hashes += 1;
    }
    if hashes == 0
        || hashes >= bytes.len()
        || !bytes[hashes].is_ascii_whitespace()
    {
        return None;
    }
    Some(&line[hashes + 1..])
}

fn is_setext_underline(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.is_empty() || (bytes[0] != b'-' && bytes[0] != b'=') {
        return false;
    }
    bytes.iter().all(|b| *b == bytes[0])
}

/// Markdown to sections. Splits on ATX headings (`^#{1,6} `) and setext
/// underline headings (`^---+` / `^===+`; the previous non-empty line
/// becomes the heading). Sections without headings collapse into one
/// leading section.
#[must_use]
pub fn normalize_markdown_to_sections(
    text: &str,
    bounds: ResearchBounds,
) -> NormalizationResult {
    let mut state = SectionBuilderState {
        sections: Vec::new(),
        bounds,
        truncated: false,
        reason: None,
        stopped: false,
    };
    struct Accumulator<'a> {
        state: &'a mut SectionBuilderState,
        current_heading: Option<String>,
        current_lines: Vec<String>,
    }
    impl Accumulator<'_> {
        fn flush(&mut self) {
            let body = self.current_lines.join("\n").trim().to_owned();
            self.current_lines.clear();
            if body.is_empty() && self.current_heading.is_none() {
                return;
            }
            append_section(self.state, self.current_heading.clone(), body);
            self.current_heading = None;
        }
    }
    let mut accumulator = Accumulator {
        state: &mut state,
        current_heading: None,
        current_lines: Vec::new(),
    };
    for raw_line in text.split('\n') {
        if accumulator.state.stopped {
            break;
        }
        let line = raw_line.trim_end();
        if let Some(heading) = atx_heading(line) {
            accumulator.flush();
            let heading = heading.trim().to_owned();
            accumulator.current_heading =
                if heading.is_empty() { None } else { Some(heading) };
            continue;
        }
        if is_setext_underline(line) {
            let lines = &accumulator.current_lines;
            let mut index = lines.len() as isize - 1;
            while index >= 0 && lines[index as usize].trim().is_empty() {
                index -= 1;
            }
            if index >= 0 {
                let previous = lines[index as usize].trim().to_owned();
                if !previous.is_empty() {
                    accumulator.current_lines.truncate(index as usize);
                    if accumulator.current_heading.is_some() {
                        accumulator.flush();
                        continue;
                    }
                    accumulator.current_heading = Some(previous);
                    continue;
                }
            }
        }
        accumulator.current_lines.push(line.to_owned());
    }
    accumulator.flush();
    NormalizationResult {
        sections: state.sections,
        truncated: state.truncated,
        reason: state.reason,
    }
}

const JSON_EXCERPT_LIMIT_REASON: &str =
    "the JSON excerpt exceeds the byte limit; it is truncated";
const TEXT_EXCERPT_LIMIT_REASON: &str =
    "the text exceeds the byte limit; it is truncated";

fn single_section(
    text: String,
    truncated: bool,
    reason: Option<String>,
) -> NormalizationResult {
    NormalizationResult {
        sections: vec![ResearchSection {
            heading: None,
            byte_length: byte_length_of(&text),
            text,
        }],
        truncated,
        reason,
    }
}

/// JSON to one section. Narrow field extraction: a top-level object with
/// a non-empty string `body` (or `description`) uses that field;
/// otherwise the value renders as bounded pretty-printed JSON. Invalid
/// JSON falls back to the raw text.
#[must_use]
pub fn normalize_json_to_sections(
    json_text: &str,
    bounds: ResearchBounds,
) -> NormalizationResult {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(json_text);
    let text = match parsed {
        Ok(value @ serde_json::Value::Object(_)) => {
            let record = value.as_object().expect("checked above");
            let body = record.get("body").and_then(serde_json::Value::as_str);
            let description =
                record.get("description").and_then(serde_json::Value::as_str);
            if body.is_some_and(|body| !body.trim().is_empty()) {
                body.expect("checked above").to_owned()
            } else if description
                .is_some_and(|description| !description.trim().is_empty())
            {
                description.expect("checked above").to_owned()
            } else {
                serde_json::to_string_pretty(&value).unwrap_or_default()
            }
        }
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_default(),
        Err(_) => json_text.to_owned(),
    };
    if byte_length_of(&text) > bounds.max_section_text_bytes {
        return single_section(
            truncate_with_marker(&text, bounds.max_section_text_bytes),
            true,
            Some(JSON_EXCERPT_LIMIT_REASON.to_owned()),
        );
    }
    single_section(text, false, None)
}

/// text/plain to one untitled section, capped by `maxSectionTextBytes`.
#[must_use]
pub fn normalize_plain_to_sections(
    text: &str,
    bounds: ResearchBounds,
) -> NormalizationResult {
    if byte_length_of(text) > bounds.max_section_text_bytes {
        return single_section(
            truncate_with_marker(text, bounds.max_section_text_bytes),
            true,
            Some(TEXT_EXCERPT_LIMIT_REASON.to_owned()),
        );
    }
    single_section(text.to_owned(), false, None)
}

/// Content-type allowlist with parameter prefix matches
/// (`text/html; charset=utf-8` -> `text/html`). Anything else returns
/// `None` so the caller fails closed with `unsupported-content`.
#[must_use]
pub fn classify_content_type(
    raw: Option<&str>,
) -> Option<ResearchContentType> {
    let base = raw?.split(';').next().unwrap_or("").trim().to_lowercase();
    ResearchContentType::parse(&base)
}

const DOCUMENT_FIXED_OVERHEAD_BYTES: usize = 128;
const PER_SECTION_OVERHEAD_BYTES: usize = 64;
const PER_LINK_OVERHEAD_BYTES: usize = 32;

fn compute_byte_length(
    sections: &[ResearchSection],
    link_count: usize,
) -> usize {
    let mut total = DOCUMENT_FIXED_OVERHEAD_BYTES;
    for section in sections {
        total += section.byte_length
            + section.heading.as_deref().map_or(0, byte_length_of)
            + PER_SECTION_OVERHEAD_BYTES;
    }
    total + link_count * PER_LINK_OVERHEAD_BYTES
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization cannot fail")
}

fn json_option(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_owned())
}

/// Serialized content bytes of a document, excluding derived identity
/// fields, in the TypeScript reference's exact insertion order (counted
/// in UTF-16 code units, matching `JSON.stringify(content).length`).
fn measured_bytes(document: &ResearchDocument) -> usize {
    let mut s = String::new();
    s.push_str("{\"id\":");
    s.push_str(&json_string(&document.id));
    s.push_str(",\"source\":{\"kind\":");
    s.push_str(&json_string(document.source.kind.as_str()));
    s.push_str(",\"id\":");
    s.push_str(&json_string(&document.source.id));
    s.push_str(",\"label\":");
    s.push_str(&json_string(&document.source.label));
    s.push_str("},\"title\":");
    s.push_str(&json_option(document.title.as_deref()));
    s.push_str(",\"fetchedAtMs\":");
    s.push_str(&document.fetched_at_ms.to_string());
    s.push_str(",\"contentType\":");
    s.push_str(&json_string(document.content_type.as_str()));
    s.push_str(",\"sections\":[");
    for (index, section) in document.sections.iter().enumerate() {
        if index > 0 {
            s.push(',');
        }
        s.push_str("{\"heading\":");
        s.push_str(&json_option(section.heading.as_deref()));
        s.push_str(",\"text\":");
        s.push_str(&json_string(&section.text));
        s.push_str(",\"byteLength\":");
        s.push_str(&section.byte_length.to_string());
        s.push('}');
    }
    s.push_str("],\"links\":[],\"provenance\":{\"source\":{\"kind\":");
    s.push_str(&json_string(document.provenance.source.kind.as_str()));
    s.push_str(",\"id\":");
    s.push_str(&json_string(&document.provenance.source.id));
    s.push_str(",\"label\":");
    s.push_str(&json_string(&document.provenance.source.label));
    s.push_str("},\"requestedRef\":");
    s.push_str(&json_option(document.provenance.requested_ref.as_deref()));
    s.push_str(",\"resolvedRevision\":");
    s.push_str(&json_option(document.provenance.resolved_revision.as_deref()));
    s.push_str(",\"requestedVersion\":");
    s.push_str(&json_option(document.provenance.requested_version.as_deref()));
    s.push_str(",\"usedVersion\":");
    s.push_str(&json_option(document.provenance.used_version.as_deref()));
    s.push_str(",\"fallback\":");
    s.push_str(if document.provenance.fallback { "true" } else { "false" });
    s.push_str(",\"fallbackReason\":");
    s.push_str(&json_option(document.provenance.fallback_reason.as_deref()));
    s.push_str(",\"fetchedAtMs\":");
    s.push_str(&document.provenance.fetched_at_ms.to_string());
    s.push_str(",\"resource\":");
    s.push_str(&json_string(&document.provenance.resource));
    s.push_str("},\"truncated\":");
    s.push_str(if document.truncated { "true" } else { "false" });
    s.push_str(",\"truncationReason\":");
    s.push_str(&json_option(document.truncation_reason.as_deref()));
    s.push_str(",\"byteLength\":");
    s.push_str(&document.byte_length.to_string());
    s.push('}');
    s.encode_utf16().count()
}

/// Options for [`build_research_document`].
pub struct BuildResearchDocumentOptions<'a> {
    /// Configured source reference.
    pub source: &'a ResearchSourceRef,
    /// Document title.
    pub title: Option<&'a str>,
    /// Classified content type.
    pub content_type: ResearchContentType,
    /// raw_text.
    pub raw_text: &'a str,
    /// Fetch provenance record.
    pub provenance: ResearchProvenance,
    /// Normalized bounds.
    pub bounds: ResearchBounds,
    /// Clock value used as the document's `fetchedAtMs`.
    pub now: u64,
}

const DOC_SECTIONS_DROPPED_REASON: &str =
    "the document exceeds the byte limit; trailing sections were dropped";
const DOC_SECTION_TRIMMED_REASON: &str =
    "the document exceeds the byte limit; the section text was truncated";

/// Build a full `ResearchDocument` from raw fetched text: applies the
/// normalizer for the content type and enforces the final
/// `maxDocumentBytes` cap on the serialized document (drop trailing
/// sections, then trim the first section's text, with explicit
/// truncation disclosure). The content digest is recomputed over the
/// exact FINAL normalized content.
#[must_use]
pub fn build_research_document(
    options: BuildResearchDocumentOptions<'_>,
) -> ResearchDocument {
    let BuildResearchDocumentOptions {
        source,
        title,
        content_type,
        raw_text,
        provenance,
        bounds,
        now,
    } = options;
    // The HTML normalizer is not part of the R13.3 frozen differential
    // surface; HTML-classified fixtures are only served through the
    // markdown-shaped fixtures here. Classification still happens at the
    // callers, so unsupported types never reach this builder in scenarios.
    let normalized = match content_type {
        ResearchContentType::TextMarkdown | ResearchContentType::TextHtml => {
            normalize_markdown_to_sections(raw_text, bounds)
        }
        ResearchContentType::ApplicationJson => {
            normalize_json_to_sections(raw_text, bounds)
        }
        ResearchContentType::TextPlain => {
            normalize_plain_to_sections(raw_text, bounds)
        }
    };
    let sections = normalized.sections;
    let links: Vec<(String, Option<String>)> = Vec::new();

    let request_digest = sha256_hex_str(&canonicalize_json(&json!({
        "resource": provenance.resource,
        "requestedRef": provenance.requested_ref,
    })));
    let id = compute_research_document_id(&source.id, &request_digest);

    let content_digest = compute_research_document_content_digest(
        title,
        content_type,
        &sections,
    )
    .unwrap_or_default();

    let mut document = ResearchDocument {
        id,
        source: source.clone(),
        title: title.map(str::to_owned),
        fetched_at_ms: now,
        content_type,
        sections,
        links,
        provenance,
        truncated: normalized.truncated,
        truncation_reason: normalized.reason,
        byte_length: 0,
        content_digest,
        raw_artifact_digest: Some(sha256_hex_str(raw_text)),
    };
    document.byte_length =
        compute_byte_length(&document.sections, document.links.len());

    if measured_bytes(&document) > bounds.max_document_bytes {
        let mut dropped = 0;
        while document.sections.len() > 1
            && measured_bytes(&document) > bounds.max_document_bytes
        {
            document.sections.pop();
            dropped += 1;
        }
        document.byte_length =
            compute_byte_length(&document.sections, document.links.len());
        if dropped > 0 {
            document.truncated = true;
            document.truncation_reason =
                Some(DOC_SECTIONS_DROPPED_REASON.to_owned());
        }
        if measured_bytes(&document) > bounds.max_document_bytes {
            if let Some(first) = document.sections.first().cloned() {
                if !first.text.is_empty() {
                    let candidate_bytes = |text: &str| -> usize {
                        let sections = vec![ResearchSection {
                            heading: first.heading.clone(),
                            byte_length: byte_length_of(text),
                            text: text.to_owned(),
                        }];
                        let byte_length = compute_byte_length(
                            &sections,
                            document.links.len(),
                        );
                        let probe = ResearchDocument {
                            sections,
                            truncated: true,
                            truncation_reason: Some(
                                DOC_SECTION_TRIMMED_REASON.to_owned(),
                            ),
                            byte_length,
                            ..document.clone()
                        };
                        measured_bytes(&probe)
                    };
                    let mut text = first.text.clone();
                    let mut guard = 0;
                    while candidate_bytes(&text) > bounds.max_document_bytes
                        && !text.is_empty()
                        && guard < 4096
                    {
                        text =
                            byte_slice(&text, text.len().saturating_sub(64));
                        guard += 1;
                    }
                    let marked = format!("{text}{TRUNCATION_MARKER}");
                    let final_text = if candidate_bytes(&marked)
                        <= bounds.max_document_bytes
                    {
                        marked
                    } else {
                        text
                    };
                    document.sections = vec![ResearchSection {
                        heading: first.heading,
                        byte_length: byte_length_of(&final_text),
                        text: final_text,
                    }];
                    document.byte_length = compute_byte_length(
                        &document.sections,
                        document.links.len(),
                    );
                    document.truncated = true;
                    document.truncation_reason =
                        Some(DOC_SECTION_TRIMMED_REASON.to_owned());
                }
            }
        }
    }

    // Content identity (ADR 0028): recompute the digest over the exact
    // FINAL normalized content.
    document.content_digest = compute_research_document_content_digest(
        document.title.as_deref(),
        document.content_type,
        &document.sections,
    )
    .unwrap_or_default();
    document
}

// ---------------------------------------------------------------------------
// Fake sources (deterministic, network-free).
// ---------------------------------------------------------------------------

/// One fixture page of the fake godot-docs source.
#[derive(Debug, Clone)]
pub struct GodotDocsPageFixture {
    /// Document title.
    pub title: String,
    /// (heading, text) pairs rendered as ATX-markdown.
    pub sections: Vec<(Option<String>, String)>,
}

/// Explicit fallback chain entry per requested version.
#[derive(Debug, Clone)]
pub struct GodotDocsFallback {
    /// used_version.
    pub used_version: String,
    /// Exact reference-failure reason.
    pub reason: String,
}

/// Fixture set for the fake godot-docs source: version -> topic -> page.
#[derive(Default)]
pub struct GodotDocsFixture {
    /// versions.
    pub versions: BTreeMap<String, BTreeMap<String, GodotDocsPageFixture>>,
    /// fallbacks.
    pub fallbacks: BTreeMap<String, GodotDocsFallback>,
}

fn render_page_markdown(page: &GodotDocsPageFixture) -> String {
    page.sections
        .iter()
        .map(|(heading, text)| match heading {
            None => text.clone(),
            Some(heading) => format!("# {heading}\n\n{text}"),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Fake godot-docs source: serves version-matched topics from fixtures;
/// unknown versions fall back through the fixture's explicit chain with
/// `fallback: true` + reason. Unknown topics fail closed ("not found").
pub struct FakeGodotDocsSource {
    /// fixture.
    pub fixture: GodotDocsFixture,
    /// Injected fixed clock for provenance timestamps.
    pub now_ms: u64,
}

impl ResearchSourcePort for FakeGodotDocsSource {
    fn kind(&self) -> ResearchSourceKind {
        ResearchSourceKind::GodotDocs
    }

    /// Stable identifier.
    fn id(&self) -> &str {
        "godot-docs-fake"
    }

    /// Human/model-facing label.
    fn label(&self) -> &str {
        "Fake Godot docs"
    }

    /// fetch.
    fn fetch(
        &self,
        request: &ResearchRequest,
        bounds: &ResearchBounds,
        signal: &CancellationSignal,
    ) -> ResearchOutcome {
        if signal.aborted {
            return ResearchOutcome::Cancelled;
        }
        let requested_version =
            request.version.clone().unwrap_or_else(|| "stable".to_owned());
        let topic = request.topic.as_deref().unwrap_or("").trim().to_owned();
        if topic.is_empty() {
            return ResearchOutcome::Failed { reason: "not found".to_owned() };
        }
        if let Some(page) = self
            .fixture
            .versions
            .get(&requested_version)
            .and_then(|topics| topics.get(&topic))
        {
            return self.serve_page(
                request,
                bounds,
                page,
                &requested_version,
                false,
                None,
            );
        }
        if let Some(fallback) = self.fixture.fallbacks.get(&requested_version)
        {
            if let Some(page) = self
                .fixture
                .versions
                .get(&fallback.used_version)
                .and_then(|topics| topics.get(&topic))
            {
                return self.serve_page(
                    request,
                    bounds,
                    page,
                    &fallback.used_version,
                    true,
                    Some(&fallback.reason),
                );
            }
        }
        ResearchOutcome::Failed { reason: "not found".to_owned() }
    }
}

impl FakeGodotDocsSource {
    /// serve_page.
    fn serve_page(
        &self,
        request: &ResearchRequest,
        bounds: &ResearchBounds,
        page: &GodotDocsPageFixture,
        used_version: &str,
        fallback: bool,
        fallback_reason: Option<&str>,
    ) -> ResearchOutcome {
        let raw_text = render_page_markdown(page);
        let resource = format!(
            "docs:{used_version}:{}",
            request.topic.as_deref().unwrap_or("").trim()
        );
        let provenance = ResearchProvenance {
            source: request.source.clone(),
            requested_ref: None,
            resolved_revision: None,
            requested_version: request.version.clone(),
            used_version: Some(used_version.to_owned()),
            fallback,
            fallback_reason: fallback_reason.map(str::to_owned),
            fetched_at_ms: self.now_ms,
            resource,
        };
        let document = build_research_document(BuildResearchDocumentOptions {
            source: &request.source,
            title: Some(&page.title),
            content_type: ResearchContentType::TextMarkdown,
            raw_text: &raw_text,
            provenance,
            bounds: *bounds,
            now: self.now_ms,
        });
        ResearchOutcome::Document { document: Box::new(document) }
    }
}

/// One fixture file of the fake repository research source.
#[derive(Debug, Clone)]
pub struct FakeRepositoryFileFixture {
    /// Classified content type.
    pub content_type: String,
    /// body.
    pub body: String,
}

/// Fixture set keyed by canonical `owner/repo`: ref -> path -> file.
#[derive(Default)]
pub struct FakeRepositoryResearchFixture {
    /// repos.
    pub repos: BTreeMap<
        String,
        BTreeMap<String, BTreeMap<String, FakeRepositoryFileFixture>>,
    >,
}

/// Validate an untrusted repository resource path (exact reference
/// reasons).
pub fn validate_research_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("a repository research request requires a non-empty path"
            .to_owned());
    }
    if path.starts_with('/') || path.contains('\\') || path.contains('\0') {
        return Err("the resource path must be relative with forward slashes"
            .to_owned());
    }
    if path.split('/').any(|segment| segment == ".." || segment == ".") {
        return Err(
            "the resource path must not contain \"..\" or \".\" segments"
                .to_owned(),
        );
    }
    Ok(())
}

fn is_full_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Fake GitHub repository research source: serves fixture file content at
/// `path`+`ref`, with provenance mirroring the real GitHub source
/// (40-hex commit refs carry `resolvedRevision`; branches/tags leave it
/// null). Unclassifiable fixture content types fail closed.
pub struct FakeRepositorySource {
    /// fixture.
    pub fixture: FakeRepositoryResearchFixture,
    /// Injected fixed clock for provenance timestamps.
    pub now_ms: u64,
}

impl ResearchSourcePort for FakeRepositorySource {
    fn kind(&self) -> ResearchSourceKind {
        ResearchSourceKind::Repository
    }

    /// Stable identifier.
    fn id(&self) -> &str {
        "github-fake"
    }

    /// Human/model-facing label.
    fn label(&self) -> &str {
        "Fake GitHub repository research"
    }

    /// fetch.
    fn fetch(
        &self,
        request: &ResearchRequest,
        bounds: &ResearchBounds,
        signal: &CancellationSignal,
    ) -> ResearchOutcome {
        if signal.aborted {
            return ResearchOutcome::Cancelled;
        }
        let origin = match normalize_repository_origin(&request.query) {
            Ok(origin) => origin,
            Err(reason) => {
                return ResearchOutcome::Refused {
                    reason: format!("invalid repository origin: {reason}"),
                };
            }
        };
        let repo_key = origin
            .strip_prefix("https://github.com/")
            .unwrap_or(&origin)
            .to_owned();
        let Some(repo) = self.fixture.repos.get(&repo_key) else {
            return ResearchOutcome::Failed {
                reason: format!(
                    "repository \"{repo_key}\" is not in the fixture set"
                ),
            };
        };
        let Some(path) = &request.path else {
            return ResearchOutcome::Refused {
                reason:
                    "a repository research request requires a path (or a release topic)"
                        .to_owned(),
            };
        };
        if let Err(reason) = validate_research_path(path) {
            return ResearchOutcome::Refused { reason };
        }
        let r#ref = request.r#ref.clone().unwrap_or_else(|| "HEAD".to_owned());
        let Some(file) = repo.get(&r#ref).and_then(|paths| paths.get(path))
        else {
            return ResearchOutcome::Failed {
                reason: "resource not found".to_owned(),
            };
        };
        let Some(content_type) =
            classify_content_type(Some(&file.content_type))
        else {
            return ResearchOutcome::UnsupportedContent {
                reason: format!(
                    "unsupported content type {}",
                    file.content_type
                ),
            };
        };
        let resolved_revision = if is_full_commit_sha(&r#ref) {
            Some(r#ref.clone())
        } else {
            None
        };
        let ref_name = r#ref.as_str();
        let provenance = ResearchProvenance {
            source: request.source.clone(),
            requested_ref: request.r#ref.clone(),
            resolved_revision,
            requested_version: None,
            used_version: None,
            fallback: false,
            fallback_reason: None,
            fetched_at_ms: self.now_ms,
            resource: format!("files:{ref_name}:{path}"),
        };
        let document = build_research_document(BuildResearchDocumentOptions {
            source: &request.source,
            title: None,
            content_type,
            raw_text: &file.body,
            provenance,
            bounds: *bounds,
            now: self.now_ms,
        });
        ResearchOutcome::Document { document: Box::new(document) }
    }
}
