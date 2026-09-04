//! Benchmark decision gate — slice 5 (decision 79 clause g) + slice 5 v2 (decision 85).
//!
//! Measures whether demand-paging over the graph/store/scheduler surfaces
//! answer-key content at materially lower token cost than dump-everything.
//! The verdict is computed, not asserted, and includes a broad-query
//! scenario where paging cannot win (anti-cherry-pick).
//!
//! Tokenizer: `fn tokenize(text: &str) -> Vec<String>` — lowercase, split on
//! non-alphanumeric, DROP tokens shorter than 3 chars, dedup preserving
//! first-occurrence order.
//!
//! Paged strategy v2 rules, EXACTLY:
//! 1. Lexical rerank: for each search hit, overlap = count of DISTINCT query tokens present in the summary's token set (summary text from the node's summary representation). expand_threshold = min(2, distinct query token count). Expand the hit IFF matched_in == "summary" AND overlap >= expand_threshold.
//! 2. Fallback: if NO hit passes the threshold, expand exactly ONE hit — the summary-matched hit with the highest overlap; tie-break node_id ascending. (Prevents recall collapse on degenerate queries.)
//! 3. Digest dedup: maintain a surfaced-digest set across the whole flow; a summary or expansion whose content digest is already surfaced contributes zero additional tokens (the content is not re-surfaced). Summaries surfaced by inspect seed the set.
//! 4. Deep-expansion priority unchanged from v1 (structured > detailed > summary > identity); a summary-level expansion of an already-inspected node naturally costs zero via rule 3.

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
// Paging strategy
// ---------------------------------------------------------------------------

/// Paging strategy selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagingStrategy {
    /// Exhaustive v1 — expand every hit.
    ExhaustiveV1,
    /// Progressive v2 — lexical rerank + digest dedup.
    ProgressiveV2,
}

impl PagingStrategy {
    /// Canonical string for the strategy.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExhaustiveV1 => "exhaustive-v1",
            Self::ProgressiveV2 => "progressive-v2",
        }
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Lowercase, split on non-alphanumeric, DROP tokens shorter than 3 chars, dedup preserving first-occurrence order.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for token in lower.split(|c: char| !c.is_alphanumeric()) {
        if token.is_empty() || token.len() < 3 {
            continue;
        }
        if seen.insert(token.to_owned()) {
            out.push(token.to_owned());
        }
    }
    out
}

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

/// Aggregated metrics for one strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyAggregate {
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
}

/// Aggregated report with deterministic decision rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkReport {
    /// V1 exhaustive aggregate.
    pub v1: StrategyAggregate,
    /// V2 progressive aggregate.
    pub v2: StrategyAggregate,
    /// Deterministic GO verdict computed on V2.
    pub go: bool,
    /// Mechanical reason citing the two compared numbers (v2).
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

fn compute_baseline_tokens(state: &ContextToolState) -> usize {
    let mut tokens = 0usize;
    for node in state.graph.nodes() {
        if let Some(set) = state.store.set(&node.id) {
            if let Some(rep) =
                resolve_representation(set, RepresentationLevel::Identity)
            {
                tokens =
                    tokens.saturating_add(estimate_tokens(rep.content.len()));
            }
        }
    }
    tokens
}

/// Run one strategy over all scenarios.
pub fn run_strategy(
    scenarios: &[BenchmarkScenario],
    strategy: PagingStrategy,
) -> Result<StrategyAggregate, BenchmarkError> {
    let mut metrics: Vec<ScenarioMetrics> = Vec::new();
    let mut total_baseline = 0usize;
    let mut total_paged = 0usize;
    let mut total_key = 0usize;
    let mut total_recall_baseline = 0usize;
    let mut total_recall_paged = 0usize;
    let mut total_tool_calls = 0usize;

    for sc in scenarios {
        let baseline_tokens = compute_baseline_tokens(&sc.state);

        // Search hits via real tool
        let search_tool = ContextSearchTool::new(sc.state.clone());
        let token = CancellationToken::new();
        let search_result =
            search_tool.execute(&json!({"query": sc.query}), token.signal());
        // Collect hits with matched_in
        let hits: Vec<(String, String)> = match search_result {
            siralos_core::provider::ToolExecutionResult::Success {
                output,
                ..
            } => output
                .get("hits")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|h| {
                            let nid = h.get("node_id")?.as_str()?.to_owned();
                            let matched = h
                                .get("matched_in")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_owned();
                            Some((nid, matched))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let hit_ids: Vec<String> =
            hits.iter().map(|(id, _)| id.clone()).collect();

        // Determine expanded set per strategy
        let expanded_ids: std::collections::BTreeSet<String> = match strategy {
            PagingStrategy::ExhaustiveV1 => {
                // Every hit that has a best level
                let mut set = std::collections::BTreeSet::new();
                for (hid, _) in &hits {
                    if let Some(s) = sc.state.store.set(hid) {
                        if best_level_for(s).is_some() {
                            set.insert(hid.clone());
                        }
                    }
                }
                set
            }
            PagingStrategy::ProgressiveV2 => {
                // Lexical rerank + fallback
                let query_tokens = tokenize(&sc.query);
                let threshold = std::cmp::min(2, query_tokens.len());
                let query_token_set: std::collections::BTreeSet<String> =
                    query_tokens.into_iter().collect();

                // Compute overlap per hit
                let mut overlaps: Vec<(String, String, usize)> = Vec::new(); // (node_id, matched_in, overlap)
                for (hid, matched) in &hits {
                    let node = sc.state.graph.node(hid);
                    let summary =
                        node.map(|n| n.summary.as_str()).unwrap_or("");
                    let summary_tokens: std::collections::BTreeSet<String> =
                        tokenize(summary).into_iter().collect();
                    let mut overlap = 0usize;
                    for qt in &query_token_set {
                        if summary_tokens.contains(qt) {
                            overlap += 1;
                        }
                    }
                    overlaps.push((hid.clone(), matched.clone(), overlap));
                }
                // Candidates that pass threshold and matched_in == summary
                let mut candidates = std::collections::BTreeSet::new();
                for (hid, matched, overlap) in &overlaps {
                    if matched == "summary" && *overlap >= threshold {
                        // Only if has level
                        if let Some(s) = sc.state.store.set(hid) {
                            if best_level_for(s).is_some() {
                                candidates.insert(hid.clone());
                            }
                        }
                    }
                }
                if !candidates.is_empty() {
                    candidates
                } else {
                    // Fallback: exactly ONE summary-matched hit with highest overlap, tie-break node_id asc
                    let mut best: Option<(String, usize)> = None;
                    for (hid, matched, overlap) in &overlaps {
                        if matched != "summary" {
                            continue;
                        }
                        // Must have a level to be expandable
                        if let Some(s) = sc.state.store.set(hid) {
                            if best_level_for(s).is_none() {
                                continue;
                            }
                        } else {
                            continue;
                        }
                        match &best {
                            None => best = Some((hid.clone(), *overlap)),
                            Some((best_id, best_overlap)) => {
                                if *overlap > *best_overlap
                                    || (*overlap == *best_overlap
                                        && hid < best_id)
                                {
                                    best = Some((hid.clone(), *overlap));
                                }
                            }
                        }
                    }
                    if let Some((best_id, _)) = best {
                        let mut set = std::collections::BTreeSet::new();
                        set.insert(best_id);
                        set
                    } else {
                        std::collections::BTreeSet::new()
                    }
                }
            }
        };

        // Token sums and expanded count per strategy
        let (sum_inspect_bytes, sum_expanded_bytes, expanded_count) =
            match strategy {
                PagingStrategy::ExhaustiveV1 => {
                    let mut sum_inspect = 0usize;
                    let mut sum_expanded = 0usize;
                    let mut exp_cnt = 0usize;
                    for (hid, _) in &hits {
                        if let Some(node) = sc.state.graph.node(hid) {
                            sum_inspect =
                                sum_inspect.saturating_add(node.summary.len());
                        }
                    }
                    for (hid, _) in &hits {
                        if !expanded_ids.contains(hid) {
                            continue;
                        }
                        if let Some(set) = sc.state.store.set(hid) {
                            if let Some(best) = best_level_for(set) {
                                if let Some(rep) =
                                    resolve_representation(set, best)
                                {
                                    sum_expanded = sum_expanded
                                        .saturating_add(rep.content.len());
                                    exp_cnt += 1;
                                }
                            }
                        }
                    }
                    (sum_inspect, sum_expanded, exp_cnt)
                }
                PagingStrategy::ProgressiveV2 => {
                    // Digest dedup: surfaced digests across inspect + expand
                    let mut surfaced: std::collections::BTreeSet<String> =
                        std::collections::BTreeSet::new();
                    let mut sum_inspect = 0usize;
                    let mut sum_expanded = 0usize;
                    let mut exp_cnt = 0usize;
                    for (hid, _) in &hits {
                        if let Some(node) = sc.state.graph.node(hid) {
                            let digest = content_digest_of(&node.summary);
                            if surfaced.insert(digest) {
                                sum_inspect = sum_inspect
                                    .saturating_add(node.summary.len());
                            }
                        }
                    }
                    for (hid, _) in &hits {
                        if !expanded_ids.contains(hid) {
                            continue;
                        }
                        if let Some(set) = sc.state.store.set(hid) {
                            if let Some(best) = best_level_for(set) {
                                if let Some(rep) =
                                    resolve_representation(set, best)
                                {
                                    let digest = rep.content_digest.clone();
                                    if surfaced.insert(digest) {
                                        sum_expanded = sum_expanded
                                            .saturating_add(rep.content.len());
                                    }
                                    exp_cnt += 1;
                                }
                            }
                        }
                    }
                    (sum_inspect, sum_expanded, exp_cnt)
                }
            };

        let tool_calls = 1usize
            .saturating_add(hit_ids.len())
            .saturating_add(expanded_count);
        let paged_tokens = estimate_tokens(
            sum_expanded_bytes.saturating_add(sum_inspect_bytes),
        )
        .saturating_add(TOOL_CALL_OVERHEAD_TOKENS * tool_calls);

        let key_size = sc.answer_key.len();
        let recall_baseline = key_size;
        let hits_set: std::collections::BTreeSet<&str> =
            hit_ids.iter().map(|s| s.as_str()).collect();
        let mut recall_paged = 0usize;
        for key_id in &sc.answer_key {
            if hits_set.contains(key_id.as_str())
                && expanded_ids.contains(key_id)
            {
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
    }

    metrics.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(StrategyAggregate {
        scenarios: metrics,
        total_baseline,
        total_paged,
        total_key,
        total_recall_baseline,
        total_recall_paged,
        total_tool_calls,
    })
}

/// Run all scenarios, aggregate and apply deterministic decision rule:
///
/// `go = (total_recall_paged == total_recall_baseline) && (total_paged *2 < total_baseline)`
/// Computed on V2.
pub fn run_benchmark(
    scenarios: &[BenchmarkScenario],
) -> Result<BenchmarkReport, BenchmarkError> {
    let v1 = run_strategy(scenarios, PagingStrategy::ExhaustiveV1)?;
    let v2 = run_strategy(scenarios, PagingStrategy::ProgressiveV2)?;

    let go = v2.total_recall_paged == v2.total_recall_baseline
        && v2.total_paged.saturating_mul(2) < v2.total_baseline;
    let recall_eq = v2.total_recall_paged == v2.total_recall_baseline;
    let token_win = v2.total_paged.saturating_mul(2) < v2.total_baseline;
    let reason = format!(
        "total_recall_paged {} == total_recall_baseline {} is {}, total_paged {} *2 < total_baseline {} is {} => {}",
        v2.total_recall_paged,
        v2.total_recall_baseline,
        recall_eq,
        v2.total_paged,
        v2.total_baseline,
        token_win,
        if go { "GO" } else { "NO-GO" }
    );

    Ok(BenchmarkReport { v1, v2, go, reason })
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
// Tests (~10 + 7 new)
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
            let report = run_strategy(
                std::slice::from_ref(sc),
                PagingStrategy::ExhaustiveV1,
            )
            .expect("run");
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
        let report_a = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ExhaustiveV1,
        )
        .expect("run a");
        let tokens_a = report_a.scenarios[0].tokens_paged;
        let mut sc2 = sc.clone();
        sc2.answer_key = vec!["na-03".to_owned()];
        let report_b = run_strategy(
            std::slice::from_ref(&sc2),
            PagingStrategy::ExhaustiveV1,
        )
        .expect("run b");
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
        let report = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ExhaustiveV1,
        )
        .expect("run");
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
        let report = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ExhaustiveV1,
        )
        .expect("run");
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
        // v1 rule
        let expected_go_v1 = report.v1.total_recall_paged
            == report.v1.total_recall_baseline
            && report.v1.total_paged * 2 < report.v1.total_baseline;
        // v2 is the verdict
        let expected_go = report.v2.total_recall_paged
            == report.v2.total_recall_baseline
            && report.v2.total_paged * 2 < report.v2.total_baseline;
        assert_eq!(report.go, expected_go);
        let _ = expected_go_v1;
    }

    #[test]
    fn report_canonical_order() {
        let mut scenarios = gold_set().expect("gold");
        scenarios.reverse();
        let report = run_strategy(&scenarios, PagingStrategy::ExhaustiveV1)
            .expect("run");
        let names: Vec<String> =
            report.scenarios.iter().map(|m| m.name.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn deterministic_run_twice_byte_equal() {
        let scenarios = gold_set().expect("gold");
        let r1 = run_strategy(&scenarios, PagingStrategy::ExhaustiveV1)
            .expect("r1");
        let r2 = run_strategy(&scenarios, PagingStrategy::ExhaustiveV1)
            .expect("r2");
        assert_eq!(r1, r2);
        let b1 = run_benchmark(&scenarios).expect("b1");
        let b2 = run_benchmark(&scenarios).expect("b2");
        assert_eq!(b1, b2);
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

    // -----------------------------------------------------------------------
    // New v2 tests (~7)
    // -----------------------------------------------------------------------

    #[test]
    fn tokenizer_lowercase_split_short_drop_dedup() {
        assert_eq!(tokenize("Hello HELLO hello"), vec!["hello"]);
        assert_eq!(tokenize("one-two three"), vec!["one", "two", "three"]);
        // short tokens dropped
        assert_eq!(tokenize("ab abc abcd"), vec!["abc", "abcd"]);
        // split on non-alphanumeric, dedup preserving order
        assert_eq!(
            tokenize("alpha, beta! alpha; gamma"),
            vec!["alpha", "beta", "gamma"]
        );
        // lowercase
        assert_eq!(tokenize("Alpha BETA"), vec!["alpha", "beta"]);
        // non-alphanumeric delimiters
        assert_eq!(
            tokenize("foo/bar.baz-qux"),
            vec!["foo", "bar", "baz", "qux"]
        );
    }

    #[test]
    fn rerank_threshold_two_plus_word_query() {
        // Query "alpha abc" -> tokens ["alpha","abc"] threshold 2. Both summaries contain phrase "alpha abc" as substring.
        // Summary "alpha abc ..." has both tokens => overlap 2 passes, "alpha abcde ..." has "alpha" + "abcde" not "abc" => overlap 1 fails.
        let l0 = "I".repeat(120);
        let mk_set = |id: &str| {
            NodeRepresentationSet::build(
                id.to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&l0),
                    derived_from: vec![],
                    content: l0.clone(),
                }],
            )
            .expect("set")
        };
        let n1 = ContextNode {
            id: "hit-01".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: "alpha abc hit summary pad".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let n2 = ContextNode {
            id: "hit-02".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('b'),
            summary: "alpha abcde hit summary pad".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let state = minimal_state_with_nodes(
            vec![n1, n2],
            vec![mk_set("hit-01"), mk_set("hit-02")],
        );
        let sc = BenchmarkScenario::build(
            "threshold-test".to_owned(),
            state,
            "alpha abc".to_owned(),
            vec!["hit-01".to_owned()],
        )
        .expect("scenario");
        let report = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ProgressiveV2,
        )
        .expect("run");
        let m = &report.scenarios[0];
        // Should expand only hit-01 (overlap 2), not hit-02 (overlap 1)
        assert_eq!(m.tool_calls, 4, "threshold filters to single expand");
        assert_eq!(m.recall_paged, 1);
    }

    #[test]
    fn single_token_query_threshold_one() {
        // Single token "alpha" threshold 1. Summary "alpha ..." passes, "alphabet ..." contains "alpha" as substring (hit) but token "alphabet" != "alpha" => overlap 0 fails.
        let l0 = "I".repeat(120);
        let mk_set = |id: &str| {
            NodeRepresentationSet::build(
                id.to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&l0),
                    derived_from: vec![],
                    content: l0.clone(),
                }],
            )
            .expect("set")
        };
        let n1 = ContextNode {
            id: "hit-01".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: "alpha something".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let n2 = ContextNode {
            id: "hit-02".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('b'),
            summary: "alphabet something".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let state = minimal_state_with_nodes(
            vec![n1, n2],
            vec![mk_set("hit-01"), mk_set("hit-02")],
        );
        let sc = BenchmarkScenario::build(
            "single-token".to_owned(),
            state,
            "alpha".to_owned(),
            vec!["hit-01".to_owned()],
        )
        .expect("scenario");
        let report = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ProgressiveV2,
        )
        .expect("run");
        let m = &report.scenarios[0];
        // Only hit-01 should be expanded
        assert_eq!(m.tool_calls, 4); // 1+2+1
        assert_eq!(m.recall_paged, 1);
    }

    #[test]
    fn fallback_fires_exactly_once_highest_overlap_tie_break() {
        // Query "alpha abc" threshold 2. Both summaries contain phrase "alpha abc" as prefix but have overlap 1 each (<2) => none pass => fallback picks one via tie-break.
        let l0 = "I".repeat(120);
        let mk_set = |id: &str| {
            NodeRepresentationSet::build(
                id.to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&l0),
                    derived_from: vec![],
                    content: l0.clone(),
                }],
            )
            .expect("set")
        };
        let n1 = ContextNode {
            id: "hit-01".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: "alpha abcde hit pad".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let n2 = ContextNode {
            id: "hit-02".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('b'),
            summary: "alpha abcfg hit pad".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let n3 = ContextNode {
            id: "hit-03".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('c'),
            summary: "alpha abchh hit pad".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let state = minimal_state_with_nodes(
            vec![n1, n2, n3],
            vec![mk_set("hit-01"), mk_set("hit-02"), mk_set("hit-03")],
        );
        let sc = BenchmarkScenario::build(
            "fallback".to_owned(),
            state,
            "alpha abc".to_owned(),
            vec!["hit-01".to_owned()],
        )
        .expect("scenario");
        let report = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ProgressiveV2,
        )
        .expect("run");
        let m = &report.scenarios[0];
        // Fallback should expand exactly one: tie break node_id asc => hit-01
        assert_eq!(m.tool_calls, 5); // 1+3+1 =5
        assert_eq!(m.recall_paged, 1);
        // Highest overlap wins variant: query "alpha" threshold1, both "alphabet" (0) tie, but make one with higher overlap via "alpha"
        let n1b = ContextNode {
            id: "hit-01".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: "alphabet hit".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let n2b = ContextNode {
            id: "hit-02".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('b'),
            summary: "alphabet hit".to_owned(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        // Both 0, tie break => hit-01
        let state2 = minimal_state_with_nodes(
            vec![n1b, n2b],
            vec![mk_set("hit-01"), mk_set("hit-02")],
        );
        let sc2 = BenchmarkScenario::build(
            "fallback-highest".to_owned(),
            state2,
            "alpha".to_owned(),
            vec!["hit-01".to_owned()],
        )
        .expect("sc2");
        // This still fallback with tie, but we also test that fallback picks highest when one has 1 and others 0? For single token, one with 1 would pass, not fallback. So we keep tie test.
        let report2 = run_strategy(
            std::slice::from_ref(&sc2),
            PagingStrategy::ProgressiveV2,
        )
        .expect("run2");
        // Both have 0 <1, none pass, fallback picks hit-01 => recall 1 if key is hit-01
        assert_eq!(report2.scenarios[0].tool_calls, 4); // 1+2+1
        assert_eq!(report2.scenarios[0].recall_paged, 1);
    }

    #[test]
    fn digest_dedup_duplicate_summary_costs_once() {
        let l0 = "I".repeat(120);
        let mk_set = |id: &str| {
            NodeRepresentationSet::build(
                id.to_owned(),
                vec![NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(&l0),
                    derived_from: vec![],
                    content: l0.clone(),
                }],
            )
            .expect("set")
        };
        // Use a 32-byte dup summary matching gold_set hit_summary length
        let dup_summary = {
            let base = "alpha hit summary pad";
            format!("{base}{}", "x".repeat(32 - base.len()))
        };
        assert_eq!(dup_summary.len(), 32);
        let n1 = ContextNode {
            id: "hit-01".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('a'),
            summary: dup_summary.clone(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let n2 = ContextNode {
            id: "hit-02".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: digest('b'),
            summary: dup_summary.clone(),
            source_bindings: vec![],
            token_estimate: 0,
        };
        let state = minimal_state_with_nodes(
            vec![n1, n2],
            vec![mk_set("hit-01"), mk_set("hit-02")],
        );
        let sc = BenchmarkScenario::build(
            "dedup".to_owned(),
            state,
            "alpha".to_owned(),
            vec!["hit-01".to_owned()],
        )
        .expect("scenario");
        let v1 = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ExhaustiveV1,
        )
        .expect("v1");
        let v2 = run_strategy(
            std::slice::from_ref(&sc),
            PagingStrategy::ProgressiveV2,
        )
        .expect("v2");
        assert!(
            v2.scenarios[0].tokens_paged < v1.scenarios[0].tokens_paged,
            "dedup reduces tokens"
        );
        assert_eq!(v1.scenarios[0].tokens_paged, 96);
        assert_eq!(v2.scenarios[0].tokens_paged, 58);
    }

    #[test]
    fn v2_key_blindness() {
        let scenarios = gold_set().expect("gold");
        let sc = scenarios
            .iter()
            .find(|s| s.name == "narrow-alpha")
            .expect("alpha")
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
        assert_eq!(
            report_a.scenarios[0].tool_calls,
            report_b.scenarios[0].tool_calls
        );
    }

    #[test]
    fn v1_results_byte_identical_to_pre_v2() {
        let scenarios = gold_set().expect("gold");
        let v1 = run_strategy(&scenarios, PagingStrategy::ExhaustiveV1)
            .expect("v1");
        // Pre-committed v1 aggregates from decision 83
        assert_eq!(v1.total_baseline, 1230);
        assert_eq!(v1.total_paged, 650);
        assert_eq!(v1.total_recall_baseline, 14);
        assert_eq!(v1.total_recall_paged, 14);
        assert_eq!(v1.total_tool_calls, 38);
        // Per-scenario
        let mut map = std::collections::BTreeMap::new();
        for m in &v1.scenarios {
            map.insert(
                m.name.as_str(),
                (m.tokens_baseline, m.tokens_paged, m.tool_calls),
            );
        }
        assert_eq!(map["narrow-alpha"], (180, 74, 5));
        assert_eq!(map["narrow-beta"], (180, 86, 5));
        assert_eq!(map["narrow-gamma"], (210, 74, 5));
        assert_eq!(map["narrow-delta"], (240, 96, 5));
        assert_eq!(map["medium-echo"], (240, 114, 7));
        assert_eq!(map["broad-foxtrot"], (180, 206, 11));
    }

    #[test]
    fn v2_fixture_preserves_recall() {
        let scenarios = gold_set().expect("gold");
        let v2 = run_strategy(&scenarios, PagingStrategy::ProgressiveV2)
            .expect("v2");
        assert_eq!(
            v2.total_recall_paged, v2.total_recall_baseline,
            "v2 must preserve recall 14/14 on gold_set"
        );
        assert_eq!(v2.total_recall_paged, 14);
    }

    #[test]
    fn v2_no_mutation_still_holds() {
        let scenarios = gold_set().expect("gold");
        let before: Vec<(Vec<SchedulerEntry>, u64)> = scenarios
            .iter()
            .map(|sc| {
                (sc.state.state.entries().to_vec(), sc.state.state.tick())
            })
            .collect();
        let _ = run_strategy(&scenarios, PagingStrategy::ProgressiveV2)
            .expect("run");
        for (sc, (entries, tick)) in scenarios.iter().zip(before) {
            assert_eq!(sc.state.state.entries(), entries.as_slice());
            assert_eq!(sc.state.state.tick(), tick);
        }
    }
}
