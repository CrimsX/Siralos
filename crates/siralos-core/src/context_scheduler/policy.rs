//! Tiering and budget policy: score rule, tier thresholds, unique-digest
//! token accounting, and budget/quota enforcement (A2/A3).

use std::collections::{BTreeMap, BTreeSet};

use super::DEFAULT_BUDGET_TOKENS;
use super::state::WorkingSetState;
use super::types::{
    BudgetReport, SchedulerEntry, SchedulerError, WorkingSetTier,
};

/// Score at or above which an entry is `Hot`.
const HOT_SCORE_THRESHOLD: u64 = 280;
/// Score at or above which an entry is `Warm` (below `Hot`).
const WARM_SCORE_THRESHOLD: u64 = 120;

/// Scheduler configuration — the budget only (clause f).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerConfig {
    /// Working-set budget in tokens.
    pub budget_tokens: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self { budget_tokens: DEFAULT_BUDGET_TOKENS }
    }
}

impl SchedulerConfig {
    /// Validate and build.
    pub fn new(budget_tokens: usize) -> Result<Self, SchedulerError> {
        if budget_tokens == 0 {
            return Err(SchedulerError::InvalidConfig {
                reason: "budget_tokens must be >= 1".to_owned(),
            });
        }
        Ok(Self { budget_tokens })
    }
}

/// Deterministic integer score for an entry at `tick`.
#[must_use]
pub fn score(entry: &SchedulerEntry, tick: u64) -> u64 {
    let relevance_part: u64 = u64::from(entry.relevance) * 4;
    let ticks_since = tick.saturating_sub(entry.last_access_tick);
    let recency_points: u64 = if ticks_since == 0 {
        30
    } else {
        30u64.saturating_sub(ticks_since.min(30))
    };
    let pin_bonus: u64 = if entry.pinned { 50 } else { 0 };
    relevance_part + recency_points + pin_bonus
}

pub(super) fn tier_for_score(s: u64) -> WorkingSetTier {
    if s >= HOT_SCORE_THRESHOLD {
        WorkingSetTier::Hot
    } else if s >= WARM_SCORE_THRESHOLD {
        WorkingSetTier::Warm
    } else {
        WorkingSetTier::Cold
    }
}

pub(super) fn hot_unique_total(state: &WorkingSetState) -> usize {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut total: usize = 0;
    let mut hot: Vec<&SchedulerEntry> = state
        .entries
        .iter()
        .filter(|e| e.tier == WorkingSetTier::Hot)
        .collect();
    hot.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    for e in hot {
        let digest = if e.content_digest.is_empty() {
            e.node_id.clone()
        } else {
            e.content_digest.clone()
        };
        if seen.insert(digest) {
            total = total.saturating_add(e.token_estimate);
        }
    }
    total
}

pub(super) fn pinned_hot_unique_total(state: &WorkingSetState) -> usize {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut total: usize = 0;
    let mut hot: Vec<&SchedulerEntry> = state
        .entries
        .iter()
        .filter(|e| e.tier == WorkingSetTier::Hot && e.pinned)
        .collect();
    hot.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    for e in hot {
        let digest = if e.content_digest.is_empty() {
            e.node_id.clone()
        } else {
            e.content_digest.clone()
        };
        if seen.insert(digest) {
            total = total.saturating_add(e.token_estimate);
        }
    }
    total
}

impl WorkingSetState {
    /// Enforce the deterministic budget by demoting lowest-scored Hot
    /// non-pinned entries to Warm with unique-digest accounting (A2).
    /// Demotion never deletes. The `token_estimates` param is retained for
    /// backward compat but internal token_estimate + content_digest are
    /// authoritative; if the map omits a node, its internal token is used.
    pub fn enforce_budget(
        &mut self,
        config: &SchedulerConfig,
        token_estimates: &[(String, usize)],
    ) -> Result<BudgetReport, SchedulerError> {
        let map: BTreeMap<String, usize> =
            token_estimates.iter().map(|(k, v)| (k.clone(), *v)).collect();
        // Unknown node in caller map -> error (strict).
        for key in map.keys() {
            if self.entry(key).is_none() {
                return Err(SchedulerError::UnknownNode {
                    node_id: key.clone(),
                });
            }
        }
        // If caller provided token overrides, apply them to entries for this call
        // (keeps old tests that vary tokens via external map).
        // We do not persist them beyond this call's accounting unless entry token differs.
        // For unique accounting we need effective token per entry: use map if present else entry.token_estimate.
        // Also need digest per entry.
        let hot_tokens_before = self.hot_unique_total_with_map(Some(&map));
        let mut hot_total = hot_tokens_before;
        let mut demoted: Vec<String> = Vec::new();
        while hot_total > config.budget_tokens {
            // Find lowest-scored Hot non-pinned entry.
            let mut candidate: Option<(u64, String)> = None;
            for e in &self.entries {
                if e.tier != WorkingSetTier::Hot || e.pinned {
                    continue;
                }
                let s = score(e, self.tick);
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
            let Some((_, victim_id)) = candidate else {
                break;
            };
            if let Some(entry) = self.entry_mut(&victim_id) {
                entry.tier = WorkingSetTier::Warm;
            }
            demoted.push(victim_id.clone());
            hot_total = self.hot_unique_total_with_map(Some(&map));
            if demoted.len() > self.entries.len() {
                break;
            }
        }
        let hot_tokens_after = self.hot_unique_total_with_map(Some(&map));
        demoted.sort();
        Ok(BudgetReport { hot_tokens_before, hot_tokens_after, demoted })
    }

    /// Unique HOT total using internal digests and token_estimate, with optional external map override.
    fn hot_unique_total_with_map(
        &self,
        external: Option<&BTreeMap<String, usize>>,
    ) -> usize {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut total: usize = 0;
        // canonical node ordering
        let mut hot: Vec<&SchedulerEntry> = self
            .entries
            .iter()
            .filter(|e| e.tier == WorkingSetTier::Hot)
            .collect();
        hot.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        for e in hot {
            let digest = if e.content_digest.is_empty() {
                e.node_id.clone()
            } else {
                e.content_digest.clone()
            };
            if seen.insert(digest) {
                let tok = external
                    .and_then(|m| m.get(&e.node_id).copied())
                    .unwrap_or(e.token_estimate);
                total = total.saturating_add(tok);
            }
        }
        total
    }
}
