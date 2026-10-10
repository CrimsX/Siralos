//! Validated working-set state and the deterministic tick pipeline.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

use super::input::{
    SearchScore, TickInput, canonicalize_search_scores_with_dropped,
};
use super::policy::{
    SchedulerConfig, hot_unique_total, pinned_hot_unique_total, score,
    tier_for_score,
};
use super::types::{
    SchedulerEntry, SchedulerError, SchedulerEvent, SchedulerOutcome,
    TickReport, WorkingSetTier,
};
use super::{MAX_SCHEDULER_NODES, PINNED_HOT_BUDGET_TOKENS};

/// Advancing tick trait — provides the `tick(&mut self, config)` name
/// required by the spec without colliding with the `tick(&self) -> u64`
/// accessor (Rust cannot overload on arity alone).
pub trait SchedulerTick {
    /// Advance the logical clock by one and re-tier.
    fn tick(&mut self, config: &SchedulerConfig) -> TickReport;
}

/// Transparent view for the `tick(&self) -> u64` accessor via `Deref`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct TickView(pub u64);

impl TickView {
    /// Current tick.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.0
    }
}

/// Validated working-set state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingSetState {
    // `entries`, `tick`, and `last_search_scores` are `pub(super)` so the
    // sibling policy/assembly submodules can account over and update this
    // state directly; the crate-external surface stays accessor-based.
    pub(super) entries: Vec<SchedulerEntry>,
    pub(super) tick: u64,
    tick_view: TickView,
    // A4 coalescing state
    last_graph_revision: Option<String>,
    last_new_nodes: Vec<String>,
    pipeline_runs: usize,
    // Decision 93: last observed search scores for assembly retention (None = absent -> unchanged order)
    pub(super) last_search_scores: Option<Vec<SearchScore>>,
}

impl WorkingSetState {
    /// Build and validate, returning canonical node_id order.
    pub fn build(
        entries: Vec<SchedulerEntry>,
    ) -> Result<Self, SchedulerError> {
        if entries.len() > MAX_SCHEDULER_NODES {
            return Err(SchedulerError::InvalidConfig {
                reason: "too many nodes".to_owned(),
            });
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for e in &entries {
            if e.relevance > 100 {
                return Err(SchedulerError::RelevanceOutOfRange {
                    node_id: e.node_id.clone(),
                    value: e.relevance,
                });
            }
            if !seen.insert(e.node_id.clone()) {
                return Err(SchedulerError::DuplicateNode {
                    node_id: e.node_id.clone(),
                });
            }
        }
        let mut sorted = entries;
        sorted.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Ok(Self {
            entries: sorted,
            tick: 0,
            tick_view: TickView(0),
            last_graph_revision: None,
            last_new_nodes: Vec::new(),
            pipeline_runs: 0,
            last_search_scores: None,
        })
    }

    /// Entries in canonical order.
    #[must_use]
    pub fn entries(&self) -> &[SchedulerEntry] {
        &self.entries
    }

    /// Current tick value (via `TickRead::tick`).
    #[must_use]
    pub(crate) fn tick_value(&self) -> u64 {
        self.tick
    }

    /// Pipeline runs observable for A4 coalescing tests.
    #[must_use]
    pub fn pipeline_runs(&self) -> usize {
        self.pipeline_runs
    }

    /// Whether this tick input would coalesce (A4 no-op) given current coalescing state.
    /// Search scores are NOT part of coalescing (they affect only assembly, not the tick pipeline).
    #[must_use]
    pub(crate) fn is_coalesced_input(&self, input: &TickInput) -> bool {
        input.events.is_empty()
            && self.last_graph_revision.as_deref()
                == Some(input.graph_revision.as_str())
            && self.last_new_nodes == input.new_node_ids
    }

    /// Last observed search scores (decision 93, None means absent).
    #[must_use]
    pub fn last_search_scores(&self) -> Option<&[SearchScore]> {
        self.last_search_scores.as_deref()
    }

    /// Set search scores for testing / direct assembly (canonicalized, bounded).
    /// Empty => None (absent).
    pub fn set_search_scores(&mut self, scores: Vec<SearchScore>) {
        let (canonical, _) = canonicalize_search_scores_with_dropped(scores);
        if canonical.is_empty() {
            self.last_search_scores = None;
        } else {
            self.last_search_scores = Some(canonical);
        }
    }

    /// One entry by id.
    #[must_use]
    pub fn entry(&self, node_id: &str) -> Option<&SchedulerEntry> {
        self.entries.iter().find(|e| e.node_id == node_id)
    }

    pub(super) fn entry_mut(
        &mut self,
        node_id: &str,
    ) -> Option<&mut SchedulerEntry> {
        self.entries.iter_mut().find(|e| e.node_id == node_id)
    }

    /// Apply one synchronous event.
    pub fn apply_event(
        &mut self,
        event: SchedulerEvent,
    ) -> Result<SchedulerOutcome, SchedulerError> {
        let node_id = event.node_id().to_owned();
        let idx = self
            .entries
            .iter()
            .position(|e| e.node_id == node_id)
            .ok_or_else(|| SchedulerError::UnknownNode {
                node_id: node_id.clone(),
            })?;
        let tier_before = self.entries[idx].tier;
        let tier_after = match event {
            SchedulerEvent::Access { .. } => {
                self.entries[idx].last_access_tick = self.tick;
                self.retier_index(idx)
            }
            SchedulerEvent::Pin { .. } => {
                self.entries[idx].pinned = true;
                WorkingSetTier::Hot
            }
            SchedulerEvent::Unpin { .. } => {
                self.entries[idx].pinned = false;
                self.retier_index(idx)
            }
            SchedulerEvent::Relevance { relevance, .. } => {
                if relevance > 100 {
                    return Err(SchedulerError::RelevanceOutOfRange {
                        node_id: node_id.clone(),
                        value: relevance,
                    });
                }
                self.entries[idx].relevance = relevance;
                self.retier_index(idx)
            }
            SchedulerEvent::Stale { .. } => {
                // Staleness demotes one tier toward Cold regardless of pin.
                match self.entries[idx].tier {
                    WorkingSetTier::Hot => WorkingSetTier::Warm,
                    WorkingSetTier::Warm => WorkingSetTier::Cold,
                    WorkingSetTier::Cold => WorkingSetTier::Cold,
                    WorkingSetTier::Archive => WorkingSetTier::Archive,
                }
            }
            SchedulerEvent::Archive { .. } => WorkingSetTier::Archive,
        };
        self.entries[idx].tier = tier_after;
        Ok(SchedulerOutcome { node_id, tier_before, tier_after })
    }

    fn retier_index(&self, idx: usize) -> WorkingSetTier {
        let entry = &self.entries[idx];
        if entry.tier == WorkingSetTier::Archive {
            return WorkingSetTier::Archive;
        }
        if entry.pinned {
            return WorkingSetTier::Hot;
        }
        let s = score(entry, self.tick);
        tier_for_score(s)
    }

    /// Increment tick and re-tier all non-Archive entries.
    /// This is the inherent alias for the advancing tick; the canonical
    /// trait method is `SchedulerTick::tick`.
    pub(crate) fn advance_tick(
        &mut self,
        _config: &SchedulerConfig,
    ) -> TickReport {
        self.tick += 1;
        self.tick_view = TickView(self.tick);
        let mut promoted: Vec<String> = Vec::new();
        let mut demoted: Vec<String> = Vec::new();
        for entry in &mut self.entries {
            if entry.tier == WorkingSetTier::Archive {
                continue;
            }
            let before = entry.tier;
            let after = if entry.pinned {
                WorkingSetTier::Hot
            } else {
                tier_for_score(score(entry, self.tick))
            };
            if after != before {
                let rank_before = before.order();
                let rank_after = after.order();
                if rank_after < rank_before {
                    promoted.push(entry.node_id.clone());
                } else {
                    demoted.push(entry.node_id.clone());
                }
                entry.tier = after;
            }
        }
        promoted.sort();
        demoted.sort();
        // Record for coalescing parity: advance_tick updates revision baseline to current tick's implicit graph?
        // For benchmark frozen guard, coalescing not involved in advance_tick path.
        TickReport { tick: self.tick, promoted, demoted }
    }

    /// Deterministic pipeline with A1-A4 amendments and pinned order 1..7.
    /// Search scores (decision 93) are stored for the subsequent assembly even on coalesced ticks.
    pub fn process_tick(
        &mut self,
        input: TickInput,
        config: &SchedulerConfig,
    ) -> TickReport {
        // Store search scores for assembly (decision 93): None/absent => unchanged order, Some(non-empty) => score-ordered retention.
        // Canonical already; empty is treated as absent.
        let search_scores_for_assembly = input.search_scores.clone();
        self.last_search_scores = search_scores_for_assembly;

        // 1. Coalescing guard: empty events AND no graph deltas since last processed tick.
        // Search scores do NOT affect coalescing (they are for assembly only).
        let is_noop = input.events.is_empty()
            && self.last_graph_revision.as_deref()
                == Some(input.graph_revision.as_str())
            && self.last_new_nodes == input.new_node_ids;
        if is_noop {
            // Identical output without pipeline run (but search scores already stored for assembly).
            return TickReport {
                tick: self.tick,
                promoted: Vec::new(),
                demoted: Vec::new(),
            };
        }

        let before_tiers: BTreeMap<String, WorkingSetTier> =
            self.entries.iter().map(|e| (e.node_id.clone(), e.tier)).collect();

        // Update clock to now before scoring.
        self.tick = input.now;
        self.tick_view = TickView(self.tick);

        // 2. Stale demotion (existing, any tier, regardless of pin)
        {
            let stale_set: BTreeSet<String> =
                input.stale_node_ids.iter().cloned().collect();
            for entry in &mut self.entries {
                if stale_set.contains(&entry.node_id) {
                    let before = entry.tier;
                    let after = match before {
                        WorkingSetTier::Hot => WorkingSetTier::Warm,
                        WorkingSetTier::Warm => WorkingSetTier::Cold,
                        WorkingSetTier::Cold => WorkingSetTier::Cold,
                        WorkingSetTier::Archive => WorkingSetTier::Archive,
                    };
                    if after != before {
                        entry.tier = after;
                    }
                }
            }
        }

        // 3. Demand-edge score updates (A1): recency=now, relevance+=32 saturating
        for ev in &input.events {
            if let Some(entry) = self.entry_mut(&ev.node_id) {
                entry.last_access_tick = input.now;
                let new_rel = entry.relevance.saturating_add(32);
                entry.relevance = new_rel.min(100);
            }
        }

        // 4/5. Recompute scores + promotion gate (stale never promotes)
        {
            let stale_set: BTreeSet<String> =
                input.stale_node_ids.iter().cloned().collect();
            for entry in &mut self.entries {
                if entry.tier == WorkingSetTier::Archive {
                    continue;
                }
                let before = entry.tier;
                let s = score(entry, self.tick);
                let pin_forces =
                    entry.pinned && !stale_set.contains(&entry.node_id);
                let target = if pin_forces {
                    WorkingSetTier::Hot
                } else {
                    tier_for_score(s)
                };
                // Stale never promotes: if stale and target is hotter than current, refuse.
                let final_target = if stale_set.contains(&entry.node_id) {
                    let cur_rank = before.order();
                    let tgt_rank = target.order();
                    if tgt_rank < cur_rank { before } else { target }
                } else {
                    target
                };
                if final_target != before {
                    entry.tier = final_target;
                }
            }
        }

        // 6. Pin-quota enforcement (A3a): pinned HOT unique digest <=1024
        {
            let mut demoted_pin: Vec<String> = Vec::new();
            loop {
                let total = pinned_hot_unique_total(self);
                if total <= PINNED_HOT_BUDGET_TOKENS {
                    break;
                }
                // Find lowest-scored pinned HOT
                let mut candidate: Option<(u64, String)> = None;
                for e in &self.entries {
                    if e.tier != WorkingSetTier::Hot || !e.pinned {
                        continue;
                    }
                    if demoted_pin.contains(&e.node_id) {
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
                let Some((_, victim)) = candidate else {
                    break;
                };
                if let Some(entry) = self.entry_mut(&victim) {
                    entry.tier = WorkingSetTier::Warm;
                }
                demoted_pin.push(victim.clone());
                // loop recomputes total with new demotion (unique accounting will account)
            }
            demoted_pin.sort();
        }

        // 7. Budget enforcement (A2) with unique-digest accounting, skipping pinned
        {
            let mut demoted_budget: Vec<String> = Vec::new();
            loop {
                let total = hot_unique_total(self);
                if total <= config.budget_tokens {
                    break;
                }
                // Find lowest-scored HOT non-pinned
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
                let Some((_, victim)) = candidate else {
                    break;
                };
                if let Some(entry) = self.entry_mut(&victim) {
                    entry.tier = WorkingSetTier::Warm;
                }
                demoted_budget.push(victim.clone());
                // continue; next iteration recomputes total (accounts for duplicate digests dropping)
                // Guard against infinite loop if no progress (duplicate digest not reducing total)
                // But we still demote lowest, eventually all non-pinned will be demoted.
                if demoted_budget.len() > self.entries.len() {
                    break;
                }
            }
            demoted_budget.sort();
        }

        // Collect promoted/demoted vs before snapshot for report
        let mut promoted: Vec<String> = Vec::new();
        let mut demoted: Vec<String> = Vec::new();
        for e in &self.entries {
            let before =
                before_tiers.get(&e.node_id).copied().unwrap_or(e.tier);
            if e.tier != before {
                let before_rank = before.order();
                let after_rank = e.tier.order();
                if after_rank < before_rank {
                    promoted.push(e.node_id.clone());
                } else {
                    demoted.push(e.node_id.clone());
                }
            }
        }
        promoted.sort();
        demoted.sort();

        // Update coalescing baseline and pipeline run count
        self.last_graph_revision = Some(input.graph_revision.clone());
        self.last_new_nodes = input.new_node_ids.clone();
        self.pipeline_runs += 1;

        TickReport { tick: self.tick, promoted, demoted }
    }
}

impl SchedulerTick for WorkingSetState {
    fn tick(&mut self, config: &SchedulerConfig) -> TickReport {
        self.advance_tick(config)
    }
}

impl Deref for WorkingSetState {
    type Target = TickView;

    fn deref(&self) -> &Self::Target {
        &self.tick_view
    }
}
