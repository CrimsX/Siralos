//! Read-only context demand-paging Tool adapters (decision 79 slice 4).
//!
//! Three read-only adapters over an immutable snapshot of the graph,
//! representation store, and scheduler state. The adapters never mutate
//! scheduler state — the model cannot promote, pin, or archive; access
//! events are host-observed. Results carry digests for provenance and
//! never echo absolute paths.

use serde_json::{Value, json};
use siralos_core::context_graph::{
    ContextGraph, ContextNodeKind, stale_context_nodes,
};
use siralos_core::context_representation::{
    ContextRepresentationStore, RepresentationLevel, available_levels,
    resolve_representation,
};
use siralos_core::context_scheduler::WorkingSetState;

fn node_kind_str(kind: ContextNodeKind) -> &'static str {
    match kind {
        ContextNodeKind::Source => "source",
        ContextNodeKind::Decision => "decision",
        ContextNodeKind::Knowledge => "knowledge",
        ContextNodeKind::Run => "run",
        ContextNodeKind::Skill => "skill",
        ContextNodeKind::Task => "task",
    }
}
use siralos_core::provider::{
    CancellationSignal, ToolDefinition, ToolExecutionResult,
};
use siralos_core::tool::{CapabilityId, Tool};

const CONTEXT_READ_CAPABILITY: &str = "context.read";
const MAX_RESULTS: usize = 16;
const MAX_QUERY_BYTES: usize = 256;
const MAX_NODE_ID_BYTES: usize = 256;

// ---------------------------------------------------------------------------
// Decision 92 scoring — deterministic search-scoring re-rank (external-model #5)
// S1 STOPWORDS: closed list, pinned exactly
// S2 scoring: whole-word summary +10 else substring +3; whole-word id +4 else substring +1; tier +8 HOT/+4 WARM; kind +3 Knowledge/+1 Source
// S3 hit iff any non-stopword term substring in summary or id
// S4 ordering score desc, node_id asc; result carries score
// S5 read-only (snapshot only)
// ---------------------------------------------------------------------------
/// Closed stopword list for decision 92 search scoring (pinned).
pub const SEARCH_STOPWORDS: [&str; 17] = [
    "a", "an", "and", "are", "as", "at", "by", "for", "from", "in", "is",
    "of", "on", "or", "the", "to", "with",
];

fn is_stopword(term_lower: &str) -> bool {
    SEARCH_STOPWORDS.contains(&term_lower)
}

fn extract_query_terms(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_owned())
        .collect()
}

fn filtered_terms(query: &str) -> Vec<String> {
    extract_query_terms(query)
        .into_iter()
        .filter(|t| !is_stopword(t))
        .collect()
}

fn is_whole_word(term_lower: &str, text_lower: &str) -> bool {
    text_lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token == term_lower)
}

fn score_node(
    node_summary: &str,
    node_id: &str,
    terms: &[String],
    tier_bonus: i32,
    kind_bonus: i32,
) -> i32 {
    let summary_lower = node_summary.to_lowercase();
    let id_lower = node_id.to_lowercase();
    let mut score = tier_bonus + kind_bonus;
    for term in terms {
        // summary part
        if is_whole_word(term, &summary_lower) {
            score += 10;
        } else if summary_lower.contains(term.as_str()) {
            score += 3;
        }
        // id part
        if is_whole_word(term, &id_lower) {
            score += 4;
        } else if id_lower.contains(term.as_str()) {
            score += 1;
        }
    }
    score
}

fn tier_bonus_for(
    entry: Option<&siralos_core::context_scheduler::SchedulerEntry>,
) -> i32 {
    match entry.map(|e| e.tier) {
        Some(siralos_core::context_scheduler::WorkingSetTier::Hot) => 8,
        Some(siralos_core::context_scheduler::WorkingSetTier::Warm) => 4,
        _ => 0,
    }
}

fn kind_bonus_for(kind: siralos_core::context_graph::ContextNodeKind) -> i32 {
    match kind {
        siralos_core::context_graph::ContextNodeKind::Knowledge => 3,
        siralos_core::context_graph::ContextNodeKind::Source => 1,
        _ => 0,
    }
}

fn context_capability() -> CapabilityId {
    CapabilityId::parse(CONTEXT_READ_CAPABILITY)
        .expect("context.read is a valid capability id")
}

/// Immutable snapshot of context state for demand-paging tools.
///
/// The host builds this snapshot and shares it immutably with the adapters.
/// `current_digests` carries the caller's current bindings for staleness
/// computation; stale is `stale_context_nodes(graph, current)` containment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextToolState {
    /// Context graph (reconstructable, bounded).
    pub graph: ContextGraph,
    /// Representation store (L0-L4).
    pub store: ContextRepresentationStore,
    /// Scheduler working-set state.
    pub state: WorkingSetState,
    /// Caller-provided current digests for staleness.
    pub current_digests: Vec<(String, String)>,
}

impl ContextToolState {
    /// Build a snapshot from parts.
    #[must_use]
    pub fn new(
        graph: ContextGraph,
        store: ContextRepresentationStore,
        state: WorkingSetState,
        current_digests: Vec<(String, String)>,
    ) -> Self {
        Self { graph, store, state, current_digests }
    }
}

// ---------------------------------------------------------------------------
// ContextInspectTool
// ---------------------------------------------------------------------------

/// `context.inspect` — read-only node metadata.
pub struct ContextInspectTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    snapshot: ContextToolState,
}

impl ContextInspectTool {
    /// Construct over an immutable snapshot.
    pub fn new(snapshot: ContextToolState) -> Self {
        Self {
            definition: ToolDefinition {
                name: "context.inspect".to_owned(),
                description: "Inspect one context node: tier, pin, stale, and available levels."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "node_id": {
                            "type": "string",
                            "description": "Node id to inspect."
                        }
                    },
                    "required": ["node_id"],
                    "additionalProperties": false
                }),
            },
            capability: context_capability(),
            snapshot,
        }
    }
}

impl Tool for ContextInspectTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Inspection was cancelled.".to_owned(),
            };
        }
        let node_id = match parse_node_id_input(input) {
            Ok(id) => id,
            Err(message) => {
                return ToolExecutionResult::InvalidInput { message };
            }
        };
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Inspection was cancelled.".to_owned(),
            };
        }
        let graph = &self.snapshot.graph;
        let node = match graph.nodes().iter().find(|n| n.id == node_id) {
            Some(n) => n,
            None => {
                return ToolExecutionResult::Failed {
                    message: format!("unknown node: {node_id}"),
                };
            }
        };
        let state_entry = self
            .snapshot
            .state
            .entries()
            .iter()
            .find(|e| e.node_id == node_id);
        let (tier_str, pinned) = match state_entry {
            Some(entry) => (entry.tier.as_str().to_owned(), entry.pinned),
            None => ("cold".to_owned(), false),
        };
        let stale_nodes =
            stale_context_nodes(graph, &self.snapshot.current_digests);
        let stale = stale_nodes.iter().any(|s| s.node_id == node_id);
        let available_levels: Vec<String> =
            match self.snapshot.store.set(&node_id) {
                Some(set) => available_levels(set)
                    .into_iter()
                    .map(|lvl| lvl.as_str().to_owned())
                    .collect(),
                None => Vec::new(),
            };
        ToolExecutionResult::Success {
            output: json!({
                "id": node.id,
                "kind": node_kind_str(node.kind),
                "content_digest": node.content_digest,
                "summary": node.summary,
                "tier": tier_str,
                "pinned": pinned,
                "stale": stale,
                "availableLevels": available_levels,
                "tokenEstimate": node.token_estimate,
            }),
            summary: format!("inspected {node_id}"),
        }
    }
}

// ---------------------------------------------------------------------------
// ContextSearchTool
// ---------------------------------------------------------------------------

/// `context.search` — deterministic lexical match over node ids and summaries.
pub struct ContextSearchTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    snapshot: ContextToolState,
}

impl ContextSearchTool {
    /// Construct over an immutable snapshot.
    pub fn new(snapshot: ContextToolState) -> Self {
        Self {
            definition: ToolDefinition {
                name: "context.search".to_owned(),
                description:
                    "Lexical search over context node ids and summaries."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Literal text to search for."
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            },
            capability: context_capability(),
            snapshot,
        }
    }
}

impl Tool for ContextSearchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Search was cancelled.".to_owned(),
            };
        }
        let query = match parse_query_input(input) {
            Ok(q) => q,
            Err(message) => {
                return ToolExecutionResult::InvalidInput { message };
            }
        };
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Search was cancelled.".to_owned(),
            };
        }
        let terms = filtered_terms(&query);
        // S1: query whose terms are ALL stopwords matches nothing (empty result)
        if terms.is_empty() {
            return ToolExecutionResult::Success {
                output: json!({
                    "query": query,
                    "hits": [],
                    "truncated": false,
                    "hitCount": 0,
                }),
                summary: "0 matches".to_owned(),
            };
        }
        let mut scored_hits: Vec<(i32, String, Value)> = Vec::new();
        for node in self.snapshot.graph.nodes() {
            if cancellation.is_cancelled() {
                return ToolExecutionResult::Cancelled {
                    message: "Search was cancelled.".to_owned(),
                };
            }
            let summary_lower = node.summary.to_lowercase();
            let id_lower = node.id.to_lowercase();
            // S3 HIT CRITERION UNCHANGED: substring in summary or id for any term
            let is_hit = terms.iter().any(|t| {
                summary_lower.contains(t.as_str())
                    || id_lower.contains(t.as_str())
            });
            if !is_hit {
                continue;
            }
            let entry = self
                .snapshot
                .state
                .entries()
                .iter()
                .find(|e| e.node_id == node.id);
            let tier_str = entry
                .map(|e| e.tier.as_str().to_owned())
                .unwrap_or_else(|| "cold".to_owned());
            let tier_bonus = tier_bonus_for(entry);
            let kind_bonus = kind_bonus_for(node.kind);
            let score = score_node(
                &node.summary,
                &node.id,
                &terms,
                tier_bonus,
                kind_bonus,
            );
            // deterministic matched_in: id if any term substring in id, else summary
            let matched_in =
                if terms.iter().any(|t| id_lower.contains(t.as_str())) {
                    "id"
                } else {
                    "summary"
                };
            let hit = json!({
                "node_id": node.id,
                "kind": node_kind_str(node.kind),
                "tier": tier_str,
                "matched_in": matched_in,
                "score": score,
            });
            scored_hits.push((score, node.id.clone(), hit));
        }
        // S4 ORDERING: score descending, tie-break node_id ascending
        scored_hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let hit_count = scored_hits.len();
        let truncated = hit_count > MAX_RESULTS;
        let mut hits: Vec<Value> =
            scored_hits.into_iter().map(|(_, _, v)| v).collect();
        if truncated {
            hits.truncate(MAX_RESULTS);
        }
        ToolExecutionResult::Success {
            output: json!({
                "query": query,
                "hits": hits,
                "truncated": truncated,
                "hitCount": hit_count,
            }),
            summary: format!(
                "{hit_count} matches{}",
                if truncated { " (truncated)" } else { "" }
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// ContextExpandTool
// ---------------------------------------------------------------------------

/// `context.expand` — resolve one representation level.
pub struct ContextExpandTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    snapshot: ContextToolState,
}

impl ContextExpandTool {
    /// Construct over an immutable snapshot.
    pub fn new(snapshot: ContextToolState) -> Self {
        Self {
            definition: ToolDefinition {
                name: "context.expand".to_owned(),
                description: "Expand one context node at a specific representation level."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "node_id": {
                            "type": "string",
                            "description": "Node id to expand."
                        },
                        "level": {
                            "type": "string",
                            "enum": ["identity","summary","structured","detailed","source"],
                            "description": "Representation level."
                        }
                    },
                    "required": ["node_id", "level"],
                    "additionalProperties": false
                }),
            },
            capability: context_capability(),
            snapshot,
        }
    }
}

impl Tool for ContextExpandTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Expansion was cancelled.".to_owned(),
            };
        }
        let (node_id, level_str) = match parse_expand_input(input) {
            Ok(v) => v,
            Err(message) => {
                return ToolExecutionResult::InvalidInput { message };
            }
        };
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Expansion was cancelled.".to_owned(),
            };
        }
        let level = match parse_level(&level_str) {
            Some(l) => l,
            None => {
                return ToolExecutionResult::InvalidInput {
                    message: "\"level\" must be one of identity|summary|structured|detailed|source.".to_owned(),
                };
            }
        };
        let graph = &self.snapshot.graph;
        if graph.nodes().iter().all(|n| n.id != node_id) {
            return ToolExecutionResult::Failed {
                message: format!("unknown node: {node_id}"),
            };
        }
        let set = match self.snapshot.store.set(&node_id) {
            Some(s) => s,
            None => {
                return ToolExecutionResult::Unavailable {
                    message: format!(
                        "not resident at level {}; available: []",
                        level.as_str()
                    ),
                };
            }
        };
        let available: Vec<String> = available_levels(set)
            .into_iter()
            .map(|lvl| lvl.as_str().to_owned())
            .collect();
        match resolve_representation(set, level) {
            Some(rep) => {
                let derived: Vec<Value> = rep
                    .derived_from
                    .iter()
                    .map(|(dep, digest)| {
                        json!({
                            "dependency": dep,
                            "digest": digest
                        })
                    })
                    .collect();
                ToolExecutionResult::Success {
                    output: json!({
                        "node_id": node_id,
                        "level": level.as_str(),
                        "origin": rep.origin.as_str(),
                        "content": rep.content,
                        "content_digest": rep.content_digest,
                        "derived_from": derived,
                    }),
                    summary: format!(
                        "expanded {node_id} at {}",
                        level.as_str()
                    ),
                }
            }
            None => ToolExecutionResult::Unavailable {
                message: format!(
                    "not resident at level {}; available: [{}]",
                    level.as_str(),
                    available.join(", ")
                ),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Input parsers (mirror workspace adapter conventions)
// ---------------------------------------------------------------------------

fn parse_node_id_input(input: &Value) -> Result<String, String> {
    let object = match input {
        Value::Object(obj) => obj,
        _ => return Err("Tool input must be a JSON object.".to_owned()),
    };
    match object.get("node_id") {
        Some(Value::String(value)) if !value.is_empty() => {
            if value.len() > MAX_NODE_ID_BYTES {
                return Err("\"node_id\" is too long.".to_owned());
            }
            if value.contains('\\') || value.contains('\0') {
                return Err("\"node_id\" must be a bounded id.".to_owned());
            }
            Ok(value.clone())
        }
        Some(Value::String(_)) => Err("\"node_id\" is required.".to_owned()),
        Some(_) => Err("\"node_id\" must be a string.".to_owned()),
        None => Err("\"node_id\" is required.".to_owned()),
    }
}

fn parse_query_input(input: &Value) -> Result<String, String> {
    let object = match input {
        Value::Object(obj) => obj,
        _ => return Err("Tool input must be a JSON object.".to_owned()),
    };
    match object.get("query") {
        Some(Value::String(value)) if !value.is_empty() => {
            if value.len() > MAX_QUERY_BYTES {
                return Err("\"query\" is too long.".to_owned());
            }
            // Empty after trim is considered empty per workspace conventions.
            if value.trim().is_empty() {
                return Err("\"query\" is required.".to_owned());
            }
            Ok(value.clone())
        }
        Some(Value::String(_)) => Err("\"query\" is required.".to_owned()),
        Some(_) => Err("\"query\" must be a string.".to_owned()),
        None => Err("\"query\" is required.".to_owned()),
    }
}

fn parse_expand_input(input: &Value) -> Result<(String, String), String> {
    let object = match input {
        Value::Object(obj) => obj,
        _ => return Err("Tool input must be a JSON object.".to_owned()),
    };
    let node_id = match object.get("node_id") {
        Some(Value::String(v)) if !v.is_empty() => {
            if v.len() > MAX_NODE_ID_BYTES {
                return Err("\"node_id\" is too long.".to_owned());
            }
            if v.contains('\\') || v.contains('\0') {
                return Err("\"node_id\" must be a bounded id.".to_owned());
            }
            v.clone()
        }
        Some(Value::String(_)) => {
            return Err("\"node_id\" is required.".to_owned());
        }
        Some(_) => return Err("\"node_id\" must be a string.".to_owned()),
        None => return Err("\"node_id\" is required.".to_owned()),
    };
    let level = match object.get("level") {
        Some(Value::String(v)) if !v.is_empty() => v.clone(),
        Some(Value::String(_)) => {
            return Err("\"level\" is required.".to_owned());
        }
        Some(_) => return Err("\"level\" must be a string.".to_owned()),
        None => return Err("\"level\" is required.".to_owned()),
    };
    // Reject extra fields for determinism.
    if object.len() != 2 {
        // Mirror workspace: additionalProperties false is InvalidInput with generic message.
        // Accept this as invalid input; prefer precise extra-field message if present.
        let allowed = ["node_id", "level"];
        for key in object.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(format!("\"{key}\" is not allowed."));
            }
        }
    }
    Ok((node_id, level))
}

fn parse_level(value: &str) -> Option<RepresentationLevel> {
    match value {
        "identity" => Some(RepresentationLevel::Identity),
        "summary" => Some(RepresentationLevel::Summary),
        "structured" => Some(RepresentationLevel::Structured),
        "detailed" => Some(RepresentationLevel::Detailed),
        "source" => Some(RepresentationLevel::Source),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use siralos_core::context_graph::{
        ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
        ContextNodeKind, estimate_tokens,
    };
    use siralos_core::context_representation::{
        ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    use siralos_core::context_scheduler::{
        SchedulerEntry, WorkingSetState, WorkingSetTier,
    };
    use siralos_core::provider::{CancellationToken, ToolExecutionResult};
    use siralos_core::tool::Tool;

    use super::{
        ContextExpandTool, ContextInspectTool, ContextSearchTool,
        ContextToolState, MAX_RESULTS,
    };

    use serde_json::json;

    fn build_fixture() -> ContextToolState {
        use siralos_core::identity::sha256_hex;
        let a_body = "body-a";
        let b_body = "body-b";
        let k_body = "body-k";
        let a_digest = sha256_hex(a_body.as_bytes());
        let b_digest = sha256_hex(b_body.as_bytes());
        let k_digest = sha256_hex(k_body.as_bytes());
        let a_mutated = sha256_hex("mutated-a".as_bytes());
        let nodes = vec![
            ContextNode {
                id: "ctx-a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: a_digest.clone(),
                summary: "auth overview for ctx-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("auth overview for ctx-a"),
            },
            ContextNode {
                id: "ctx-b".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: b_digest.clone(),
                summary: "decision summary b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("decision summary b"),
            },
            ContextNode {
                id: "ctx-knowledge".to_owned(),
                kind: ContextNodeKind::Knowledge,
                content_digest: k_digest.clone(),
                summary: "knowledge with bindings".to_owned(),
                source_bindings: vec![
                    ("ctx-a".to_owned(), a_digest.clone()),
                    ("ctx-b".to_owned(), b_digest.clone()),
                ],
                token_estimate: estimate_tokens("knowledge with bindings"),
            },
        ];
        let edges = vec![
            ContextEdge {
                from: "ctx-a".to_owned(),
                to: "ctx-b".to_owned(),
                kind: ContextEdgeKind::Contains,
            },
            ContextEdge {
                from: "ctx-b".to_owned(),
                to: "ctx-knowledge".to_owned(),
                kind: ContextEdgeKind::DependsOn,
            },
        ];
        let graph = ContextGraph::build(nodes, edges).expect("graph");
        let identity_content_a = r#"{"id":"ctx-a","kind":"source"}"#;
        let structured_content_a = r#"["fact1","fact2"]"#;
        let identity_content_b = r#"{"id":"ctx-b","kind":"decision"}"#;
        let identity_content_k =
            r#"{"id":"ctx-knowledge","kind":"knowledge"}"#;
        let summary_content_k = "knowledge prose summary";
        let reps_a = vec![
            NodeRepresentation {
                level: RepresentationLevel::Identity,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(identity_content_a),
                derived_from: vec![],
                content: identity_content_a.to_owned(),
            },
            NodeRepresentation {
                level: RepresentationLevel::Structured,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(structured_content_a),
                derived_from: vec![],
                content: structured_content_a.to_owned(),
            },
        ];
        let reps_b = vec![NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of(identity_content_b),
            derived_from: vec![],
            content: identity_content_b.to_owned(),
        }];
        let reps_k = vec![
            NodeRepresentation {
                level: RepresentationLevel::Identity,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(identity_content_k),
                derived_from: vec![],
                content: identity_content_k.to_owned(),
            },
            NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::ModelDerived,
                content_digest: content_digest_of(summary_content_k),
                derived_from: vec![(
                    "ctx-knowledge".to_owned(),
                    content_digest_of(identity_content_k),
                )],
                content: summary_content_k.to_owned(),
            },
        ];
        let set_a = NodeRepresentationSet::build("ctx-a".to_owned(), reps_a)
            .expect("set_a");
        let set_b = NodeRepresentationSet::build("ctx-b".to_owned(), reps_b)
            .expect("set_b");
        let set_k =
            NodeRepresentationSet::build("ctx-knowledge".to_owned(), reps_k)
                .expect("set_k");
        let store =
            ContextRepresentationStore::build(vec![set_a, set_b, set_k])
                .expect("store");
        let entries = vec![
            SchedulerEntry {
                node_id: "ctx-a".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: true,
                relevance: 80,
                last_access_tick: 5,
                token_estimate: 1000,
                content_digest: a_digest.clone(),
            },
            SchedulerEntry {
                node_id: "ctx-b".to_owned(),
                tier: WorkingSetTier::Warm,
                pinned: false,
                relevance: 50,
                last_access_tick: 2,
                token_estimate: 1000,
                content_digest: b_digest.clone(),
            },
            SchedulerEntry {
                node_id: "ctx-knowledge".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 10,
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: k_digest.clone(),
            },
        ];
        let state = WorkingSetState::build(entries).expect("state");
        // Make ctx-knowledge stale by mutating ctx-a digest.
        let current = vec![
            ("ctx-a".to_owned(), a_mutated),
            ("ctx-b".to_owned(), b_digest),
        ];
        ContextToolState::new(graph, store, state, current)
    }

    #[test]
    fn inspect_full_fields_incl_available_levels_and_stale_flag() {
        let snapshot = build_fixture();
        let tool = ContextInspectTool::new(snapshot);
        let token = CancellationToken::new();
        let result =
            tool.execute(&json!({ "node_id": "ctx-a" }), token.signal());
        let ToolExecutionResult::Success { output, summary: _ } = result
        else {
            panic!("inspect failed");
        };
        assert_eq!(output["id"], "ctx-a");
        assert_eq!(output["kind"], "source");
        assert!(output["content_digest"].as_str().unwrap().len() == 64);
        assert_eq!(output["summary"], "auth overview for ctx-a");
        assert_eq!(output["tier"], "hot");
        assert_eq!(output["pinned"], true);
        assert_eq!(output["stale"], false);
        assert_eq!(
            output["availableLevels"],
            json!(["identity", "structured"])
        );
        assert!(output["tokenEstimate"].as_u64().unwrap() > 0);
    }

    #[test]
    fn inspect_unknown_node_typed_not_found() {
        let snapshot = build_fixture();
        let tool = ContextInspectTool::new(snapshot);
        let token = CancellationToken::new();
        let result =
            tool.execute(&json!({ "node_id": "missing" }), token.signal());
        assert!(matches!(result, ToolExecutionResult::Failed { .. }));
        if let ToolExecutionResult::Failed { message } = result {
            assert!(message.contains("unknown node"));
        }
    }

    #[test]
    fn search_hit_on_id_and_on_summary_canonical_order() {
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        // Search hits ctx-a via summary containing auth.
        let result = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("search failed");
        };
        let hits = output["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["node_id"], "ctx-a");
        assert_eq!(hits[0]["matched_in"], "summary");
        assert!(hits[0]["score"].is_number());
        // hits on id: query "ctx-" should hit all ordered by score desc then node_id asc.
        let result2 =
            tool.execute(&json!({ "query": "ctx-" }), token.signal());
        let ToolExecutionResult::Success { output: out2, .. } = result2 else {
            panic!("search failed");
        };
        let hits2 = out2["hits"].as_array().unwrap();
        assert_eq!(hits2.len(), 3);
        // Scores: ctx-a highest (Hot+Source+whole-word id/summary), then ctx-b, then ctx-knowledge
        assert_eq!(hits2[0]["node_id"], "ctx-a");
        assert_eq!(hits2[1]["node_id"], "ctx-b");
        assert_eq!(hits2[2]["node_id"], "ctx-knowledge");
        assert!(hits2.iter().all(|h| h["matched_in"] == "id"));
        assert!(hits2.iter().all(|h| h["score"].is_number()));
        // score descending tiebreak verified (scores descending)
        let s0 = hits2[0]["score"].as_i64().unwrap();
        let s1 = hits2[1]["score"].as_i64().unwrap();
        let s2 = hits2[2]["score"].as_i64().unwrap();
        assert!(s0 >= s1 && s1 >= s2);
    }

    #[test]
    fn search_max_results_cap() {
        let mut snapshot = build_fixture();
        // Expand fixture with many nodes to test cap.
        use siralos_core::context_graph::{ContextNode, ContextNodeKind};
        use siralos_core::identity::sha256_hex;
        let mut nodes: Vec<ContextNode> = snapshot.graph.nodes().to_vec();
        for i in 0..20 {
            let id = format!("extra-{i:02}");
            nodes.push(ContextNode {
                id: id.clone(),
                kind: ContextNodeKind::Source,
                content_digest: sha256_hex(id.as_bytes()),
                summary: format!("summary {i}"),
                source_bindings: vec![],
                token_estimate: 10,
            });
        }
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        snapshot.graph = graph;
        // Need to also add entries for new nodes to keep state coherent.
        let mut entries = snapshot.state.entries().to_vec();
        for i in 0..20 {
            entries.push(siralos_core::context_scheduler::SchedulerEntry {
                node_id: format!("extra-{i:02}"),
                tier: siralos_core::context_scheduler::WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: format!("{:064x}", i),
            });
        }
        snapshot.state = WorkingSetState::build(entries).expect("state");
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result =
            tool.execute(&json!({ "query": "extra-" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("search failed");
        };
        assert_eq!(output["hits"].as_array().unwrap().len(), MAX_RESULTS);
        assert_eq!(output["truncated"], true);
        assert_eq!(output["hitCount"], 20);
        // Every hit must carry a deterministic score
        for hit in output["hits"].as_array().unwrap() {
            assert!(hit["score"].is_number());
        }
    }

    #[test]
    fn search_empty_query_typed_invalid() {
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        assert!(matches!(
            tool.execute(&json!({ "query": "" }), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(matches!(
            tool.execute(&json!({}), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(matches!(
            tool.execute(&json!({ "query": 123 }), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
    }

    #[test]
    fn search_no_hits_empty_not_error() {
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result =
            tool.execute(&json!({ "query": "nope-nope" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("should be success");
        };
        assert_eq!(output["hits"].as_array().unwrap().len(), 0);
        assert_eq!(output["hitCount"], 0);
        assert_eq!(output["truncated"], false);
    }

    #[test]
    fn expand_resolves_l2_with_digest_origin_provenance() {
        let snapshot = build_fixture();
        let tool = ContextExpandTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({ "node_id": "ctx-a", "level": "structured" }),
            token.signal(),
        );
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("expand failed: {result:?}");
        };
        assert_eq!(output["node_id"], "ctx-a");
        assert_eq!(output["level"], "structured");
        assert_eq!(output["origin"], "host_extracted");
        assert!(output["content_digest"].as_str().unwrap().len() == 64);
        assert!(output["derived_from"].as_array().unwrap().is_empty());
        assert!(!output["content"].as_str().unwrap().is_empty());
    }

    #[test]
    fn expand_absent_level_typed_unavailable_with_available_list() {
        let snapshot = build_fixture();
        let tool = ContextExpandTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({ "node_id": "ctx-b", "level": "structured" }),
            token.signal(),
        );
        assert!(matches!(result, ToolExecutionResult::Unavailable { .. }));
        if let ToolExecutionResult::Unavailable { message } = result {
            assert!(message.contains("not resident at level structured"));
            assert!(message.contains("identity"));
        }
    }

    #[test]
    fn expand_unknown_node_typed_not_found() {
        let snapshot = build_fixture();
        let tool = ContextExpandTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({ "node_id": "unknown", "level": "identity" }),
            token.signal(),
        );
        assert!(matches!(result, ToolExecutionResult::Failed { .. }));
    }

    #[test]
    fn expand_malformed_level_typed_invalid() {
        let snapshot = build_fixture();
        let tool = ContextExpandTool::new(snapshot);
        let token = CancellationToken::new();
        assert!(matches!(
            tool.execute(
                &json!({ "node_id": "ctx-a", "level": "bogus" }),
                token.signal()
            ),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(matches!(
            tool.execute(
                &json!({ "node_id": "ctx-a", "level": "" }),
                token.signal()
            ),
            ToolExecutionResult::InvalidInput { .. }
        ));
    }

    #[test]
    fn no_mutation_call_tools_assert_state_unchanged() {
        let snapshot = build_fixture();
        let before = snapshot.state.entries().to_vec();
        let before_tick = snapshot.state.tick();
        let inspect = ContextInspectTool::new(snapshot.clone());
        let search = ContextSearchTool::new(snapshot.clone());
        let expand = ContextExpandTool::new(snapshot.clone());
        let token = CancellationToken::new();
        let _ =
            inspect.execute(&json!({ "node_id": "ctx-a" }), token.signal());
        let _ = search.execute(&json!({ "query": "auth" }), token.signal());
        let _ = expand.execute(
            &json!({ "node_id": "ctx-a", "level": "identity" }),
            token.signal(),
        );
        let _ = expand.execute(
            &json!({ "node_id": "ctx-b", "level": "structured" }),
            token.signal(),
        );
        // Verify the original snapshot's state is unchanged.
        assert_eq!(snapshot.state.entries(), before.as_slice());
        assert_eq!(snapshot.state.tick(), before_tick);
        // Also verify that tool snapshots did not mutate internal copies.
        assert_eq!(
            inspect.execute(&json!({ "node_id": "ctx-a" }), token.signal()),
            inspect.execute(&json!({ "node_id": "ctx-a" }), token.signal())
        );
    }

    // --- Decision 92 scoring tests (~9) ---

    #[test]
    fn stopword_filtering_list_terms_dropped_and_stopword_only_empty() {
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        // "the auth" -> "the" dropped, same as "auth"
        let r1 = tool.execute(&json!({ "query": "the auth" }), token.signal());
        let r2 = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output: o1, .. } = r1 else {
            panic!("r1")
        };
        let ToolExecutionResult::Success { output: o2, .. } = r2 else {
            panic!("r2")
        };
        assert_eq!(o1["hits"], o2["hits"]);
        assert_eq!(o1["hitCount"], o2["hitCount"]);
        // stopword-only -> empty
        let r3 =
            tool.execute(&json!({ "query": "the and a" }), token.signal());
        let ToolExecutionResult::Success { output: o3, .. } = r3 else {
            panic!("r3")
        };
        assert_eq!(o3["hitCount"], 0);
        assert_eq!(o3["hits"].as_array().unwrap().len(), 0);
        // case-insensitive stopword check
        let r4 =
            tool.execute(&json!({ "query": "The AND An" }), token.signal());
        let ToolExecutionResult::Success { output: o4, .. } = r4 else {
            panic!("r4")
        };
        assert_eq!(o4["hitCount"], 0);
    }

    #[test]
    fn whole_word_vs_substring_weights_exact_values() {
        use siralos_core::identity::sha256_hex;
        let nodes = vec![
            ContextNode {
                id: "node-a".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("a".as_bytes()),
                summary: "auth overview".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "node-b".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("b".as_bytes()),
                summary: "authentication details".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        let store = ContextRepresentationStore::build(vec![]).expect("store");
        let entries = vec![
            SchedulerEntry {
                node_id: "node-a".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "a".repeat(64),
            },
            SchedulerEntry {
                node_id: "node-b".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "b".repeat(64),
            },
        ];
        let state = WorkingSetState::build(entries).expect("state");
        let snapshot = ContextToolState::new(graph, store, state, vec![]);
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("failed")
        };
        let hits = output["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 2);
        // Find scores by node_id
        let sa = hits.iter().find(|h| h["node_id"] == "node-a").unwrap();
        let sb = hits.iter().find(|h| h["node_id"] == "node-b").unwrap();
        // node-a: whole-word in summary +10, tier 0, kind 0 => 10
        // node-b: substring in summary +3, tier 0, kind 0 => 3
        assert_eq!(sa["score"], 10);
        assert_eq!(sb["score"], 3);
        // ordering: node-a first (higher score)
        assert_eq!(hits[0]["node_id"], "node-a");
        // id whole-word vs substring: use ids
        let nodes2 = vec![
            ContextNode {
                id: "auth-node".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("c".as_bytes()),
                summary: "nothing".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "authentication-node".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("d".as_bytes()),
                summary: "nothing".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
        ];
        let graph2 = ContextGraph::build(nodes2, vec![]).expect("graph2");
        let store2 =
            ContextRepresentationStore::build(vec![]).expect("store2");
        let entries2 = vec![
            SchedulerEntry {
                node_id: "auth-node".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "c".repeat(64),
            },
            SchedulerEntry {
                node_id: "authentication-node".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "d".repeat(64),
            },
        ];
        let state2 = WorkingSetState::build(entries2).expect("state2");
        let snap2 = ContextToolState::new(graph2, store2, state2, vec![]);
        let tool2 = ContextSearchTool::new(snap2);
        let result2 =
            tool2.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output: o2, .. } = result2 else {
            panic!("failed2")
        };
        let hits2 = o2["hits"].as_array().unwrap();
        let ha = hits2.iter().find(|h| h["node_id"] == "auth-node").unwrap();
        let hb = hits2
            .iter()
            .find(|h| h["node_id"] == "authentication-node")
            .unwrap();
        // auth-node: whole-word in id +4
        // authentication-node: substring in id +1 (auth is substring of authentication)
        assert_eq!(ha["score"], 4);
        assert_eq!(hb["score"], 1);
    }

    #[test]
    fn tier_bonus_hot_vs_warm_vs_cold() {
        use siralos_core::identity::sha256_hex;
        let nodes = vec![
            ContextNode {
                id: "n-hot".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("h".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "n-warm".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("w".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "n-cold".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("c".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        let store = ContextRepresentationStore::build(vec![]).expect("store");
        let entries = vec![
            SchedulerEntry {
                node_id: "n-hot".to_owned(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "h".repeat(64),
            },
            SchedulerEntry {
                node_id: "n-warm".to_owned(),
                tier: WorkingSetTier::Warm,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "w".repeat(64),
            },
            SchedulerEntry {
                node_id: "n-cold".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "c".repeat(64),
            },
        ];
        let state = WorkingSetState::build(entries).expect("state");
        let snapshot = ContextToolState::new(graph, store, state, vec![]);
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("failed")
        };
        let hits = output["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 3);
        // Scores: summary whole-word +10 plus tier
        let hot =
            hits.iter().find(|h| h["node_id"] == "n-hot").unwrap()["score"]
                .as_i64()
                .unwrap();
        let warm =
            hits.iter().find(|h| h["node_id"] == "n-warm").unwrap()["score"]
                .as_i64()
                .unwrap();
        let cold =
            hits.iter().find(|h| h["node_id"] == "n-cold").unwrap()["score"]
                .as_i64()
                .unwrap();
        assert_eq!(hot, 10 + 8);
        assert_eq!(warm, 10 + 4);
        assert_eq!(cold, 10);
        // ordering hot > warm > cold
        assert_eq!(hits[0]["node_id"], "n-hot");
        assert_eq!(hits[1]["node_id"], "n-warm");
        assert_eq!(hits[2]["node_id"], "n-cold");
    }

    #[test]
    fn kind_weight_knowledge_source_others() {
        use siralos_core::identity::sha256_hex;
        let nodes = vec![
            ContextNode {
                id: "k-1".to_owned(),
                kind: ContextNodeKind::Knowledge,
                content_digest: sha256_hex("k".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "s-1".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: sha256_hex("s".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "d-1".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("d".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        let store = ContextRepresentationStore::build(vec![]).expect("store");
        let entries = vec![
            SchedulerEntry {
                node_id: "k-1".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "k".repeat(64),
            },
            SchedulerEntry {
                node_id: "s-1".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "s".repeat(64),
            },
            SchedulerEntry {
                node_id: "d-1".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "d".repeat(64),
            },
        ];
        let state = WorkingSetState::build(entries).expect("state");
        let snapshot = ContextToolState::new(graph, store, state, vec![]);
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("failed")
        };
        let hits = output["hits"].as_array().unwrap();
        let k_score =
            hits.iter().find(|h| h["node_id"] == "k-1").unwrap()["score"]
                .as_i64()
                .unwrap();
        let s_score =
            hits.iter().find(|h| h["node_id"] == "s-1").unwrap()["score"]
                .as_i64()
                .unwrap();
        let d_score =
            hits.iter().find(|h| h["node_id"] == "d-1").unwrap()["score"]
                .as_i64()
                .unwrap();
        // summary whole-word +10 plus kind
        assert_eq!(k_score, 10 + 3);
        assert_eq!(s_score, 10 + 1);
        assert_eq!(d_score, 10);
        // ordering knowledge > source > decision
        assert_eq!(hits[0]["node_id"], "k-1");
        assert_eq!(hits[1]["node_id"], "s-1");
        assert_eq!(hits[2]["node_id"], "d-1");
    }

    #[test]
    fn ordering_score_desc_node_id_asc_tiebreak() {
        use siralos_core::identity::sha256_hex;
        // Two nodes with equal score, different node_ids: tiebreak asc
        let nodes = vec![
            ContextNode {
                id: "b-node".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("b".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "a-node".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("a".as_bytes()),
                summary: "auth".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
            ContextNode {
                id: "c-node".to_owned(),
                kind: ContextNodeKind::Decision,
                content_digest: sha256_hex("c".as_bytes()),
                summary: "authentication".to_owned(),
                source_bindings: vec![],
                token_estimate: 10,
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        let store = ContextRepresentationStore::build(vec![]).expect("store");
        let entries = vec![
            SchedulerEntry {
                node_id: "b-node".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "b".repeat(64),
            },
            SchedulerEntry {
                node_id: "a-node".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "a".repeat(64),
            },
            SchedulerEntry {
                node_id: "c-node".to_owned(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 1,
                last_access_tick: 0,
                token_estimate: 10,
                content_digest: "c".repeat(64),
            },
        ];
        let state = WorkingSetState::build(entries).expect("state");
        let snapshot = ContextToolState::new(graph, store, state, vec![]);
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("failed")
        };
        let hits = output["hits"].as_array().unwrap();
        // a-node and b-node both score 10 (whole-word), c-node scores 3 (substring)
        assert_eq!(hits[0]["node_id"], "a-node");
        assert_eq!(hits[1]["node_id"], "b-node");
        assert_eq!(hits[2]["node_id"], "c-node");
        assert_eq!(hits[0]["score"], 10);
        assert_eq!(hits[1]["score"], 10);
        assert_eq!(hits[2]["score"], 3);
    }

    #[test]
    fn score_in_results_observability() {
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "query": "auth" }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("failed")
        };
        let hits = output["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        for hit in hits {
            assert!(hit.get("score").is_some());
            assert!(hit["score"].is_number());
            assert!(hit["score"].as_i64().unwrap() >= 0);
        }
    }

    #[test]
    fn read_only_scoring_uses_snapshot_only() {
        let snapshot = build_fixture();
        let before_entries = snapshot.state.entries().to_vec();
        let before_tick = snapshot.state.tick();
        let tool = ContextSearchTool::new(snapshot.clone());
        let token = CancellationToken::new();
        let _ = tool.execute(&json!({ "query": "auth the" }), token.signal());
        let _ = tool.execute(&json!({ "query": "decision" }), token.signal());
        assert_eq!(snapshot.state.entries(), before_entries.as_slice());
        assert_eq!(snapshot.state.tick(), before_tick);
        // scores deterministic without mutation
        let r1 =
            tool.execute(&json!({ "query": "knowledge" }), token.signal());
        let r2 =
            tool.execute(&json!({ "query": "knowledge" }), token.signal());
        assert_eq!(r1, r2);
    }

    #[test]
    fn determinism_byte_equal() {
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let r1 = tool.execute(&json!({ "query": "auth" }), token.signal());
        let r2 = tool.execute(&json!({ "query": "auth" }), token.signal());
        assert_eq!(r1, r2);
        // canonical json byte-equal via serde_json canonicalization (BTreeMap ordering)
        let ToolExecutionResult::Success { output: o1, .. } = r1 else {
            panic!("r1")
        };
        let ToolExecutionResult::Success { output: o2, .. } = r2 else {
            panic!("r2")
        };
        let j1 = serde_json::to_string(&o1).expect("json1");
        let j2 = serde_json::to_string(&o2).expect("json2");
        assert_eq!(j1, j2);
    }

    #[test]
    fn inspect_accepts_nested_path_slash() {
        // R1 tripwire: inspect must accept a/b.txt (real workspace slash paths)
        let mut snapshot = build_fixture();
        // inject a slashed node mirroring a scanned nested file a/b.txt
        let slashed_id = "a/b.txt";
        let digest = siralos_core::identity::sha256_hex("slashed".as_bytes());
        let node = ContextNode {
            id: slashed_id.to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest.clone(),
            summary: "nested file".to_owned(),
            source_bindings: vec![],
            token_estimate: 10,
        };
        let mut nodes = snapshot.graph.nodes().to_vec();
        nodes.push(node);
        let graph =
            ContextGraph::build(nodes, snapshot.graph.edges().to_vec())
                .expect("graph");
        snapshot.graph = graph;
        // add representation for the slashed node
        let rep = NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of("{\"id\":\"a/b.txt\"}"),
            derived_from: vec![],
            content: "{\"id\":\"a/b.txt\"}".to_owned(),
        };
        let set =
            NodeRepresentationSet::build(slashed_id.to_owned(), vec![rep])
                .expect("set");
        let mut sets = snapshot.store.sets().to_vec();
        sets.push(set);
        snapshot.store =
            ContextRepresentationStore::build(sets).expect("store");
        let entry = SchedulerEntry {
            node_id: slashed_id.to_owned(),
            tier: WorkingSetTier::Hot,
            pinned: false,
            relevance: 10,
            last_access_tick: 0,
            token_estimate: 10,
            content_digest: digest.clone(),
        };
        let mut entries = snapshot.state.entries().to_vec();
        entries.push(entry);
        snapshot.state = WorkingSetState::build(entries).expect("state");
        snapshot.current_digests.push((slashed_id.to_owned(), digest));
        let tool = ContextInspectTool::new(snapshot);
        let token = CancellationToken::new();
        let result =
            tool.execute(&json!({ "node_id": slashed_id }), token.signal());
        assert!(
            matches!(result, ToolExecutionResult::Success { .. }),
            "inspect with slashed id must succeed, got {result:?}"
        );
    }

    #[test]
    fn inspect_rejects_backslash_and_nul_but_allows_slash() {
        let snapshot = build_fixture();
        let tool = ContextInspectTool::new(snapshot);
        let token = CancellationToken::new();
        // backslash must be rejected (path-traversal hygiene)
        let r_back =
            tool.execute(&json!({ "node_id": "a\\b.txt" }), token.signal());
        assert!(!matches!(r_back, ToolExecutionResult::Success { .. }));
        assert!(
            r_back.message().contains("bounded id"),
            "backslash should be bounded-id, got {}",
            r_back.message()
        );
        // NUL must be rejected
        let r_nul =
            tool.execute(&json!({ "node_id": "a\0b.txt" }), token.signal());
        assert!(!matches!(r_nul, ToolExecutionResult::Success { .. }));
        assert!(
            r_nul.message().contains("bounded id"),
            "NUL should be bounded-id, got {}",
            r_nul.message()
        );
        // slash must be accepted (either success or unknown node, but not bounded-id error)
        let r_slash =
            tool.execute(&json!({ "node_id": "a/b.txt" }), token.signal());
        // a/b.txt not in fixture, so it should be "unknown node", not bounded id
        assert!(!matches!(r_slash, ToolExecutionResult::Success { .. }));
        assert!(
            r_slash.message().contains("unknown node"),
            "slash should not be bounded-id, got {}",
            r_slash.message()
        );
        assert!(
            !r_slash.message().contains("bounded id"),
            "slash must not be bounded-id"
        );
    }

    #[test]
    fn expand_accepts_nested_path_slash() {
        let snapshot = build_fixture();
        let tool = ContextExpandTool::new(snapshot);
        let token = CancellationToken::new();
        // expand with slashed id should not be bounded-id error (unknown node is ok)
        let result = tool.execute(
            &json!({ "node_id": "a/b.txt", "level": "summary" }),
            token.signal(),
        );
        // expect Failed unknown node, not bounded id
        assert!(!matches!(result, ToolExecutionResult::Success { .. }));
        assert!(
            !result.message().contains("bounded id"),
            "expand should allow slash, got {}",
            result.message()
        );
        assert!(
            result.message().contains("unknown node")
                || result.message().contains("not found"),
            "expand slash unknown node, got {}",
            result.message()
        );
        // backslash must be rejected
        let r_back = tool.execute(
            &json!({ "node_id": "a\\b.txt", "level": "summary" }),
            token.signal(),
        );
        assert!(!matches!(r_back, ToolExecutionResult::Success { .. }));
        assert!(
            r_back.message().contains("bounded id"),
            "backslash should be bounded-id, got {}",
            r_back.message()
        );
    }

    #[test]
    fn search_tripwire_with_slash_query() {
        // R1 tripwire for search: slash in query is allowed (not bounded id)
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot);
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "query": "a/b" }), token.signal());
        // should be Success (maybe 0 hits) not Failed bounded id
        assert!(matches!(result, ToolExecutionResult::Success { .. }));
    }

    #[test]
    fn inspect_slashed_id_end_to_end_via_nested_fixture() {
        // End-to-end: inject a slashed node as a scanned file would, inspect succeeds
        let mut snapshot = build_fixture();
        let slashed_id = "nested/a/b.txt";
        let digest = siralos_core::identity::sha256_hex("nested".as_bytes());
        let node = ContextNode {
            id: slashed_id.to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest.clone(),
            summary: "nested deep".to_owned(),
            source_bindings: vec![],
            token_estimate: 10,
        };
        let mut nodes = snapshot.graph.nodes().to_vec();
        nodes.push(node);
        let graph =
            ContextGraph::build(nodes, snapshot.graph.edges().to_vec())
                .expect("graph");
        snapshot.graph = graph;
        let rep = NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of("{\"id\":\"nested/a/b.txt\"}"),
            derived_from: vec![],
            content: "{\"id\":\"nested/a/b.txt\"}".to_owned(),
        };
        let set =
            NodeRepresentationSet::build(slashed_id.to_owned(), vec![rep])
                .expect("set");
        let mut sets = snapshot.store.sets().to_vec();
        sets.push(set);
        snapshot.store =
            ContextRepresentationStore::build(sets).expect("store");
        let entry = SchedulerEntry {
            node_id: slashed_id.to_owned(),
            tier: WorkingSetTier::Hot,
            pinned: false,
            relevance: 20,
            last_access_tick: 1,
            token_estimate: 10,
            content_digest: digest.clone(),
        };
        let mut entries = snapshot.state.entries().to_vec();
        entries.push(entry);
        snapshot.state = WorkingSetState::build(entries).expect("state");
        snapshot.current_digests.push((slashed_id.to_owned(), digest));
        let tool = ContextInspectTool::new(snapshot.clone());
        let token = CancellationToken::new();
        let result =
            tool.execute(&json!({ "node_id": slashed_id }), token.signal());
        let ToolExecutionResult::Success { output, .. } = result else {
            panic!("slashed inspect end-to-end failed");
        };
        assert_eq!(output["id"], slashed_id);
        // also verify demand tripwire: second tool sees same node via search
        let search = ContextSearchTool::new(snapshot);
        let r2 = search.execute(&json!({ "query": "nested" }), token.signal());
        assert!(matches!(r2, ToolExecutionResult::Success { .. }));
    }

    #[test]
    fn key_blindness_search_does_not_depend_on_external_key() {
        // Changing a dummy answer key must not change search hits or paged tokens.
        // The search tool is key-blind by construction (reads only snapshot).
        let snapshot = build_fixture();
        let tool = ContextSearchTool::new(snapshot.clone());
        let token = CancellationToken::new();
        let hits_a =
            match tool.execute(&json!({ "query": "auth" }), token.signal()) {
                ToolExecutionResult::Success { output, .. } => {
                    output["hits"].clone()
                }
                other => panic!("unexpected {other:?}"),
            };
        // Pretend key changed: snapshot is identical, hits must be identical
        let tool2 = ContextSearchTool::new(snapshot);
        let hits_b =
            match tool2.execute(&json!({ "query": "auth" }), token.signal()) {
                ToolExecutionResult::Success { output, .. } => {
                    output["hits"].clone()
                }
                other => panic!("unexpected {other:?}"),
            };
        assert_eq!(hits_a, hits_b);
        // Also verify benchmark flow is key-blind: changing answer_key doesn't change paged tokens
        use crate::tool::context_benchmark::{
            PagingStrategy, gold_set_v3, run_strategy,
        };
        let scenarios = gold_set_v3().expect("gold");
        let sc = scenarios
            .iter()
            .find(|s| s.name == "narrow-alpha")
            .expect("na")
            .clone();
        let report_a = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ProgressiveV2,
        )
        .expect("run a");
        let tokens_a = report_a.scenarios[0].tokens_paged;
        let mut sc2 = sc.clone();
        sc2.answer_key = vec!["na-03".to_owned()];
        let report_b = run_strategy(
            std::slice::from_ref(&sc2),
            PagingStrategy::ProgressiveV2,
        )
        .expect("run b");
        assert_eq!(tokens_a, report_b.scenarios[0].tokens_paged);
    }
}
