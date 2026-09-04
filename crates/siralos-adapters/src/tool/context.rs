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
        let lower_query = query.to_lowercase();
        let mut hits: Vec<Value> = Vec::new();
        for node in self.snapshot.graph.nodes() {
            if cancellation.is_cancelled() {
                return ToolExecutionResult::Cancelled {
                    message: "Search was cancelled.".to_owned(),
                };
            }
            let lower_id = node.id.to_lowercase();
            let matched_in = if lower_id.contains(&lower_query) {
                "id"
            } else if summary_contains_whole_wordish(
                &node.summary,
                &lower_query,
            ) {
                "summary"
            } else {
                continue;
            };
            let entry = self
                .snapshot
                .state
                .entries()
                .iter()
                .find(|e| e.node_id == node.id);
            let tier_str = entry
                .map(|e| e.tier.as_str().to_owned())
                .unwrap_or_else(|| "cold".to_owned());
            hits.push(json!({
                "node_id": node.id,
                "kind": node_kind_str(node.kind),
                "tier": tier_str,
                "matched_in": matched_in,
            }));
        }
        hits.sort_by(|a, b| {
            a["node_id"]
                .as_str()
                .unwrap_or("")
                .cmp(b["node_id"].as_str().unwrap_or(""))
        });
        let hit_count = hits.len();
        let truncated = hit_count > MAX_RESULTS;
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

fn summary_contains_whole_wordish(summary: &str, lower_query: &str) -> bool {
    if summary.is_empty() {
        return false;
    }
    let lower_summary = summary.to_lowercase();
    // Whole-word-ish: query appears inside a token split by non-alphanumeric.
    // Fallback to simple substring for tight coupling to "auth" within "auth overview".
    if lower_summary.contains(lower_query) {
        // Check token containment for whole-word-ish sense.
        for token in lower_summary.split(|c: char| !c.is_alphanumeric()) {
            if token.is_empty() {
                continue;
            }
            if token.contains(lower_query) {
                return true;
            }
        }
        // If substring exists but token check missed (e.g., multi-word query),
        // still consider it a match via substring.
        return true;
    }
    false
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
            if value.contains('/')
                || value.contains('\\')
                || value.contains('\0')
            {
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
        // hits on id: query "ctx-" should hit all in canonical order.
        let result2 =
            tool.execute(&json!({ "query": "ctx-" }), token.signal());
        let ToolExecutionResult::Success { output: out2, .. } = result2 else {
            panic!("search failed");
        };
        let hits2 = out2["hits"].as_array().unwrap();
        assert_eq!(hits2.len(), 3);
        assert_eq!(hits2[0]["node_id"], "ctx-a");
        assert_eq!(hits2[1]["node_id"], "ctx-b");
        assert_eq!(hits2[2]["node_id"], "ctx-knowledge");
        assert!(hits2.iter().all(|h| h["matched_in"] == "id"));
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
}
