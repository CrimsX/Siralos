//! Benchmark decision gate — slice 5 (decision 79 clause g).
//!
//! Measures whether demand-paging over the graph/store/scheduler surfaces
//! answer-key content at materially lower token cost than dump-everything.
//! The verdict is computed, not asserted, and includes a broad-query
//! scenario where paging cannot win (anti-cherry-pick).

use siralos_core::context_graph::{
    ContextGraph, ContextNode, ContextNodeKind,
};
use siralos_core::context_representation::{
    ContextRepresentationStore, NodeRepresentation, NodeRepresentationSet,
    RepresentationLevel, RepresentationOrigin, available_levels,
    content_digest_of, resolve_representation,
};
use siralos_core::context_scheduler::{
    SchedulerEntry, WorkingSetState, WorkingSetTier,
};

use super::context::{ContextSearchTool, ContextToolState};
use serde_json::json;
use siralos_core::provider::CancellationToken;
use siralos_core::tool::Tool;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Bytes per token — ceil division.
///
/// The only token estimator in the gate; every number below flows through it.
pub const TOKEN_BYTES_PER_TOKEN: usize = 4;
/// Overhead tokens per tool call.
pub const TOOL_CALL_OVERHEAD_TOKENS: usize = 4;

/// Ceil division by [`TOKEN_BYTES_PER_TOKEN`].
///
/// The only token estimator in the gate; every number below flows through it.
#[must_use]
pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(TOKEN_BYTES_PER_TOKEN)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Validation failures for benchmark construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkError {
    /// Key node does not exist in the graph.
    UnknownKeyNode {
        /// Scenario name.
        scenario: String,
        /// Missing node id.
        node_id: String,
    },
    /// Key node lacks an L0 (Identity) representation.
    KeyMissingL0 {
        /// Scenario name.
        scenario: String,
        /// Node id missing L0.
        node_id: String,
    },
    /// Answer key is empty.
    EmptyKey {
        /// Scenario name.
        scenario: String,
    },
    /// Duplicate node id in answer key.
    DuplicateKey {
        /// Scenario name.
        scenario: String,
        /// Duplicate node id.
        node_id: String,
    },
}

impl std::fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKeyNode { scenario, node_id } => {
                write!(f, "unknown key node {node_id} in scenario {scenario}")
            }
            Self::KeyMissingL0 { scenario, node_id } => {
                write!(
                    f,
                    "key node {node_id} missing L0 in scenario {scenario}"
                )
            }
            Self::EmptyKey { scenario } => {
                write!(f, "empty key in scenario {scenario}")
            }
            Self::DuplicateKey { scenario, node_id } => {
                write!(
                    f,
                    "duplicate key node {node_id} in scenario {scenario}"
                )
            }
        }
    }
}

impl std::error::Error for BenchmarkError {}

// ---------------------------------------------------------------------------
// Scenario
// ---------------------------------------------------------------------------

/// Immutable benchmark scenario snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkScenario {
    /// Scenario name, canonical.
    pub name: String,
    /// Immutable snapshot over graph/store/scheduler.
    pub state: ContextToolState,
    /// Query string to search.
    pub query: String,
    /// Answer-key node ids.
    pub answer_key: Vec<String>,
}

impl BenchmarkScenario {
    /// Validate and build a scenario.
    ///
    /// Checks: key non-empty, unique, every key exists in graph, every key
    /// has an L0 (Identity) representation.
    pub fn build(
        name: String,
        state: ContextToolState,
        query: String,
        answer_key: Vec<String>,
    ) -> Result<Self, BenchmarkError> {
        if answer_key.is_empty() {
            return Err(BenchmarkError::EmptyKey { scenario: name.clone() });
        }
        let mut seen = std::collections::BTreeSet::new();
        for node_id in &answer_key {
            if !seen.insert(node_id.clone()) {
                return Err(BenchmarkError::DuplicateKey {
                    scenario: name.clone(),
                    node_id: node_id.clone(),
                });
            }
            if state.graph.node(node_id).is_none() {
                return Err(BenchmarkError::UnknownKeyNode {
                    scenario: name.clone(),
                    node_id: node_id.clone(),
                });
            }
            // Require L0.
            let has_l0 = state.store.set(node_id).is_some_and(|set| {
                available_levels(set).contains(&RepresentationLevel::Identity)
            });
            if !has_l0 {
                return Err(BenchmarkError::KeyMissingL0 {
                    scenario: name.clone(),
                    node_id: node_id.clone(),
                });
            }
        }
        Ok(Self { name, state, query, answer_key })
    }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Per-scenario integer metrics, no floats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioMetrics {
    /// Scenario name.
    pub name: String,
    /// Baseline tokens (sum of L0 estimates).
    pub tokens_baseline: usize,
    /// Paged tokens (estimate of expanded+inspect + overhead).
    pub tokens_paged: usize,
    /// |key|
    pub key_size: usize,
    /// Baseline recall (= key size).
    pub recall_baseline: usize,
    /// Paged recall (key nodes hit and expanded).
    pub recall_paged: usize,
    /// Tool calls (1 search + hits inspects + expands).
    pub tool_calls: usize,
}

/// Aggregated report with deterministic decision rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkReport {
    /// Per-scenario metrics, canonical by name.
    pub scenarios: Vec<ScenarioMetrics>,
    /// Sum of baseline tokens.
    pub total_baseline: usize,
    /// Sum of paged tokens.
    pub total_paged: usize,
    /// Sum of key sizes.
    pub total_key: usize,
    /// Sum of baseline recalls.
    pub total_recall_baseline: usize,
    /// Sum of paged recalls.
    pub total_recall_paged: usize,
    /// Sum of tool calls.
    pub total_tool_calls: usize,
    /// Deterministic GO verdict.
    pub go: bool,
    /// Mechanical reason citing the two compared numbers.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Core benchmark
// ---------------------------------------------------------------------------

/// Baseline strategy (documented): surface every node's L0 summary content
/// -> `tokens_baseline = sum of estimate(L0 content bytes) over all nodes`.
/// `recall_baseline = |key|` (every key node's L0 is surfaced) — but ONLY
/// if every key node HAS an L0; `build()` requires it.
/// Paged strategy — KEY-BLIND, deterministic, realistic tool flow:
/// (1) search {query} -> hits (canonical order, cap 16);
/// (2) inspect every hit -> metadata (its summary content counts toward tokens_paged);
/// (3) expand every hit at its best available level with priority
///     structured > detailed > summary > identity
///     (a hit with NO levels contributes only its inspect summary);
/// `tokens_paged = estimate(sum of all expanded content bytes + all inspect
///  summary bytes) + TOOL_CALL_OVERHEAD_TOKENS * tool_calls`
/// where `tool_calls = 1 search + hits inspects + expands`.
/// `recall_paged = |key nodes that appear in the search hits AND got expanded|`.
/// The flow never reads the answer key; it surfaces what the query finds.
fn best_level_for(set: &NodeRepresentationSet) -> Option<RepresentationLevel> {
    let levels = available_levels(set);
    [
        RepresentationLevel::Structured,
        RepresentationLevel::Detailed,
        RepresentationLevel::Summary,
        RepresentationLevel::Identity,
    ]
    .into_iter()
    .find(|lvl| levels.contains(lvl))
}

/// Run all scenarios, aggregate and apply deterministic decision rule:
///
/// `go = (total_recall_paged == total_recall_baseline) && (total_paged *2 < total_baseline)`
pub fn run_benchmark(
    scenarios: &[BenchmarkScenario],
) -> Result<BenchmarkReport, BenchmarkError> {
    // Validate that scenarios are build-valid? Already built, but ensure no mutation.
    // Compute per-scenario metrics.
    let mut metrics: Vec<ScenarioMetrics> = Vec::new();
    let mut total_baseline = 0usize;
    let mut total_paged = 0usize;
    let mut total_key = 0usize;
    let mut total_recall_baseline = 0usize;
    let mut total_recall_paged = 0usize;
    let mut total_tool_calls = 0usize;

    // Capture state before to ensure no mutation later checked externally; here we just compute.
    for sc in scenarios {
        // Baseline
        let mut baseline_bytes_sum = 0usize;
        let mut baseline_tokens = 0usize;
        for node in sc.state.graph.nodes() {
            if let Some(set) = sc.state.store.set(&node.id) {
                if let Some(rep) =
                    resolve_representation(set, RepresentationLevel::Identity)
                {
                    baseline_bytes_sum =
                        baseline_bytes_sum.saturating_add(rep.content.len());
                    // Per spec: sum of estimate per node
                    baseline_tokens = baseline_tokens
                        .saturating_add(estimate_tokens(rep.content.len()));
                }
            }
        }
        // Alternative: baseline_tokens as sum per node ceil, already done.
        // paged strategy key-blind
        let search_tool = ContextSearchTool::new(sc.state.clone());
        let token = CancellationToken::new();
        let search_result =
            search_tool.execute(&json!({"query": sc.query}), token.signal());
        let hits: Vec<String> = match search_result {
            siralos_core::provider::ToolExecutionResult::Success {
                output,
                ..
            } => output
                .get("hits")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|h| {
                            h.get("node_id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_owned())
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        // Inspect + expand
        let mut sum_inspect_summary_bytes = 0usize;
        let mut sum_expanded_bytes = 0usize;
        let mut expanded_count = 0usize;
        for hit_id in &hits {
            // Inspect summary bytes: node.summary
            if let Some(node) = sc.state.graph.node(hit_id) {
                sum_inspect_summary_bytes = sum_inspect_summary_bytes
                    .saturating_add(node.summary.len());
            } else if let Some(set) = sc.state.store.set(hit_id) {
                // Fallback? shouldn't happen
                let _ = set;
            }
            // Expand best level
            if let Some(set) = sc.state.store.set(hit_id) {
                if let Some(best) = best_level_for(set) {
                    if let Some(rep) = resolve_representation(set, best) {
                        sum_expanded_bytes = sum_expanded_bytes
                            .saturating_add(rep.content.len());
                        expanded_count += 1;
                    }
                }
            }
        }
        let tool_calls =
            1usize.saturating_add(hits.len()).saturating_add(expanded_count);
        let paged_tokens = estimate_tokens(
            sum_expanded_bytes.saturating_add(sum_inspect_summary_bytes),
        )
        .saturating_add(TOOL_CALL_OVERHEAD_TOKENS * tool_calls);

        let key_size = sc.answer_key.len();
        let recall_baseline = key_size;
        // recall_paged = |key nodes that appear in hits AND got expanded|
        let hits_set: std::collections::BTreeSet<&str> =
            hits.iter().map(|s| s.as_str()).collect();
        let mut recall_paged = 0usize;
        for key_id in &sc.answer_key {
            if hits_set.contains(key_id.as_str()) {
                // Check expanded: node has at least one level
                if let Some(set) = sc.state.store.set(key_id) {
                    if best_level_for(set).is_some() {
                        recall_paged += 1;
                    }
                }
            }
        }

        total_baseline = total_baseline.saturating_add(baseline_tokens);
        total_paged = total_paged.saturating_add(paged_tokens);
        total_key = total_key.saturating_add(key_size);
        total_recall_baseline =
            total_recall_baseline.saturating_add(recall_baseline);
        total_recall_paged = total_recall_paged.saturating_add(recall_paged);
        total_tool_calls = total_tool_calls.saturating_add(tool_calls);

        metrics.push(ScenarioMetrics {
            name: sc.name.clone(),
            tokens_baseline: baseline_tokens,
            tokens_paged: paged_tokens,
            key_size,
            recall_baseline,
            recall_paged,
            tool_calls,
        });
        // Avoid unused variable baseline_bytes_sum (kept for clarity)
        let _ = baseline_bytes_sum;
    }

    metrics.sort_by(|a, b| a.name.cmp(&b.name));

    let go = total_recall_paged == total_recall_baseline
        && total_paged.saturating_mul(2) < total_baseline;
    let recall_eq = total_recall_paged == total_recall_baseline;
    let token_win = total_paged.saturating_mul(2) < total_baseline;
    let reason = format!(
        "total_recall_paged {} == total_recall_baseline {} is {}, total_paged {} *2 < total_baseline {} is {} => {}",
        total_recall_paged,
        total_recall_baseline,
        recall_eq,
        total_paged,
        total_baseline,
        token_win,
        if go { "GO" } else { "NO-GO" }
    );

    Ok(BenchmarkReport {
        scenarios: metrics,
        total_baseline,
        total_paged,
        total_key,
        total_recall_baseline,
        total_recall_paged,
        total_tool_calls,
        go,
        reason,
    })
}

// ---------------------------------------------------------------------------
// Gold set — hermetic, no fs
// ---------------------------------------------------------------------------

fn digest(ch: char) -> String {
    std::iter::repeat_n(ch, 64).collect()
}

fn make_node(id: &str, summary: &str, _digest_ch: char) -> ContextNode {
    ContextNode {
        id: id.to_owned(),
        kind: ContextNodeKind::Source,
        content_digest: digest('a'),
        summary: summary.to_owned(),
        source_bindings: vec![],
        token_estimate: 0,
    }
}

fn rep_identity(content: &str) -> NodeRepresentation {
    NodeRepresentation {
        level: RepresentationLevel::Identity,
        origin: RepresentationOrigin::HostExtracted,
        content_digest: content_digest_of(content),
        derived_from: vec![],
        content: content.to_owned(),
    }
}

fn rep_structured(content: &str) -> NodeRepresentation {
    NodeRepresentation {
        level: RepresentationLevel::Structured,
        origin: RepresentationOrigin::HostExtracted,
        content_digest: content_digest_of(content),
        derived_from: vec![],
        content: content.to_owned(),
    }
}

fn rep_detailed(content: &str) -> NodeRepresentation {
    NodeRepresentation {
        level: RepresentationLevel::Detailed,
        origin: RepresentationOrigin::HostExtracted,
        content_digest: content_digest_of(content),
        derived_from: vec![],
        content: content.to_owned(),
    }
}

fn rep_summary_model(
    content: &str,
    derived_from: Vec<(String, String)>,
) -> NodeRepresentation {
    NodeRepresentation {
        level: RepresentationLevel::Summary,
        origin: RepresentationOrigin::ModelDerived,
        content_digest: content_digest_of(content),
        derived_from,
        content: content.to_owned(),
    }
}

fn build_state(
    nodes: Vec<ContextNode>,
    sets: Vec<NodeRepresentationSet>,
) -> ContextToolState {
    let graph = ContextGraph::build(nodes, vec![]).expect("graph");
    let store = ContextRepresentationStore::build(sets).expect("store");
    let entries: Vec<SchedulerEntry> = graph
        .nodes()
        .iter()
        .map(|n| SchedulerEntry {
            node_id: n.id.clone(),
            tier: WorkingSetTier::Cold,
            pinned: false,
            relevance: 10,
            last_access_tick: 0,
            token_estimate: 0,
        })
        .collect();
    let state = WorkingSetState::build(entries).expect("state");
    ContextToolState::new(graph, store, state, vec![])
}

/// Gold set: 6 scenarios over hand-built graphs/stores/states.
/// Four narrow (query matches 2 of 6-8; key 1-2), one medium (3 of 8; key 3),
/// one broad (5 of 6; key 5). Each key has L0; at least one L2 and one
/// only-L1-model-derived with provenance. Names narrow-alpha..delta, medium-echo, broad-foxtrot.
pub fn gold_set() -> Result<Vec<BenchmarkScenario>, BenchmarkError> {
    // Content helpers: exact byte lengths.
    let l0_content = "I".repeat(120); // 120 bytes => 30 tokens
    let structured_content = "S".repeat(80); // 80 => 20
    let detailed_content = "D".repeat(96); // 96 => 24
    let summary_model_content = "M".repeat(72); // 72 => 18
    // Summaries containing query keywords: pad to 32 bytes for hits, non-hits 20 bytes.
    // Ensure hit summary length = 32, non-hit = 20.
    let hit_summary = |keyword: &str| -> String {
        // "keyword pad ..." ensure 32 bytes, include keyword
        let base = format!("{keyword} hit summary pad");
        if base.len() >= 32 {
            base[..32].to_owned()
        } else {
            format!("{base}{}", "x".repeat(32 - base.len()))
        }
    };
    let miss_summary = "unrelated summary pad".to_owned(); // 22 bytes approx, we'll pad to 20
    let miss_summary_padded = {
        let s = miss_summary;
        if s.len() > 20 {
            s[..20].to_owned()
        } else {
            format!("{s}{}", "y".repeat(20 - s.len()))
        }
    };

    let mut scenarios: Vec<BenchmarkScenario> = Vec::new();

    // Helper to create representation set for a node id with choices.
    // For key nodes with L2: Identity + Structured
    // For key nodes with only L1 model: Identity + Summary model
    // For other nodes: Identity only (or Identity+Detailed to vary)
    let mk_set_l2 = |node_id: &str| -> NodeRepresentationSet {
        NodeRepresentationSet::build(
            node_id.to_owned(),
            vec![
                rep_identity(&l0_content),
                rep_structured(&structured_content),
            ],
        )
        .expect("set l2")
    };
    let mk_set_l1_model = |node_id: &str| -> NodeRepresentationSet {
        let derived =
            vec![(node_id.to_owned(), content_digest_of(&l0_content))];
        NodeRepresentationSet::build(
            node_id.to_owned(),
            vec![
                rep_identity(&l0_content),
                rep_summary_model(&summary_model_content, derived),
            ],
        )
        .expect("set l1 model")
    };
    let mk_set_identity_only = |node_id: &str| -> NodeRepresentationSet {
        NodeRepresentationSet::build(
            node_id.to_owned(),
            vec![rep_identity(&l0_content)],
        )
        .expect("set identity")
    };
    let mk_set_detailed = |node_id: &str| -> NodeRepresentationSet {
        NodeRepresentationSet::build(
            node_id.to_owned(),
            vec![rep_identity(&l0_content), rep_detailed(&detailed_content)],
        )
        .expect("set detailed")
    };

    // narrow-alpha: 6 nodes, query "alpha", hits 2, key 2
    {
        let q = "alpha";
        let hit1 = "na-01";
        let hit2 = "na-02";
        let nodes = vec![
            make_node(hit1, &hit_summary(q), 'a'),
            make_node(hit2, &hit_summary(q), 'b'),
            make_node("na-03", &miss_summary_padded, 'c'),
            make_node("na-04", &miss_summary_padded, 'd'),
            make_node("na-05", &miss_summary_padded, 'e'),
            make_node("na-06", &miss_summary_padded, 'f'),
        ];
        let sets = vec![
            mk_set_l2(hit1),
            mk_set_l1_model(hit2),
            mk_set_identity_only("na-03"),
            mk_set_identity_only("na-04"),
            mk_set_detailed("na-05"),
            mk_set_identity_only("na-06"),
        ];
        let state = build_state(nodes, sets);
        let sc = BenchmarkScenario::build(
            "narrow-alpha".to_owned(),
            state,
            q.to_owned(),
            vec![hit1.to_owned(), hit2.to_owned()],
        )?;
        scenarios.push(sc);
    }
    // narrow-beta: 6 nodes, query "beta", hits 2, key 1
    {
        let q = "beta";
        let hit1 = "nb-01";
        let hit2 = "nb-02";
        let nodes = vec![
            make_node(hit1, &hit_summary(q), 'a'),
            make_node(hit2, &hit_summary(q), 'b'),
            make_node("nb-03", &miss_summary_padded, 'c'),
            make_node("nb-04", &miss_summary_padded, 'd'),
            make_node("nb-05", &miss_summary_padded, 'e'),
            make_node("nb-06", &miss_summary_padded, 'f'),
        ];
        let sets = vec![
            mk_set_identity_only(hit1),
            mk_set_l2(hit2),
            mk_set_identity_only("nb-03"),
            mk_set_identity_only("nb-04"),
            mk_set_identity_only("nb-05"),
            mk_set_identity_only("nb-06"),
        ];
        let state = build_state(nodes, sets);
        let sc = BenchmarkScenario::build(
            "narrow-beta".to_owned(),
            state,
            q.to_owned(),
            vec![hit2.to_owned()],
        )?;
        scenarios.push(sc);
    }
    // narrow-gamma: 7 nodes, query "gamma", hits 2, key 2
    {
        let q = "gamma";
        let hit1 = "ng-01";
        let hit2 = "ng-02";
        let nodes = vec![
            make_node(hit1, &hit_summary(q), 'a'),
            make_node(hit2, &hit_summary(q), 'b'),
            make_node("ng-03", &miss_summary_padded, 'c'),
            make_node("ng-04", &miss_summary_padded, 'd'),
            make_node("ng-05", &miss_summary_padded, 'e'),
            make_node("ng-06", &miss_summary_padded, 'f'),
            make_node("ng-07", &miss_summary_padded, 'g'),
        ];
        let sets = vec![
            mk_set_l1_model(hit1),
            mk_set_l2(hit2),
            mk_set_identity_only("ng-03"),
            mk_set_identity_only("ng-04"),
            mk_set_identity_only("ng-05"),
            mk_set_detailed("ng-06"),
            mk_set_identity_only("ng-07"),
        ];
        let state = build_state(nodes, sets);
        let sc = BenchmarkScenario::build(
            "narrow-gamma".to_owned(),
            state,
            q.to_owned(),
            vec![hit1.to_owned(), hit2.to_owned()],
        )?;
        scenarios.push(sc);
    }
    // narrow-delta: 8 nodes, query "delta", hits 2, key 1
    {
        let q = "delta";
        let hit1 = "nd-01";
        let hit2 = "nd-02";
        let nodes = vec![
            make_node(hit1, &hit_summary(q), 'a'),
            make_node(hit2, &hit_summary(q), 'b'),
            make_node("nd-03", &miss_summary_padded, 'c'),
            make_node("nd-04", &miss_summary_padded, 'd'),
            make_node("nd-05", &miss_summary_padded, 'e'),
            make_node("nd-06", &miss_summary_padded, 'f'),
            make_node("nd-07", &miss_summary_padded, 'g'),
            make_node("nd-08", &miss_summary_padded, 'h'),
        ];
        let sets = vec![
            mk_set_identity_only(hit1),
            mk_set_identity_only(hit2),
            mk_set_identity_only("nd-03"),
            mk_set_l2("nd-04"), // non-hit with L2 but not counted
            mk_set_identity_only("nd-05"),
            mk_set_identity_only("nd-06"),
            mk_set_identity_only("nd-07"),
            mk_set_identity_only("nd-08"),
        ];
        let state = build_state(nodes, sets);
        let sc = BenchmarkScenario::build(
            "narrow-delta".to_owned(),
            state,
            q.to_owned(),
            vec![hit1.to_owned()],
        )?;
        scenarios.push(sc);
    }
    // medium-echo: 8 nodes, query "echo", hits 3, key 3
    {
        let q = "echo";
        let hit1 = "me-01";
        let hit2 = "me-02";
        let hit3 = "me-03";
        let nodes = vec![
            make_node(hit1, &hit_summary(q), 'a'),
            make_node(hit2, &hit_summary(q), 'b'),
            make_node(hit3, &hit_summary(q), 'c'),
            make_node("me-04", &miss_summary_padded, 'd'),
            make_node("me-05", &miss_summary_padded, 'e'),
            make_node("me-06", &miss_summary_padded, 'f'),
            make_node("me-07", &miss_summary_padded, 'g'),
            make_node("me-08", &miss_summary_padded, 'h'),
        ];
        let sets = vec![
            mk_set_l2(hit1),
            mk_set_l1_model(hit2),
            mk_set_detailed(hit3),
            mk_set_identity_only("me-04"),
            mk_set_identity_only("me-05"),
            mk_set_identity_only("me-06"),
            mk_set_identity_only("me-07"),
            mk_set_identity_only("me-08"),
        ];
        let state = build_state(nodes, sets);
        let sc = BenchmarkScenario::build(
            "medium-echo".to_owned(),
            state,
            q.to_owned(),
            vec![hit1.to_owned(), hit2.to_owned(), hit3.to_owned()],
        )?;
        scenarios.push(sc);
    }
    // broad-foxtrot: 6 nodes, query "foxtrot", hits 5 of 6, key 5
    {
        let q = "foxtrot";
        let hit1 = "bf-01";
        let hit2 = "bf-02";
        let hit3 = "bf-03";
        let hit4 = "bf-04";
        let hit5 = "bf-05";
        let miss = "bf-06";
        let nodes = vec![
            make_node(hit1, &hit_summary(q), 'a'),
            make_node(hit2, &hit_summary(q), 'b'),
            make_node(hit3, &hit_summary(q), 'c'),
            make_node(hit4, &hit_summary(q), 'd'),
            make_node(hit5, &hit_summary(q), 'e'),
            make_node(miss, &miss_summary_padded, 'f'),
        ];
        let sets = vec![
            mk_set_l2(hit1),
            mk_set_l1_model(hit2),
            mk_set_detailed(hit3),
            mk_set_identity_only(hit4),
            mk_set_identity_only(hit5),
            mk_set_identity_only(miss),
        ];
        let state = build_state(nodes, sets);
        let sc = BenchmarkScenario::build(
            "broad-foxtrot".to_owned(),
            state,
            q.to_owned(),
            vec![
                hit1.to_owned(),
                hit2.to_owned(),
                hit3.to_owned(),
                hit4.to_owned(),
                hit5.to_owned(),
            ],
        )?;
        scenarios.push(sc);
    }

    Ok(scenarios)
}

// ---------------------------------------------------------------------------
// Tests (~10)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use siralos_core::context_graph::{
        ContextGraph, ContextNode, ContextNodeKind,
    };
    use siralos_core::context_representation::{
        ContextRepresentationStore, NodeRepresentationSet,
        RepresentationLevel, RepresentationOrigin, content_digest_of,
    };
    use siralos_core::context_scheduler::{
        SchedulerEntry, WorkingSetState, WorkingSetTier,
    };

    fn digest(ch: char) -> String {
        std::iter::repeat_n(ch, 64).collect()
    }

    fn minimal_state_with_nodes(
        nodes: Vec<ContextNode>,
        sets: Vec<NodeRepresentationSet>,
    ) -> ContextToolState {
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        let store = ContextRepresentationStore::build(sets).expect("store");
        let entries: Vec<SchedulerEntry> = graph
            .nodes()
            .iter()
            .map(|n| SchedulerEntry {
                node_id: n.id.clone(),
                tier: WorkingSetTier::Cold,
                pinned: false,
                relevance: 0,
                last_access_tick: 0,
                token_estimate: 0,
            })
            .collect();
        let state = WorkingSetState::build(entries).expect("state");
        ContextToolState::new(graph, store, state, vec![])
    }

    #[test]
    fn estimator_math_ceil_and_zero() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
        assert_eq!(estimate_tokens(8), 2);
        assert_eq!(estimate_tokens(9), 3);
    }

    #[test]
    fn build_refusals() {
        let nodes = vec![ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: "alpha".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        }];
        let l0 = NodeRepresentation {
            level: RepresentationLevel::Identity,
            origin: RepresentationOrigin::HostExtracted,
            content_digest: content_digest_of("hello"),
            derived_from: vec![],
            content: "hello".to_owned(),
        };
        let set = NodeRepresentationSet::build("a".to_owned(), vec![l0])
            .expect("set");
        let state = minimal_state_with_nodes(nodes, vec![set]);

        // empty key
        assert_eq!(
            BenchmarkScenario::build(
                "s".to_owned(),
                state.clone(),
                "q".to_owned(),
                vec![]
            ),
            Err(BenchmarkError::EmptyKey { scenario: "s".to_owned() })
        );
        // unknown key
        assert_eq!(
            BenchmarkScenario::build(
                "s".to_owned(),
                state.clone(),
                "q".to_owned(),
                vec!["missing".to_owned()]
            ),
            Err(BenchmarkError::UnknownKeyNode {
                scenario: "s".to_owned(),
                node_id: "missing".to_owned()
            })
        );
        // duplicate
        assert_eq!(
            BenchmarkScenario::build(
                "s".to_owned(),
                state.clone(),
                "q".to_owned(),
                vec!["a".to_owned(), "a".to_owned()]
            ),
            Err(BenchmarkError::DuplicateKey {
                scenario: "s".to_owned(),
                node_id: "a".to_owned()
            })
        );
        // missing L0: node without representation
        let nodes2 = vec![ContextNode {
            id: "b".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('b'),
            summary: "beta".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        }];
        let state2 = minimal_state_with_nodes(nodes2, vec![]);
        assert_eq!(
            BenchmarkScenario::build(
                "s".to_owned(),
                state2,
                "q".to_owned(),
                vec!["b".to_owned()]
            ),
            Err(BenchmarkError::KeyMissingL0 {
                scenario: "s".to_owned(),
                node_id: "b".to_owned()
            })
        );
    }

    #[test]
    fn recall_baseline_equals_key_size() {
        let scenarios = gold_set().expect("gold");
        for sc in &scenarios {
            let report = run_benchmark(std::slice::from_ref(sc)).expect("run");
            assert_eq!(
                report.scenarios[0].recall_baseline,
                report.scenarios[0].key_size
            );
            assert_eq!(
                report.scenarios[0].recall_baseline,
                sc.answer_key.len()
            );
        }
    }

    #[test]
    fn paged_flow_is_key_blind() {
        let scenarios = gold_set().expect("gold");
        let sc = scenarios
            .iter()
            .find(|s| s.name == "narrow-alpha")
            .expect("alpha")
            .clone();
        let report_a =
            run_benchmark(std::slice::from_ref(&sc)).expect("run a");
        let tokens_a = report_a.scenarios[0].tokens_paged;
        // Change key to different node (still valid key with L0) — should not affect tokens_paged
        let mut sc2 = sc.clone();
        // pick a different valid key node that is not original key but exists and has L0
        // Use na-03 which has L0 but was not hit; still valid key
        sc2.answer_key = vec!["na-03".to_owned()];
        // need to rebuild to validate? directly mutate answer_key without rebuild — paged tokens should be same
        // But our run_benchmark doesn't re-validate; it just uses answer_key as is.
        // To keep validation, we must ensure key still has L0 (it does). So we can reuse sc2 without re-build.
        let report_b =
            run_benchmark(std::slice::from_ref(&sc2)).expect("run b");
        assert_eq!(tokens_a, report_b.scenarios[0].tokens_paged);
        assert_eq!(
            report_a.scenarios[0].tool_calls,
            report_b.scenarios[0].tool_calls
        );
    }

    #[test]
    fn broad_scenario_loses_or_ties() {
        let scenarios = gold_set().expect("gold");
        let sc = scenarios
            .iter()
            .find(|s| s.name == "broad-foxtrot")
            .expect("broad")
            .clone();
        let report = run_benchmark(std::slice::from_ref(&sc)).expect("run");
        let m = &report.scenarios[0];
        assert!(
            m.tokens_paged >= m.tokens_baseline,
            "broad should lose or tie: paged {} vs baseline {}",
            m.tokens_paged,
            m.tokens_baseline
        );
    }

    #[test]
    fn narrow_scenario_wins() {
        let scenarios = gold_set().expect("gold");
        let sc = scenarios
            .iter()
            .find(|s| s.name == "narrow-alpha")
            .expect("alpha")
            .clone();
        let report = run_benchmark(std::slice::from_ref(&sc)).expect("run");
        let m = &report.scenarios[0];
        assert!(
            m.tokens_paged < m.tokens_baseline,
            "narrow should win: paged {} vs baseline {}",
            m.tokens_paged,
            m.tokens_baseline
        );
    }

    #[test]
    fn aggregate_decision_rule() {
        let scenarios = gold_set().expect("gold");
        let report = run_benchmark(&scenarios).expect("run");
        let expected_go = report.total_recall_paged
            == report.total_recall_baseline
            && report.total_paged * 2 < report.total_baseline;
        assert_eq!(report.go, expected_go);
    }

    #[test]
    fn report_canonical_order() {
        let mut scenarios = gold_set().expect("gold");
        // Shuffle order
        scenarios.reverse();
        let report = run_benchmark(&scenarios).expect("run");
        let names: Vec<String> =
            report.scenarios.iter().map(|m| m.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn deterministic_run_twice_byte_equal() {
        let scenarios = gold_set().expect("gold");
        let r1 = run_benchmark(&scenarios).expect("r1");
        let r2 = run_benchmark(&scenarios).expect("r2");
        assert_eq!(r1, r2);
    }

    #[test]
    fn no_mutation_scheduler_state_unchanged() {
        let scenarios = gold_set().expect("gold");
        let before: Vec<(Vec<SchedulerEntry>, u64)> = scenarios
            .iter()
            .map(|sc| {
                (sc.state.state.entries().to_vec(), sc.state.state.tick())
            })
            .collect();
        let _ = run_benchmark(&scenarios).expect("run");
        for (sc, (entries, tick)) in scenarios.iter().zip(before) {
            assert_eq!(sc.state.state.entries(), entries.as_slice());
            assert_eq!(sc.state.state.tick(), tick);
        }
    }

    #[test]
    fn gold_set_has_six_scenarios_and_names() {
        let scenarios = gold_set().expect("gold");
        assert_eq!(scenarios.len(), 6);
        let mut names: Vec<String> =
            scenarios.iter().map(|s| s.name.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "broad-foxtrot",
                "medium-echo",
                "narrow-alpha",
                "narrow-beta",
                "narrow-delta",
                "narrow-gamma"
            ]
        );
    }
}
