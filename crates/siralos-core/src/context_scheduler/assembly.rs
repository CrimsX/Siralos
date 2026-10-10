//! HOT-tier L1 assembly (decision 90 B1/B2): summaries, bounded
//! neighbor stubs, and assembled-set budget enforcement.

use std::collections::{BTreeMap, BTreeSet};

use super::PINNED_HOT_BUDGET_TOKENS;
use super::input::{SearchScore, canonicalize_search_scores_with_dropped};
use super::policy::{SchedulerConfig, score};
use super::state::WorkingSetState;
use super::types::WorkingSetTier;

// ---------------------------------------------------------------------------
// Assembly (decision 90 B1/B2)
// ---------------------------------------------------------------------------

/// Neighbor stub — bounded 1-hop neighbor map entry (ids+digests only, never content).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NeighborStub {
    /// Neighbor node id.
    pub node_id: String,
    /// Neighbor content digest (64 hex).
    pub content_digest: String,
}

/// One assembled HOT entry — L1 summary only (never deeper).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledEntry {
    /// Assembled node id.
    pub node_id: String,
    /// Content digest of the assembled summary (for unique-digest accounting).
    pub content_digest: String,
    /// L1 summary content (never deeper; empty if no summary).
    pub summary: String,
    /// Token estimate of the summary (via `estimate_tokens`).
    pub token_estimate: usize,
    /// Bounded neighbor stubs (<=8, node_id ascending, ids+digests only).
    pub neighbor_stubs: Vec<NeighborStub>,
}

/// Assembled context — HOT tier L1 summaries + bounded neighbor stubs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledContext {
    /// Entries in canonical node_id order.
    pub entries: Vec<AssembledEntry>,
    /// Total assembled tokens before budget enforcement.
    pub total_tokens_before: usize,
    /// Total assembled tokens after budget enforcement.
    pub total_tokens_after: usize,
    /// Demoted node ids during assembly budget enforcement (canonical order).
    pub demoted: Vec<String>,
}

pub(crate) fn stub_token_estimate(stub: &NeighborStub) -> usize {
    // Honest byte estimate via the same estimator, no tool-call overhead.
    // Concatenate node_id + digest with a separator.
    crate::context_graph::estimate_tokens(&format!(
        "{}:{}",
        stub.node_id, stub.content_digest
    ))
}

pub(crate) fn assembled_unique_total(entries: &[AssembledEntry]) -> usize {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut total: usize = 0;
    for e in entries {
        let digest = if e.content_digest.is_empty() {
            e.node_id.clone()
        } else {
            e.content_digest.clone()
        };
        if seen.insert(digest) {
            total = total.saturating_add(e.token_estimate);
        }
        for stub in &e.neighbor_stubs {
            let sd = if stub.content_digest.is_empty() {
                stub.node_id.clone()
            } else {
                stub.content_digest.clone()
            };
            if seen.insert(sd) {
                total = total.saturating_add(stub_token_estimate(stub));
            }
        }
    }
    total
}

fn pinned_assembled_unique_total(
    entries: &[AssembledEntry],
    state: &WorkingSetState,
) -> usize {
    let pinned_ids: BTreeSet<String> = state
        .entries()
        .iter()
        .filter(|e| e.tier == WorkingSetTier::Hot && e.pinned)
        .map(|e| e.node_id.clone())
        .collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut total: usize = 0;
    for e in entries {
        if !pinned_ids.contains(&e.node_id) {
            continue;
        }
        let digest = if e.content_digest.is_empty() {
            e.node_id.clone()
        } else {
            e.content_digest.clone()
        };
        if seen.insert(digest) {
            total = total.saturating_add(e.token_estimate);
        }
        for stub in &e.neighbor_stubs {
            let sd = if stub.content_digest.is_empty() {
                stub.node_id.clone()
            } else {
                stub.content_digest.clone()
            };
            if seen.insert(sd) {
                total = total.saturating_add(stub_token_estimate(stub));
            }
        }
    }
    total
}

fn build_assembled_entries(
    state: &WorkingSetState,
    graph: &crate::context_graph::ContextGraph,
    store: &crate::context_representation::ContextRepresentationStore,
) -> Vec<AssembledEntry> {
    let mut hot_ids: Vec<String> = state
        .entries()
        .iter()
        .filter(|e| e.tier == WorkingSetTier::Hot)
        .map(|e| e.node_id.clone())
        .collect();
    hot_ids.sort();
    let mut entries: Vec<AssembledEntry> = Vec::new();
    for node_id in hot_ids {
        // L1 summary resolution: store Summary if present, else graph summary
        let (summary, summary_digest) = if let Some(set) = store.set(&node_id)
        {
            if let Some(rep) = crate::context_representation::resolve_representation(
                set,
                crate::context_representation::RepresentationLevel::Summary,
            ) {
                // Use the stored summary's content and its digest (already validated)
                (rep.content.clone(), rep.content_digest.clone())
            } else if let Some(node) = graph.node(&node_id) {
                // Fallback to graph summary (if any)
                let s = node.summary.clone();
                let d = crate::context_representation::content_digest_of(&s);
                (s, d)
            } else {
                (String::new(), String::new())
            }
        } else if let Some(node) = graph.node(&node_id) {
            let s = node.summary.clone();
            let d = crate::context_representation::content_digest_of(&s);
            (s, d)
        } else {
            (String::new(), String::new())
        };
        let token_estimate = crate::context_graph::estimate_tokens(&summary);
        // 1-hop neighbor map: both directions, distinct, sorted ascending, first 8
        let mut neighbors: BTreeSet<String> = BTreeSet::new();
        for edge in graph.edges() {
            if edge.from == node_id {
                neighbors.insert(edge.to.clone());
            } else if edge.to == node_id {
                neighbors.insert(edge.from.clone());
            }
        }
        neighbors.remove(&node_id);
        let mut neighbor_ids: Vec<String> = neighbors.into_iter().collect();
        neighbor_ids.sort();
        if neighbor_ids.len() > 8 {
            neighbor_ids.truncate(8);
        }
        let mut stubs: Vec<NeighborStub> = Vec::new();
        for nid in neighbor_ids {
            if let Some(n) = graph.node(&nid) {
                stubs.push(NeighborStub {
                    node_id: nid.clone(),
                    content_digest: n.content_digest.clone(),
                });
            } else {
                // Fallback: unknown neighbor (should not happen) uses node_id as digest
                stubs.push(NeighborStub {
                    node_id: nid.clone(),
                    content_digest: nid.clone(),
                });
            }
        }
        // Stubs already sorted by node_id ascending due to sorted neighbor_ids
        entries.push(AssembledEntry {
            node_id,
            content_digest: summary_digest,
            summary,
            token_estimate,
            neighbor_stubs: stubs,
        });
    }
    entries
}

impl WorkingSetState {
    /// Assemble HOT L1 summaries + bounded neighbor stubs (B1/B2) and enforce
    /// budget over the assembled set. Demotion follows existing rule:
    /// pinned-HOT quota (1024) first, then 4096 budget, demoting lowest-scored
    /// HOT nodes (score asc, node_id asc tiebreak) until within quota.
    /// Returns the assembled context after enforcement; the state's tiers are
    /// mutated to reflect demotions (never deletes).
    pub fn assemble(
        &mut self,
        graph: &crate::context_graph::ContextGraph,
        store: &crate::context_representation::ContextRepresentationStore,
        config: &SchedulerConfig,
    ) -> AssembledContext {
        let mut entries = build_assembled_entries(self, graph, store);
        let total_before = assembled_unique_total(&entries);
        let mut demoted: Vec<String> = Vec::new();

        // 6. Pin-quota enforcement over assembled pinned total (1024) — lowest pinned HOT first
        loop {
            let pinned_total = pinned_assembled_unique_total(&entries, self);
            if pinned_total <= PINNED_HOT_BUDGET_TOKENS {
                break;
            }
            let mut candidate: Option<(u64, String)> = None;
            for e in &entries {
                // pinned HOT entry still in assembled set
                let state_entry = self.entry(&e.node_id);
                let Some(se) = state_entry else { continue };
                if se.tier != WorkingSetTier::Hot || !se.pinned {
                    continue;
                }
                let s = score(se, self.tick);
                match &candidate {
                    None => candidate = Some((s, e.node_id.clone())),
                    Some((best_score, best_id)) => {
                        if s < *best_score
                            || (s == *best_score && e.node_id < *best_id)
                        {
                            candidate = Some((s, e.node_id.clone()));
                        }
                    }
                }
            }
            let Some((_, victim)) = candidate else { break };
            if let Some(entry) = self.entry_mut(&victim) {
                entry.tier = WorkingSetTier::Warm;
            }
            demoted.push(victim.clone());
            entries.retain(|e| e.node_id != victim);
            if demoted.len() > self.entries.len() {
                break;
            }
        }

        // 7. Budget enforcement over assembled set (4096) — lowest non-pinned HOT first.
        // Decision 93: when search_scores present and non-empty, order by lower search score first (absent = 0),
        // ties break by scheduler score, then node_id ascending. When absent, order is existing scheduler-score order.
        // Pin-quota order remains unchanged (scheduler score only).
        let search_map: Option<BTreeMap<String, i32>> = self
            .last_search_scores
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(|scores| {
                scores.iter().map(|s| (s.node_id.clone(), s.score)).collect()
            });
        loop {
            let total = assembled_unique_total(&entries);
            if total <= config.budget_tokens {
                break;
            }
            if let Some(map) = search_map.as_ref() {
                let mut candidate: Option<(i32, u64, String)> = None;
                for e in &entries {
                    let state_entry = self.entry(&e.node_id);
                    let Some(se) = state_entry else { continue };
                    if se.tier != WorkingSetTier::Hot || se.pinned {
                        continue;
                    }
                    let retained = map.get(&e.node_id).copied().unwrap_or(0);
                    let sched = score(se, self.tick);
                    match &candidate {
                        None => {
                            candidate =
                                Some((retained, sched, e.node_id.clone()))
                        }
                        Some((best_retained, best_sched, best_id)) => {
                            let is_better = retained < *best_retained
                                || (retained == *best_retained
                                    && sched < *best_sched)
                                || (retained == *best_retained
                                    && sched == *best_sched
                                    && e.node_id < *best_id);
                            if is_better {
                                candidate =
                                    Some((retained, sched, e.node_id.clone()));
                            }
                        }
                    }
                }
                let Some((_, _, victim)) = candidate else { break };
                if let Some(entry) = self.entry_mut(&victim) {
                    entry.tier = WorkingSetTier::Warm;
                }
                demoted.push(victim.clone());
                entries.retain(|e| e.node_id != victim);
                if demoted.len() > self.entries.len() {
                    break;
                }
            } else {
                let mut candidate: Option<(u64, String)> = None;
                for e in &entries {
                    let state_entry = self.entry(&e.node_id);
                    let Some(se) = state_entry else { continue };
                    if se.tier != WorkingSetTier::Hot || se.pinned {
                        continue;
                    }
                    let s = score(se, self.tick);
                    match &candidate {
                        None => candidate = Some((s, e.node_id.clone())),
                        Some((best_score, best_id)) => {
                            if s < *best_score
                                || (s == *best_score && e.node_id < *best_id)
                            {
                                candidate = Some((s, e.node_id.clone()));
                            }
                        }
                    }
                }
                let Some((_, victim)) = candidate else { break };
                if let Some(entry) = self.entry_mut(&victim) {
                    entry.tier = WorkingSetTier::Warm;
                }
                demoted.push(victim.clone());
                entries.retain(|e| e.node_id != victim);
                if demoted.len() > self.entries.len() {
                    break;
                }
            }
        }

        let total_after = assembled_unique_total(&entries);
        demoted.sort();
        // Ensure canonical ordering of entries (already sorted)
        entries.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        AssembledContext {
            entries,
            total_tokens_before: total_before,
            total_tokens_after: total_after,
            demoted,
        }
    }

    /// Assemble with explicit search scores (decision 93, pure for testing):
    /// temporarily sets `last_search_scores` to `search_scores` (canonicalized) and assembles.
    /// Empty or None => unchanged order. Restores previous scores after.
    pub fn assemble_with_search_scores(
        &mut self,
        graph: &crate::context_graph::ContextGraph,
        store: &crate::context_representation::ContextRepresentationStore,
        config: &SchedulerConfig,
        search_scores: Option<Vec<SearchScore>>,
    ) -> AssembledContext {
        let prev = self.last_search_scores.clone();
        match search_scores {
            Some(scores) => {
                let (canonical, _) =
                    canonicalize_search_scores_with_dropped(scores);
                if canonical.is_empty() {
                    self.last_search_scores = None;
                } else {
                    self.last_search_scores = Some(canonical);
                }
            }
            None => self.last_search_scores = None,
        }
        let result = self.assemble(graph, store, config);
        self.last_search_scores = prev;
        result
    }
}
