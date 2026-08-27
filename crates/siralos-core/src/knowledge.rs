//! Structured project knowledge core (Stage 3R R13.2).
//!
//! Mirrors the TypeScript knowledge model and coordinator's deterministic
//! core: validated proposals, no-churn subject evolution, structural
//! rejection of policy-shaped claims, injected-secret protection,
//! provenance gating over host-verified ports, deterministic scored
//! retrieval with traces, bounded pinning, retirement, and state
//! revisions. Facts are untrusted factual context and never grant
//! capability.

use std::collections::BTreeMap;

use crate::identity::{CanonicalValue, compute_artifact_digest_hex};
use crate::identity::{canonicalize_json, sha256_hex_str};
use serde_json::{Value, json};

/// State-version tag for the knowledge snapshot digest.
pub const KNOWLEDGE_STATE_VERSION: &str = "knowledge-1";

/// Reference limits for the exercised surface.
pub struct KnowledgeLimits {
    /// The max facts bound.
    pub max_facts: usize,
    /// The max revisions per subject bound.
    pub max_revisions_per_subject: usize,
    /// The max content bytes bound.
    pub max_content_bytes: usize,
    /// The max pinned facts bound.
    pub max_pinned_facts: usize,
    /// The max pinned bytes bound.
    pub max_pinned_bytes: usize,
    /// The max retrieval facts bound.
    pub max_retrieval_facts: usize,
    /// The max retrieval bytes bound.
    pub max_retrieval_bytes: usize,
}

impl Default for KnowledgeLimits {
    fn default() -> Self {
        Self {
            max_facts: 256,
            max_revisions_per_subject: 64,
            max_content_bytes: 4 * 1024,
            max_pinned_facts: 6,
            max_pinned_bytes: 1200,
            max_retrieval_facts: 8,
            max_retrieval_bytes: 6000,
        }
    }
}

/// Injected determinism/provenance ports. Absent research verification is
/// distinguishable from a rejecting verifier.
#[derive(Default, Clone)]
pub struct KnowledgePorts {
    /// The injected fixed clock (milliseconds).
    pub now_ms: u64,
    /// The secrets bound.
    pub secrets: Vec<String>,
    /// The file states bound.
    pub file_states: Vec<(String, String)>,
    /// The research evidence ids bound.
    pub research_evidence_ids: Option<Vec<String>>,
}

fn is_valid_subject_key(subject_key: &str) -> bool {
    if subject_key.is_empty() || subject_key.len() > 128 {
        return false;
    }
    let mut chars = subject_key.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|current| {
        current.is_ascii_lowercase()
            || current.is_ascii_digit()
            || matches!(current, '.' | '_' | '-')
    })
}

/// Deterministic fact id (`kf_` prefix, 24 hex digits).
pub fn compute_fact_id(
    subject_key: Option<&str>,
    content: &str,
    revision: u64,
) -> String {
    let digest = sha256_hex_str(&canonicalize_json(&json!({
        "content": content,
        "revision": revision,
        "scope": "project",
        "subjectKey": subject_key,
    })));
    format!("kf_{}", &digest[..24])
}

/// Whitespace-churn-insensitive comparison form.
pub fn normalize_fact_content(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());
    let mut previous_was_space = false;
    for character in content.replace("\r\n", "\n").chars() {
        let is_space = character.is_whitespace();
        if is_space {
            if !previous_was_space {
                normalized.push(' ');
            }
        } else {
            normalized.push(character);
        }
        previous_was_space = is_space;
    }
    normalized.trim().to_string()
}

/// Canonical ADR-0028 content digest of one fact revision (content only).
pub fn compute_knowledge_fact_content_digest(content: &str) -> String {
    let payload = CanonicalValue::Object(
        [("content".to_string(), CanonicalValue::Str(content.to_string()))]
            .into_iter()
            .collect(),
    );
    compute_artifact_digest_hex("KnowledgeFact", 1, &payload)
        .expect("KnowledgeFact payload is canonical")
}

/// Deterministic knowledge-state digest over active facts.
pub fn compute_knowledge_state_revision(facts: &[Value]) -> String {
    let mut sorted: Vec<&Value> = facts.iter().collect();
    sorted
        .sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let entries: Vec<Value> = sorted
        .iter()
        .map(|fact| {
            json!({
                "id": fact["id"],
                "scope": fact["scope"],
                "subjectKey": fact["subjectKey"],
                "revision": fact["revision"],
                "content": fact["content"],
                "status": "active",
            })
        })
        .collect();
    sha256_hex_str(&canonicalize_json(&Value::Array(entries)))
}

// ---------------------------------------------------------------------------
// Retrieval scoring (reference-frozen constants).
// ---------------------------------------------------------------------------

const STOPWORDS: [&str; 27] = [
    "the", "a", "an", "is", "are", "was", "were", "to", "of", "for", "and",
    "or", "in", "on", "with", "at", "by", "it", "its", "this", "that",
    "project", "godot", "file", "files", "use", "uses",
];

/// Lowercase keyword tokens with stopwords and single characters removed.
pub fn tokenize_fact_text(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens.retain(|token| {
        token.len() > 1 && !STOPWORDS.contains(&token.as_str())
    });
    tokens.sort();
    tokens.dedup();
    tokens
}

const SUBJECT_EXACT: i64 = 100;
const SUBJECT_PREFIX: i64 = 40;
const KEYWORD_PER_OVERLAP: i64 = 4;
const KEYWORD_CAP: i64 = 40;
const PATH_RELEVANCE: i64 = 20;

fn confidence_weight(confidence: &str) -> i64 {
    match confidence {
        "high" => 15,
        "medium" => 8,
        _ => 2,
    }
}

fn freshness_weight(volatility: &str) -> f64 {
    match volatility {
        "volatile" => 12.0,
        "stable" => 6.0,
        "evergreen" => 4.0,
        _ => 10.0,
    }
}

fn freshness_window_days(volatility: &str) -> f64 {
    match volatility {
        "volatile" => 30.0,
        "stable" => 730.0,
        "evergreen" => 0.0,
        _ => 180.0,
    }
}

// ---------------------------------------------------------------------------
// Conservative structural rejection of policy-shaped knowledge.
// ---------------------------------------------------------------------------

fn word_starts_before(bytes: &[u8], index: usize) -> bool {
    index == 0
        || !(bytes[index - 1].is_ascii_alphanumeric()
            || bytes[index - 1] == b'_')
}

fn word_ends_at(bytes: &[u8], end: usize) -> bool {
    match bytes.get(end) {
        None => true,
        Some(byte) => !(*byte).is_ascii_alphanumeric() && *byte != b'_',
    }
}

fn find_word(text: &[u8], word: &[u8]) -> Vec<usize> {
    let mut positions = Vec::new();
    if text.len() < word.len() {
        return positions;
    }
    for start in 0..=(text.len() - word.len()) {
        if text[start..start + word.len()].eq_ignore_ascii_case(word)
            && word_starts_before(text, start)
            && word_ends_at(text, start + word.len())
        {
            positions.push(start);
        }
    }
    positions
}

/// True when `text[first_end..second_start]` contains no period or
/// newline and its length stays within the gap bound.
fn gap_ok(
    text: &[u8],
    first_end: usize,
    second_start: usize,
    max_gap: usize,
) -> bool {
    second_start >= first_end
        && second_start - first_end <= max_gap
        && text[first_end..second_start]
            .iter()
            .all(|byte| *byte != b'.' && *byte != b'\n')
}

fn any_ordered_pair_within_gap(
    text: &[u8],
    firsts: &[&[u8]],
    seconds: &[&[u8]],
    max_gap: usize,
) -> bool {
    for first in firsts {
        for first_start in find_word(text, first) {
            let first_end = first_start + first.len();
            for second in seconds {
                for second_start in find_word(text, second) {
                    if second_start >= first_end
                        && gap_ok(text, first_end, second_start, max_gap)
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Two-token phrase whose tokens are separated by whitespace only.
fn phrase_positions(
    text: &[u8],
    first: &[u8],
    second: &[u8],
) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for first_start in find_word(text, first) {
        let first_end = first_start + first.len();
        let mut cursor = first_end;
        while cursor < text.len() && text[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == first_end {
            continue;
        }
        for second_start in find_word(text, second) {
            if second_start == cursor {
                spans.push((first_start, second_start + second.len()));
            }
        }
    }
    spans
}

/// Structural rejection of policy-shaped knowledge. Not an AI safety
/// classifier: only clear permission/capability/sandbox claims shaped
/// like instructions are rejected, and knowledge never grants capability
/// regardless.
pub fn reject_policy_shaped_content(content: &str) -> Option<&'static str> {
    const REASON: &str = "Knowledge that claims permissions, capability grants, or sandbox/approval policy is not accepted; knowledge is factual context and can never grant capability.";
    let lower: Vec<u8> =
        normalize_fact_content(&content.to_lowercase()).into_bytes();

    let allow_verbs: [&[u8]; 5] = [
        b"allow".as_slice(),
        b"enable".as_slice(),
        b"grant".as_slice(),
        b"permit".as_slice(),
        b"approve".as_slice(),
    ];
    let effect_nouns: [&[u8]; 12] = [
        b"shell".as_slice(),
        b"network".as_slice(),
        b"write".as_slice(),
        b"execute".as_slice(),
        b"exec".as_slice(),
        b"access".as_slice(),
        b"sandbox".as_slice(),
        b"approval".as_slice(),
        b"mutation".as_slice(),
        b"command".as_slice(),
        b"commands".as_slice(),
        b"internet".as_slice(),
    ];
    let access_nouns: [&[u8]; 9] = [
        b"access".as_slice(),
        b"shell".as_slice(),
        b"network".as_slice(),
        b"write".as_slice(),
        b"execute".as_slice(),
        b"command".as_slice(),
        b"commands".as_slice(),
        b"script".as_slice(),
        b"scripts".as_slice(),
    ];
    let state_verbs: [&[u8]; 2] = [b"is".as_slice(), b"are".as_slice()];
    let protection_nouns: [&[u8]; 7] = [
        b"sandbox".as_slice(),
        b"approval".as_slice(),
        b"checkpoint".as_slice(),
        b"security".as_slice(),
        b"policy".as_slice(),
        b"restriction".as_slice(),
        b"limit".as_slice(),
    ];
    let authority_nouns: [&[u8]; 5] = [
        b"policy".as_slice(),
        b"rules".as_slice(),
        b"restrictions".as_slice(),
        b"security".as_slice(),
        b"approval".as_slice(),
    ];
    let authority_tails: [&[u8]; 3] = [
        b"needed".as_slice(),
        b"required".as_slice(),
        b"necessary".as_slice(),
    ];
    let without_firsts: [&[u8]; 2] = [b"without".as_slice(), b"no".as_slice()];
    let execution_actors: [&[u8]; 14] = [
        b"command".as_slice(),
        b"commands".as_slice(),
        b"script".as_slice(),
        b"scripts".as_slice(),
        b"shell".as_slice(),
        b"execution".as_slice(),
        b"mutation".as_slice(),
        b"mutations".as_slice(),
        b"write".as_slice(),
        b"writes".as_slice(),
        b"edit".as_slice(),
        b"edits".as_slice(),
        b"change".as_slice(),
        b"changes".as_slice(),
    ];
    let write_actors: [&[u8]; 4] = [
        b"writes".as_slice(),
        b"edits".as_slice(),
        b"changes".as_slice(),
        b"mutations".as_slice(),
    ];

    // always\s+allow
    for always_start in find_word(&lower, b"always".as_slice()) {
        let mut cursor = always_start + 6;
        while cursor < lower.len() && lower[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor > always_start + 6
            && find_word(&lower, b"allow".as_slice())
                .into_iter()
                .any(|start| start == cursor)
        {
            return Some(REASON);
        }
    }
    // (allow|enable|grant|permit|approve) …{0,60}… effect noun
    if any_ordered_pair_within_gap(&lower, &allow_verbs, &effect_nouns, 60) {
        return Some(REASON);
    }
    // access noun …{0,30}… is/are ws+ participle
    for noun in access_nouns {
        for noun_start in find_word(&lower, noun) {
            let noun_end = noun_start + noun.len();
            let phrases = state_verbs
                .iter()
                .flat_map(|verb| {
                    phrase_positions(&lower, verb, b"allowed".as_slice())
                        .into_iter()
                        .chain(phrase_positions(
                            &lower,
                            verb,
                            b"permitted".as_slice(),
                        ))
                        .chain(phrase_positions(
                            &lower,
                            verb,
                            b"granted".as_slice(),
                        ))
                        .chain(phrase_positions(
                            &lower,
                            verb,
                            b"enabled".as_slice(),
                        ))
                })
                .collect::<Vec<_>>();
            if phrases.iter().any(|(start, _)| {
                *start >= noun_end && gap_ok(&lower, noun_end, *start, 30)
            }) {
                return Some(REASON);
            }
        }
    }
    // (disable|bypass|turn off|turn down) …{0,60}… protection noun
    let disable_spans: Vec<(usize, usize)> =
        find_word(&lower, b"disable".as_slice())
            .into_iter()
            .map(|start| (start, start + 7))
            .chain(
                find_word(&lower, b"bypass".as_slice())
                    .into_iter()
                    .map(|start| (start, start + 6)),
            )
            .chain(phrase_positions(
                &lower,
                b"turn".as_slice(),
                b"off".as_slice(),
            ))
            .chain(phrase_positions(
                &lower,
                b"turn".as_slice(),
                b"down".as_slice(),
            ))
            .collect();
    for (disable_start, disable_end) in disable_spans {
        for protection in protection_nouns {
            for protection_start in find_word(&lower, protection) {
                if protection_start >= disable_end
                    && gap_ok(&lower, disable_end, protection_start, 60)
                {
                    return Some(REASON);
                }
            }
        }
        let _ = disable_start;
    }
    // ignore …{0,40}… authority noun
    if any_ordered_pair_within_gap(
        &lower,
        &[b"ignore".as_slice()],
        &authority_nouns,
        40,
    ) {
        return Some(REASON);
    }
    // no ws+ authority-noun …{0,40}… needed/required/necessary
    let no_phrases: Vec<(usize, usize)> = [
        b"approval".as_slice(),
        b"permission".as_slice(),
        b"checkpoint".as_slice(),
        b"review".as_slice(),
    ]
    .iter()
    .flat_map(|noun| phrase_positions(&lower, b"no".as_slice(), noun))
    .collect();
    for (no_start, no_end) in no_phrases {
        for tail in authority_tails {
            for tail_start in find_word(&lower, tail) {
                if tail_start >= no_end
                    && gap_ok(&lower, no_end, tail_start, 40)
                {
                    return Some(REASON);
                }
            }
        }
        let _ = no_start;
    }
    // execution/write actors …{0,40}… without|no ws+ authority-noun
    let without_phrases: Vec<(usize, usize)> = without_firsts
        .iter()
        .flat_map(|first| {
            [
                b"approval".as_slice(),
                b"permission".as_slice(),
                b"checkpoint".as_slice(),
                b"review".as_slice(),
            ]
            .iter()
            .flat_map(|second| phrase_positions(&lower, first, second))
            .collect::<Vec<_>>()
        })
        .collect();
    for actor in execution_actors.iter().chain(write_actors.iter()) {
        for actor_start in find_word(&lower, actor) {
            let actor_end = actor_start + actor.len();
            if without_phrases.iter().any(|(start, _)| {
                *start >= actor_end && gap_ok(&lower, actor_end, *start, 40)
            }) {
                return Some(REASON);
            }
        }
    }
    // is/are ws+ granted …{0,40}… without/no ws+ authority-noun
    for (verb, participle) in [
        (b"is".as_slice(), b"allowed".as_slice()),
        (b"is".as_slice(), b"permitted".as_slice()),
        (b"is".as_slice(), b"granted".as_slice()),
        (b"are".as_slice(), b"allowed".as_slice()),
        (b"are".as_slice(), b"permitted".as_slice()),
        (b"are".as_slice(), b"granted".as_slice()),
    ] {
        for (_phrase_start, phrase_end) in
            phrase_positions(&lower, verb, participle)
        {
            for actor in write_actors {
                for actor_start in find_word(&lower, actor) {
                    let _ = actor_start;
                }
            }
            let without_hits: Vec<usize> = without_firsts
                .iter()
                .flat_map(|first| {
                    [
                        b"approval".as_slice(),
                        b"permission".as_slice(),
                        b"checkpoint".as_slice(),
                        b"review".as_slice(),
                    ]
                    .iter()
                    .filter_map(|second| {
                        phrase_positions(&lower, first, second)
                            .into_iter()
                            .map(|(start, _)| start)
                            .find(|start| *start >= phrase_end)
                    })
                    .collect::<Vec<_>>()
                })
                .collect();
            if without_hits
                .into_iter()
                .any(|start| gap_ok(&lower, phrase_end, start, 40))
            {
                return Some(REASON);
            }
        }
    }
    // Start-anchored: unrestricted/full ws+ network/shell/write/access
    // within the first 40 characters and before any period/newline.
    for qualifier in [b"unrestricted".as_slice(), b"full".as_slice()] {
        for qualifier_start in find_word(&lower, qualifier) {
            if qualifier_start > 40
                || lower[..qualifier_start]
                    .iter()
                    .any(|byte| *byte == b'.' || *byte == b'\n')
            {
                continue;
            }
            for object in [
                b"network".as_slice(),
                b"shell".as_slice(),
                b"write".as_slice(),
                b"access".as_slice(),
            ] {
                if phrase_positions(&lower, qualifier, object)
                    .into_iter()
                    .any(|(start, _)| start == qualifier_start)
                {
                    return Some(REASON);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Coordinator over injected clock/provenance ports.
// ---------------------------------------------------------------------------

/// One stored knowledge fact (JSON-shaped for exact protocol parity).
#[derive(Debug, Clone)]
pub struct StoredFact {
    /// The value bound.
    pub value: Value,
    /// The bytes key bound.
    pub bytes_key: String,
}

fn utf8_len(text: &str) -> usize {
    text.chars().map(|character| character.len_utf8()).sum()
}

fn fact_projection_bytes(fact: &Value) -> usize {
    let prefix = fact["subjectKey"]
        .as_str()
        .unwrap_or_else(|| fact["id"].as_str().expect("id"));
    utf8_len(&format!(
        "{prefix} {}",
        fact["content"].as_str().unwrap_or_default()
    ))
}

fn is_expired(fact: &Value, at_ms: u64) -> bool {
    match fact["expiresAtMs"].as_u64() {
        Some(expires_at) => expires_at <= at_ms,
        None => false,
    }
}

/// The deterministic knowledge coordinator.
pub struct KnowledgeCoordinator {
    ports: KnowledgePorts,
    limits: KnowledgeLimits,
    by_subject: BTreeMap<String, Vec<Value>>,
    current: BTreeMap<String, usize>,
    one_off: BTreeMap<String, Value>,
    retired: std::collections::BTreeSet<String>,
    pinned: std::collections::BTreeSet<String>,
    last_trace: Option<Value>,
}

/// Outcome of one proposal.
pub enum ProposalResult {
    /// The fact was stored as proposed.
    Accepted(Value),
    /// An equivalent current revision already exists.
    Unchanged,
    /// The candidate was rejected with the exact reference reason.
    Rejected(String),
}

impl KnowledgeCoordinator {
    /// Construct the coordinator over injected ports and limit overrides.
    pub fn new(mut ports: KnowledgePorts, overrides: KnowledgeLimits) -> Self {
        let defaults = KnowledgeLimits::default();
        let limits = KnowledgeLimits {
            max_pinned_facts: overrides.max_pinned_facts,
            ..defaults
        };
        ports.secrets.retain(|secret| !secret.is_empty());
        Self {
            ports,
            limits,
            by_subject: BTreeMap::new(),
            current: BTreeMap::new(),
            one_off: BTreeMap::new(),
            retired: std::collections::BTreeSet::new(),
            pinned: std::collections::BTreeSet::new(),
            last_trace: None,
        }
    }

    fn confidence_for(candidate: &Value) -> &'static str {
        if candidate["proposedConfidence"].is_string() {
            match candidate["proposedConfidence"].as_str().expect("checked") {
                "high" => "high",
                "medium" => "medium",
                _ => "low",
            }
        } else {
            let evidence_count = candidate["provenance"]
                .as_array()
                .map(|list| list.len())
                .unwrap_or(0);
            if evidence_count > 0 { "medium" } else { "low" }
        }
    }

    fn validate_provenance_ref(
        &self,
        reference: &Value,
    ) -> Result<(), String> {
        let reference_type = reference["type"].as_str().unwrap_or_default();
        match reference_type {
            "evidence" => {
                let evidence_id =
                    reference["evidenceId"].as_str().unwrap_or_default();
                if evidence_id.is_empty() {
                    return Err(
                        "An evidence provenance reference requires an evidence id.".to_string(),
                    );
                }
                Ok(())
            }
            "workspace_file" => {
                let path = reference["path"].as_str().unwrap_or_default();
                let sha256 = reference["sha256"].as_str().unwrap_or_default();
                if path.is_empty()
                    || path.contains('\\')
                    || !path.chars().all(|character| {
                        character.is_ascii_alphanumeric()
                            || matches!(character, '.' | '/' | ' ' | '-')
                    })
                {
                    return Err(format!(
                        "The workspace-file provenance path \"{path}\" is malformed."
                    ));
                }
                if sha256.len() != 64
                    || !sha256.bytes().all(|byte| {
                        byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
                    })
                {
                    return Err(
                        "A workspace-file provenance reference requires the exact 64-hex-digit SHA-256."
                            .to_string(),
                    );
                }
                let known = self.ports.file_states.iter().any(
                    |(known_path, known_sha)| {
                        known_path == path && known_sha == sha256
                    },
                );
                if !known {
                    return Err(format!(
                        "The referenced file state \"{path}\" does not match the current workspace; reread the file before citing it."
                    ));
                }
                Ok(())
            }
            "research_evidence" => {
                let evidence_id =
                    reference["evidenceId"].as_str().unwrap_or_default();
                if evidence_id.is_empty() {
                    return Err(
                        "A research-evidence provenance reference requires an evidence id."
                            .to_string(),
                    );
                }
                let source = &reference["source"];
                let kind = source["kind"].as_str().unwrap_or_default();
                let id = source["id"].as_str().unwrap_or_default();
                let label = source["label"].as_str().unwrap_or_default();
                if !matches!(kind, "repository" | "godot-docs" | "fake")
                    || id.is_empty()
                    || id.len() > 128
                    || label.is_empty()
                    || label.len() > 128
                {
                    return Err(
                        "A research-evidence provenance reference requires a valid research source."
                            .to_string(),
                    );
                }
                if reference["fetchedAtMs"].as_f64().is_none() {
                    return Err(
                        "A research-evidence provenance reference requires a valid fetch timestamp."
                            .to_string(),
                    );
                }
                match &self.ports.research_evidence_ids {
                    None => Err("Research-evidence provenance requires host verification; no research-evidence verifier is configured.".to_string()),
                    Some(ids) => {
                        if ids.iter().any(|known| known == evidence_id) {
                            Ok(())
                        } else {
                            Err(format!(
                                "The referenced research evidence \"{evidence_id}\" does not exist; a fact cannot cite missing evidence."
                            ))
                        }
                    }
                }
            }
            _ => Err("Unknown provenance reference type.".to_string()),
        }
    }

    fn validate_candidate(&self, candidate: &Value) -> Result<(), String> {
        let content = candidate["content"].as_str().unwrap_or_default();
        if content.trim().is_empty() {
            return Err(
                "A knowledge fact requires non-empty content.".to_string()
            );
        }
        if utf8_len(content) > self.limits.max_content_bytes {
            return Err(format!(
                "Knowledge content exceeds the limit of {} bytes.",
                self.limits.max_content_bytes
            ));
        }
        if let Some(subject_key) = candidate.get("subjectKey") {
            let key = subject_key.as_str().unwrap_or_default();
            if !is_valid_subject_key(key) {
                return Err(format!(
                    "The subject key \"{key}\" is malformed; use a lowercase dotted key such as project.godot.version."
                ));
            }
        }
        if let Some(kind) = candidate["type"].as_str() {
            if !matches!(kind, "fact" | "decision" | "convention") {
                return Err("Unknown knowledge fact type.".to_string());
            }
        }
        if let Some(references) = candidate["provenance"].as_array() {
            for reference in references {
                self.validate_provenance_ref(reference)?;
            }
        }
        for secret in &self.ports.secrets {
            if content.contains(secret.as_str()) {
                return Err(
                    "The candidate contains a known secret and cannot be stored as knowledge."
                        .to_string(),
                );
            }
        }
        if let Some(reason) = reject_policy_shaped_content(content) {
            return Err(reason.to_string());
        }
        Ok(())
    }

    fn build_fact(
        &self,
        subject_key: Option<&str>,
        revision: u64,
        candidate: &Value,
        activation: &str,
    ) -> Value {
        let content = candidate["content"].as_str().unwrap_or_default();
        let timestamp = self.ports.now_ms;
        json!({
            "id": compute_fact_id(subject_key, content, revision),
            "scope": "project",
            "subjectKey": subject_key,
            "type": candidate["type"].as_str().unwrap_or("fact"),
            "content": content,
            "contentDigest": compute_knowledge_fact_content_digest(content),
            "revision": revision,
            "provenance": candidate.get("provenance").cloned().unwrap_or(json!([])),
            "confidence": Self::confidence_for(candidate),
            "volatility": candidate["proposedVolatility"].as_str().unwrap_or("normal"),
            "createdAtMs": timestamp,
            "updatedAtMs": timestamp,
            "lastVerifiedAtMs": candidate.get("lastVerifiedAtMs").and_then(Value::as_u64).map(Value::from).unwrap_or(Value::Null),
            "expiresAtMs": candidate.get("expiresAtMs").and_then(Value::as_u64).map(Value::from).unwrap_or(Value::Null),
            "activation": activation,
        })
    }

    /// The single mutation entry point for proposing facts.
    pub fn propose(&mut self, candidate: &Value) -> ProposalResult {
        if let Err(reason) = self.validate_candidate(candidate) {
            return ProposalResult::Rejected(reason);
        }
        match candidate.get("subjectKey").and_then(Value::as_str) {
            Some(subject_key) => self.propose_subject(candidate, subject_key),
            None => self.propose_one_off(candidate),
        }
    }

    fn propose_subject(
        &mut self,
        candidate: &Value,
        subject_key: &str,
    ) -> ProposalResult {
        if let Some(current_index) = self.current.get(subject_key).copied() {
            if let Some(entry) = self.by_subject.get(subject_key) {
                if let Some(current) = entry.get(current_index - 1) {
                    if normalize_fact_content(
                        current["content"].as_str().unwrap_or_default(),
                    ) == normalize_fact_content(
                        candidate["content"].as_str().unwrap_or_default(),
                    ) {
                        return ProposalResult::Unchanged;
                    }
                }
            }
        }
        let next_revision = match self.by_subject.get(subject_key) {
            Some(entry) => entry.len() + 1,
            None => 1,
        };
        if let Some(entry) = self.by_subject.get(subject_key) {
            if entry.len() >= self.limits.max_revisions_per_subject {
                return ProposalResult::Rejected(format!(
                    "The subject \"{subject_key}\" has reached the revision-history limit of {}; retire and re-create it instead of churning revisions.",
                    self.limits.max_revisions_per_subject
                ));
            }
        }
        let new_subject = !self.by_subject.contains_key(subject_key);
        if new_subject
            && self.one_off.len() + self.by_subject.len()
                >= self.limits.max_facts
        {
            return ProposalResult::Rejected(format!(
                "The knowledge store reached the limit of {} active facts.",
                self.limits.max_facts
            ));
        }
        let wants_pin = candidate["pinned"].as_bool() == Some(true);
        let activation =
            if wants_pin && self.pinned.len() < self.limits.max_pinned_facts {
                "pinned"
            } else {
                "retrieved"
            };
        if wants_pin && activation != "pinned" {
            return ProposalResult::Rejected(format!(
                "The pinned-knowledge budget ({}) is exhausted; unpin another fact first.",
                self.limits.max_pinned_facts
            ));
        }
        let fact = self.build_fact(
            Some(subject_key),
            next_revision as u64,
            candidate,
            activation,
        );
        self.by_subject
            .entry(subject_key.to_string())
            .or_default()
            .push(fact.clone());
        self.current.insert(subject_key.to_string(), next_revision);
        self.retired.remove(subject_key);
        if activation == "pinned" {
            self.pinned.insert(subject_key.to_string());
        }
        ProposalResult::Accepted(fact)
    }

    fn propose_one_off(&mut self, candidate: &Value) -> ProposalResult {
        if self.one_off.len() + self.by_subject.len() >= self.limits.max_facts
        {
            return ProposalResult::Rejected(format!(
                "The knowledge store reached the limit of {} active facts.",
                self.limits.max_facts
            ));
        }
        let fact = self.build_fact(None, 1, candidate, "retrieved");
        let id = fact["id"].as_str().expect("id").to_string();
        self.one_off.insert(id, fact.clone());
        ProposalResult::Accepted(fact)
    }

    /// Current active revision of a subject as JSON, or null.
    pub fn fact(&self, subject_key: &str) -> Value {
        match (
            self.by_subject.get(subject_key),
            self.current.get(subject_key).copied(),
        ) {
            (Some(entry), Some(index)) => entry[index - 1].clone(),
            _ => Value::Null,
        }
    }

    /// Immutable revision history of a subject, oldest first.
    pub fn history(&self, subject_key: &str) -> Vec<Value> {
        self.by_subject.get(subject_key).cloned().unwrap_or_default()
    }

    /// All current active facts sorted by id.
    pub fn active_facts(&self) -> Vec<Value> {
        let mut facts: Vec<Value> = Vec::new();
        for (subject_key, entry) in &self.by_subject {
            let Some(index) = self.current.get(subject_key).copied() else {
                continue;
            };
            if let Some(fact) = entry.get(index - 1) {
                facts.push(fact.clone());
            }
        }
        facts.extend(self.one_off.values().cloned());
        facts.sort_by(|left, right| {
            left["id"].as_str().cmp(&right["id"].as_str())
        });
        facts
    }

    /// Retired subjects whose revisions are retained, sorted.
    pub fn retired_subjects(&self) -> Vec<String> {
        self.retired.iter().cloned().collect()
    }

    /// Bounded pinned projection sorted by subject key.
    pub fn pinned_facts(&self) -> Vec<Value> {
        let mut facts: Vec<Value> = self
            .pinned
            .iter()
            .map(|subject_key| self.fact(subject_key))
            .filter(|fact| !fact.is_null())
            .collect();
        facts.sort_by(|left, right| {
            left["subjectKey"].as_str().cmp(&right["subjectKey"].as_str())
        });
        facts
    }

    fn replace_activation(&mut self, subject_key: &str, activation: &str) {
        if let Some(entry) = self.by_subject.get_mut(subject_key) {
            if let Some(index) = self.current.get(subject_key).copied() {
                if let Some(current) = entry.get_mut(index - 1) {
                    current["activation"] = json!(activation);
                }
            }
        }
    }

    fn pinned_projection_bytes(&self) -> usize {
        self.pinned
            .iter()
            .map(|subject_key| self.fact(subject_key))
            .filter(|fact| !fact.is_null())
            .map(|fact| fact_projection_bytes(&fact))
            .sum()
    }

    /// Move a fact into the bounded pinned set.
    pub fn pin(&mut self, subject_key: &str) -> Result<(), String> {
        let fact = self.fact(subject_key);
        if fact.is_null() {
            return Err(format!(
                "No active fact exists for subject \"{subject_key}\"."
            ));
        }
        if self.pinned.contains(subject_key) {
            return Ok(());
        }
        if self.pinned.len() >= self.limits.max_pinned_facts {
            return Err(format!(
                "The pinned-knowledge budget ({} facts) is exhausted; unpin another fact first.",
                self.limits.max_pinned_facts
            ));
        }
        let projected = self.pinned_projection_bytes()
            + utf8_len(fact["content"].as_str().unwrap_or_default());
        if projected > self.limits.max_pinned_bytes {
            return Err(format!(
                "Pinning \"{subject_key}\" would exceed the pinned-knowledge byte budget ({}).",
                self.limits.max_pinned_bytes
            ));
        }
        self.pinned.insert(subject_key.to_string());
        self.replace_activation(subject_key, "pinned");
        Ok(())
    }

    /// Remove a fact from the pinned set.
    pub fn unpin(&mut self, subject_key: &str) {
        if self.pinned.remove(subject_key) {
            self.replace_activation(subject_key, "retrieved");
        }
    }

    /// Retire a subject: the current pointer becomes absent.
    pub fn retire(&mut self, subject_key: &str) {
        if !self.by_subject.contains_key(subject_key) {
            return;
        }
        self.current.remove(subject_key);
        self.retired.insert(subject_key.to_string());
        self.pinned.remove(subject_key);
    }

    /// Deterministic bounded retrieval; expired and pinned facts excluded.
    pub fn retrieve(&mut self, query: &Value) -> Value {
        let timestamp = self.ports.now_ms;
        let limit = query["limit"]
            .as_u64()
            .map(|value| (value as usize).min(self.limits.max_retrieval_facts))
            .unwrap_or(self.limits.max_retrieval_facts);
        let max_bytes = query["maxBytes"]
            .as_u64()
            .map(|value| (value as usize).min(self.limits.max_retrieval_bytes))
            .unwrap_or(self.limits.max_retrieval_bytes);
        let query_tokens =
            tokenize_fact_text(query["text"].as_str().unwrap_or_default());
        let query_subject = query["subjectKey"].as_str().map(str::to_string);
        let query_paths: Vec<String> = query["paths"]
            .as_array()
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| {
                        let raw = path.as_str().unwrap_or_default();
                        let unified = raw.replace('\\', "/");
                        unified
                            .strip_prefix("./")
                            .unwrap_or(&unified)
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();

        struct Considered {
            fact: Value,
            score: f64,
            reasons: Vec<String>,
        }
        let mut considered: Vec<Considered> = Vec::new();
        for fact in self.active_facts() {
            if fact["activation"] == "pinned" || is_expired(&fact, timestamp) {
                continue;
            }
            if let Some(types) = query["factTypes"].as_array() {
                let matches_type = types
                    .iter()
                    .any(|kind| kind.as_str() == fact["type"].as_str());
                if !matches_type {
                    continue;
                }
            }
            let mut relevance = 0i64;
            let mut reasons: Vec<String> = Vec::new();
            if let Some(subject_query) = &query_subject {
                let subject_key =
                    fact["subjectKey"].as_str().unwrap_or_default();
                if subject_query == subject_key {
                    relevance += SUBJECT_EXACT;
                    reasons.push("exact subject-key match".to_string());
                } else if subject_key.starts_with(&format!("{subject_query}."))
                    || subject_query.starts_with(&format!("{subject_key}."))
                {
                    relevance += SUBJECT_PREFIX;
                    reasons.push("subject-key prefix match".to_string());
                }
            }
            let haystack = format!(
                "{} {}",
                fact["subjectKey"].as_str().unwrap_or_default(),
                fact["content"].as_str().unwrap_or_default()
            );
            let fact_tokens = tokenize_fact_text(&haystack);
            let overlap = query_tokens
                .iter()
                .filter(|token| fact_tokens.contains(token))
                .count() as i64;
            if overlap > 0 {
                relevance += (overlap * KEYWORD_PER_OVERLAP).min(KEYWORD_CAP);
                reasons.push(format!("keyword overlap ({overlap})"));
            }
            for reference in
                fact["provenance"].as_array().expect("provenance array")
            {
                if reference["type"] == "workspace_file" {
                    let reference_path =
                        reference["path"].as_str().unwrap_or_default();
                    let relevant = query_paths.iter().any(|path| {
                        reference_path == path
                            || reference_path.starts_with(&format!("{path}/"))
                            || path.starts_with(&format!("{reference_path}/"))
                    });
                    if relevant {
                        relevance += PATH_RELEVANCE;
                        reasons.push(format!(
                            "provenance path relevance ({reference_path})"
                        ));
                        break;
                    }
                }
            }
            if relevance <= 0 {
                continue;
            }
            let confidence = confidence_weight(
                fact["confidence"].as_str().unwrap_or("low"),
            );
            if confidence > 0 {
                relevance += confidence;
                reasons.push(format!(
                    "confidence {}",
                    fact["confidence"].as_str().unwrap_or("low")
                ));
            }
            let volatility = fact["volatility"].as_str().unwrap_or("normal");
            let window_days = freshness_window_days(volatility);
            if window_days > 0.0 {
                let age_days = (timestamp
                    .saturating_sub(fact["updatedAtMs"].as_u64().unwrap_or(0)))
                    as f64
                    / 86_400_000.0;
                let decayed = freshness_weight(volatility)
                    * (1.0 - age_days / window_days).max(0.0);
                if decayed > 0.0 {
                    relevance += decayed as i64;
                    reasons.push(format!(
                        "freshness {}d",
                        age_days.round() as i64
                    ));
                }
            } else {
                relevance += freshness_weight(volatility) as i64;
                reasons.push("freshness 0d".to_string());
            }
            considered.push(Considered {
                fact: fact.clone(),
                score: relevance as f64,
                reasons,
            });
        }
        considered.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| {
                    let left_key =
                        left.fact["subjectKey"].as_str().unwrap_or_else(
                            || left.fact["id"].as_str().expect("id"),
                        );
                    let right_key =
                        right.fact["subjectKey"].as_str().unwrap_or_else(
                            || right.fact["id"].as_str().expect("id"),
                        );
                    left_key.cmp(right_key)
                })
                .then_with(|| {
                    right.fact["revision"]
                        .as_u64()
                        .cmp(&left.fact["revision"].as_u64())
                })
        });
        let mut selections: Vec<Value> = Vec::new();
        let mut selected_facts: Vec<Value> = Vec::new();
        let mut used_bytes = 0usize;
        let mut omitted = 0usize;
        for candidate in &considered {
            let bytes = fact_projection_bytes(&candidate.fact);
            if selected_facts.len() >= limit || used_bytes + bytes > max_bytes
            {
                omitted += 1;
                continue;
            }
            used_bytes += bytes;
            selected_facts.push(candidate.fact.clone());
            let rounded_score = ((candidate.score * 100.0).round()) / 100.0;
            let score_value = if rounded_score.fract() == 0.0 {
                json!(rounded_score as i64)
            } else {
                json!(rounded_score)
            };
            selections.push(json!({
                "factId": candidate.fact["id"],
                "subjectKey": candidate.fact["subjectKey"],
                "revision": candidate.fact["revision"],
                "confidence": candidate.fact["confidence"],
                "volatility": candidate.fact["volatility"],
                "score": score_value,
                "matchReasons": candidate.reasons,
                "expiresAtMs": candidate.fact["expiresAtMs"],
            }));
        }
        let trace = json!({
            "atMs": timestamp,
            "scope": "project",
            "query": {
                "text": query["text"].as_str().map(Value::from).unwrap_or(Value::Null),
                "subjectKey": query_subject,
                "paths": query_paths,
                "factTypes": query.get("factTypes").cloned().unwrap_or(json!([])),
            },
            "selected": selections,
            "consideredCount": considered.len(),
            "omittedCount": omitted,
            "budget": { "limit": limit, "maxBytes": max_bytes, "usedBytes": used_bytes },
        });
        self.last_trace = Some(trace.clone());
        json!({ "facts": selected_facts, "trace": trace })
    }

    /// Trace of the most recent retrieval, or null.
    pub fn last_retrieval_trace(&self) -> Value {
        self.last_trace.clone().unwrap_or(Value::Null)
    }

    /// Deterministic digest over the current knowledge state.
    pub fn revision(&self) -> String {
        let entries: Vec<Value> = self
            .active_facts()
            .iter()
            .map(|fact| {
                json!({
                    "id": fact["id"],
                    "subjectKey": fact["subjectKey"],
                    "revision": fact["revision"],
                    "content": fact["content"],
                    "activation": fact["activation"],
                })
            })
            .collect();
        sha256_hex_str(&canonicalize_json(&json!({
            "version": KNOWLEDGE_STATE_VERSION,
            "facts": entries,
        })))
    }

    /// Active fact count (subjects plus one-off facts).
    pub fn size(&self) -> usize {
        self.by_subject.len() + self.one_off.len()
    }
}
