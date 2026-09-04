//! Deterministic tiered context scheduler — HOT/WARM/COLD working set
//! (decision 79 slice 3, clauses a + f, re-pinned by decision 89).
//!
//! The scheduler is synchronous deterministic ticks on in-turn events — no
//! threads, locks, or async runtime. The working-context budget is a
//! deterministic constant with deterministic demotion on overflow;
//! demotion reorders, it never deletes. Archive is explicit-only.
//!
//! # Scoring
//!
//! ```text
//! score = relevance * 4 + recency_points + pin_bonus
//! recency_points = if ticks_since_access == 0 { 30 }
//!                  else { 30.saturating_sub(ticks_since_access.min(30)) }
//! pin_bonus      = 50 if pinned else 0
//! range          = 0 ..= 480  (100*4 + 30 + 50)
//! ```
//!
//! # Tier thresholds (fixed constants)
//!
//! ```text
//! Hot  if score >= 280
//! Warm if score >= 120
//! else Cold
//! ```
//!
//! `DEFAULT_HOT_THRESHOLD` (70) and `DEFAULT_WARM_THRESHOLD` (30) are the
//! human-readable thresholds; the tier rule multiplies them by 4 to obtain
//! the score thresholds above (70*4=280, 30*4=120). `SchedulerConfig` in
//! this slice carries only the budget — the tier thresholds are fixed and
//! documented once here, tested precisely.
//!
//! # Events
//!
//! * `Access` — updates `last_access_tick = self.tick` and re-tiers via the
//!   score rule (pinned → Hot).
//! * `Pin` — sets `pinned = true` and forces `Hot`.
//! * `Unpin` — clears `pinned` and re-tiers via the score rule.
//! * `Relevance` — updates relevance and re-tiers (pinned → Hot).
//! * `Stale` — content-staleness is a fact, not a preference: demotes one
//!   tier toward Cold (`Hot → Warm`, `Warm → Cold`, `Cold` stays `Cold`)
//!   **regardless of `pinned`**. This precedence is intentional and
//!   deterministic: staleness outranks pin.
//! * `Archive` — explicit-only move to `Archive`; no automatic arc ever
//!   produces `Archive`.
//!
//! # Tick (decision 89 amendments)
//!
//! The decision 89 re-pin adds four amendments with the following pinned
//! processing order, documented regardless of internal code structure:
//!
//! 1. coalescing guard — a tick whose input is a no-op (empty events AND no
//!    graph deltas since the last processed tick, comparing graph revision and
//!    new-node set) produces the identical output state without re-running the
//!    pipeline. Derived from input values only.
//! 2. stale demotion — any `Stale` node demotes one tier toward Cold,
//!    regardless of pin, over any tier.
//! 3. demand-edge score updates — each canonical `AccessEvent` updates its
//!    node: `last_access_tick = now` and `relevance += 32` saturating at 100.
//!    Events may target any tier.
//! 4. recompute scores — `score = relevance*4 + recency + pin_bonus` at `now`.
//! 5. promotion gate — stale nodes never promote; the gate refuses any
//!    upward move for stale nodes even after demand updates.
//! 6. pin-quota enforcement — pinned HOT unique-digest total capped at 1024;
//!    demote lowest-scored pinned HOT to Warm (node_id asc tiebreak) until
//!    within quota. Pin still protects within quota.
//! 7. budget enforcement — HOT unique-digest total capped at 4096; demote
//!    lowest-scored HOT non-pinned to Warm until within budget.
//!
//! Digest-counted budget: among HOT nodes, a content digest's tokens count
//! once at its first node in canonical node ordering; subsequent same-digest
//! HOT nodes contribute 0. Both the 4096 budget and the 1024 pin quota use
//! this accounting.
//!
//! # Budget (clause f, amended by A2/A3)
//!
//! `enforce_budget()` and the tick pipeline sum HOT tokens with
//! unique-digest accounting and while `hot_total>budget` demote the
//! lowest-scored HOT non-pinned entry to Warm (tie-break: lower score first,
//! then `node_id` ascending). Pinned HOT entries are never budget-demoted
//! except by the pin-quota step. Demotion never deletes underlying
//! authoritative information.
//!
//! # Assembly (decision 90 B1/B2)
//!
//! HOT nodes assemble L1 summaries by default — the 4096 budget and 1024
//! pinned-HOT quota count assembled SUMMARY tokens (unique-digest). Deeper
//! levels are NEVER auto-assembled and NEVER auto-escalated; deeper content
//! is reachable only through the read-only `context.expand` tool via the
//! demand edge (decision 88 curve finding: rank-only escalation loses
//! structured depth 13/14 at every k). Each assembled HOT node carries a
//! bounded 1-hop neighbor map: at most 8 neighbors ordered by `node_id`
//! ascending, each stub carrying ONLY `node_id` + `content_digest` (never
//! content), counted honestly via the same `estimate_tokens` estimator with
//! unique-digest dedup, without changing tier membership of neighbors.
//!
//! Processing order (pinned): tick pipeline per decision 89
//! (coalescing, stale demotion, demand updates, recompute, promotion gate,
//! pin quota, budget) -> assembly: hot nodes sorted canonically -> per node:
//! L1 summary contribution + neighbor stubs (B1/B2) -> budget enforcement
//! over the assembled set.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

/// Maximum number of scheduler nodes.
pub const MAX_SCHEDULER_NODES: usize = 512;
/// Default hot threshold in relevance units (human-readable).
pub const DEFAULT_HOT_THRESHOLD: u8 = 70;
/// Default warm threshold in relevance units (human-readable).
pub const DEFAULT_WARM_THRESHOLD: u8 = 30;
/// Default working-set budget in tokens.
pub const DEFAULT_BUDGET_TOKENS: usize = 4096;
/// Pinned HOT quota (decision 89 A3).
pub const PINNED_HOT_BUDGET_TOKENS: usize = 1024;
/// Maximum canonical events per TickInput (A1).
pub const MAX_TICK_EVENTS: usize = 64;

/// Score at or above which an entry is `Hot`.
const HOT_SCORE_THRESHOLD: u64 = 280;
/// Score at or above which an entry is `Warm` (below `Hot`).
const WARM_SCORE_THRESHOLD: u64 = 120;

/// Working-set tier, ordered `Hot < Warm < Cold < Archive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WorkingSetTier {
    /// Hot — in working set.
    Hot,
    /// Warm — near working set.
    Warm,
    /// Cold — out of working set, still retained.
    Cold,
    /// Archive — explicit-only, never automatic.
    Archive,
}

impl WorkingSetTier {
    /// Canonical string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hot => "hot",
            Self::Warm => "warm",
            Self::Cold => "cold",
            Self::Archive => "archive",
        }
    }

    fn order(self) -> u8 {
        match self {
            Self::Hot => 0,
            Self::Warm => 1,
            Self::Cold => 2,
            Self::Archive => 3,
        }
    }
}

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

/// One working-set entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerEntry {
    /// Unique node id.
    pub node_id: String,
    /// Current tier.
    pub tier: WorkingSetTier,
    /// Explicit pin — forces `Hot` except for staleness demotion and budget
    /// (budget never demotes pinned; staleness demotes even pinned).
    pub pinned: bool,
    /// Host-computed relevance 0..=100.
    pub relevance: u8,
    /// Tick when last accessed.
    pub last_access_tick: u64,
    /// Token estimate for budgeting.
    pub token_estimate: usize,
    /// Content digest for unique-digest budget (A2). Empty means unique per
    /// node (backwards compat); non-empty is canonical 64-hex digest.
    pub content_digest: String,
}

/// Canonical single-node demand event (A1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccessEvent {
    /// Target node id.
    pub node_id: String,
}

impl AccessEvent {
    /// Create.
    #[must_use]
    pub fn new(node_id: impl Into<String>) -> Self {
        Self { node_id: node_id.into() }
    }
}

/// Canonicalize events: dedupe by node_id (one per node), order by node_id
/// ascending, truncate to first 64 (A1).
#[must_use]
pub fn canonicalize_events(events: Vec<AccessEvent>) -> Vec<AccessEvent> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut uniq: Vec<AccessEvent> = Vec::new();
    for ev in events {
        if seen.insert(ev.node_id.clone()) {
            uniq.push(ev);
        }
    }
    uniq.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    if uniq.len() > MAX_TICK_EVENTS {
        uniq.truncate(MAX_TICK_EVENTS);
    }
    uniq
}

/// Host-owned TickInput for the deterministic pipeline (A1, A4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickInput {
    /// Deterministic clock (now).
    pub now: u64,
    /// Canonical events (<=64, deduped, sorted).
    pub events: Vec<AccessEvent>,
    /// Graph revision digest (for coalescing).
    pub graph_revision: String,
    /// Canonical new-node set (sorted, deduped).
    pub new_node_ids: Vec<String>,
    /// Stale node ids for this tick (any tier).
    pub stale_node_ids: Vec<String>,
}

impl TickInput {
    /// Build and canonicalize events/new-nodes. Stale is stored canonical
    /// (sorted deduped) for deterministic ordering but not truncated.
    #[must_use]
    pub fn new(
        now: u64,
        events: Vec<AccessEvent>,
        graph_revision: impl Into<String>,
        new_node_ids: Vec<String>,
        stale_node_ids: Vec<String>,
    ) -> Self {
        let canonical_events = canonicalize_events(events);
        let new_nodes_sorted: Vec<String> = {
            let mut seen = BTreeSet::new();
            let mut v = Vec::new();
            for id in new_node_ids {
                if seen.insert(id.clone()) {
                    v.push(id);
                }
            }
            v.sort();
            v
        };
        // stale canonical: dedupe + sort
        let stale_sorted: Vec<String> = {
            let mut seen = BTreeSet::new();
            let mut v = Vec::new();
            for id in stale_node_ids {
                if seen.insert(id.clone()) {
                    v.push(id);
                }
            }
            v.sort();
            v
        };
        Self {
            now,
            events: canonical_events,
            graph_revision: graph_revision.into(),
            new_node_ids: new_nodes_sorted,
            stale_node_ids: stale_sorted,
        }
    }

    /// Empty input helper (no events, no graph delta).
    #[must_use]
    pub fn empty(now: u64, graph_revision: impl Into<String>) -> Self {
        Self::new(now, vec![], graph_revision, vec![], vec![])
    }
}

/// Validated working-set state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingSetState {
    entries: Vec<SchedulerEntry>,
    tick: u64,
    tick_view: TickView,
    // A4 coalescing state
    last_graph_revision: Option<String>,
    last_new_nodes: Vec<String>,
    pipeline_runs: usize,
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
        })
    }

    /// Entries in canonical order.
    #[must_use]
    pub fn entries(&self) -> &[SchedulerEntry] {
        &self.entries
    }

    /// Current tick value (via `TickRead::tick`).
    #[must_use]
    pub fn tick_value(&self) -> u64 {
        self.tick
    }

    /// Pipeline runs observable for A4 coalescing tests.
    #[must_use]
    pub fn pipeline_runs(&self) -> usize {
        self.pipeline_runs
    }

    /// One entry by id.
    #[must_use]
    pub fn entry(&self, node_id: &str) -> Option<&SchedulerEntry> {
        self.entries.iter().find(|e| e.node_id == node_id)
    }

    fn entry_mut(&mut self, node_id: &str) -> Option<&mut SchedulerEntry> {
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
    pub fn advance_tick(&mut self, _config: &SchedulerConfig) -> TickReport {
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
    pub fn process_tick(
        &mut self,
        input: TickInput,
        config: &SchedulerConfig,
    ) -> TickReport {
        // 1. Coalescing guard: empty events AND no graph deltas since last processed tick.
        let is_noop = input.events.is_empty()
            && self.last_graph_revision.as_deref()
                == Some(input.graph_revision.as_str())
            && self.last_new_nodes == input.new_node_ids;
        if is_noop {
            // Identical output without pipeline run.
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

    #[allow(dead_code)]
    fn hot_total_legacy(
        &self,
        map: &BTreeMap<String, usize>,
    ) -> Result<usize, SchedulerError> {
        let mut total: usize = 0;
        for e in &self.entries {
            if e.tier == WorkingSetTier::Hot {
                let tok = map.get(&e.node_id).ok_or_else(|| {
                    SchedulerError::UnknownNode { node_id: e.node_id.clone() }
                })?;
                total = total.saturating_add(*tok);
            }
        }
        Ok(total)
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

/// In-turn scheduler event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerEvent {
    /// Access updates recency and re-tiers.
    Access {
        /// Node id.
        node_id: String,
    },
    /// Pin forces `Hot`.
    Pin {
        /// Node id.
        node_id: String,
    },
    /// Unpin clears pin and re-tiers.
    Unpin {
        /// Node id.
        node_id: String,
    },
    /// Relevance update (0..=100) and re-tier.
    Relevance {
        /// Node id.
        node_id: String,
        /// New relevance.
        relevance: u8,
    },
    /// Stale content demotes one tier toward Cold, even if pinned.
    Stale {
        /// Node id.
        node_id: String,
    },
    /// Explicit-only archive move.
    Archive {
        /// Node id.
        node_id: String,
    },
}

impl SchedulerEvent {
    fn node_id(&self) -> &str {
        match self {
            Self::Access { node_id }
            | Self::Pin { node_id }
            | Self::Unpin { node_id }
            | Self::Relevance { node_id, .. }
            | Self::Stale { node_id }
            | Self::Archive { node_id } => node_id,
        }
    }
}

/// Outcome of one event application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerOutcome {
    /// Node id.
    pub node_id: String,
    /// Tier before event.
    pub tier_before: WorkingSetTier,
    /// Tier after event.
    pub tier_after: WorkingSetTier,
}

/// Report from one tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// New tick.
    pub tick: u64,
    /// Promoted node ids (toward Hot), canonical order.
    pub promoted: Vec<String>,
    /// Demoted node ids (toward Cold), canonical order.
    pub demoted: Vec<String>,
}

/// Report from budget enforcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetReport {
    /// Hot tokens before demotion.
    pub hot_tokens_before: usize,
    /// Hot tokens after demotion.
    pub hot_tokens_after: usize,
    /// Demoted node ids, canonical order.
    pub demoted: Vec<String>,
}

/// Scheduler error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerError {
    /// Unknown node id.
    UnknownNode {
        /// Node id.
        node_id: String,
    },
    /// Duplicate node id.
    DuplicateNode {
        /// Node id.
        node_id: String,
    },
    /// Invalid configuration.
    InvalidConfig {
        /// Reason.
        reason: String,
    },
    /// Relevance out of 0..=100.
    RelevanceOutOfRange {
        /// Node id.
        node_id: String,
        /// Value.
        value: u8,
    },
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownNode { node_id } => {
                write!(f, "unknown node: {node_id}")
            }
            Self::DuplicateNode { node_id } => {
                write!(f, "duplicate node: {node_id}")
            }
            Self::InvalidConfig { reason } => {
                write!(f, "invalid config: {reason}")
            }
            Self::RelevanceOutOfRange { node_id, value } => {
                write!(f, "relevance out of range: {node_id} {value}")
            }
        }
    }
}

impl std::error::Error for SchedulerError {}

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

fn tier_for_score(s: u64) -> WorkingSetTier {
    if s >= HOT_SCORE_THRESHOLD {
        WorkingSetTier::Hot
    } else if s >= WARM_SCORE_THRESHOLD {
        WorkingSetTier::Warm
    } else {
        WorkingSetTier::Cold
    }
}

fn hot_unique_total(state: &WorkingSetState) -> usize {
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

fn pinned_hot_unique_total(state: &WorkingSetState) -> usize {
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

fn stub_token_estimate(stub: &NeighborStub) -> usize {
    // Honest byte estimate via the same estimator, no tool-call overhead.
    // Concatenate node_id + digest with a separator.
    crate::context_graph::estimate_tokens(&format!(
        "{}:{}",
        stub.node_id, stub.content_digest
    ))
}

fn assembled_unique_total(entries: &[AssembledEntry]) -> usize {
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

        // 7. Budget enforcement over assembled set (4096) — lowest non-pinned HOT first
        loop {
            let total = assembled_unique_total(&entries);
            if total <= config.budget_tokens {
                break;
            }
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
}

#[cfg(test)]
mod tests {
    use super::{
        AccessEvent, SchedulerConfig, SchedulerEntry, SchedulerError,
        SchedulerEvent, TickInput, WorkingSetState, WorkingSetTier,
        canonicalize_events, hot_unique_total, score,
    };

    fn entry(
        id: &str,
        tier: WorkingSetTier,
        pinned: bool,
        relevance: u8,
        last_access: u64,
        tokens: usize,
    ) -> SchedulerEntry {
        SchedulerEntry {
            node_id: id.to_owned(),
            tier,
            pinned,
            relevance,
            last_access_tick: last_access,
            token_estimate: tokens,
            content_digest: String::new(),
        }
    }

    fn entry_with_digest(
        id: &str,
        tier: WorkingSetTier,
        pinned: bool,
        relevance: u8,
        last_access: u64,
        tokens: usize,
        digest: &str,
    ) -> SchedulerEntry {
        SchedulerEntry {
            node_id: id.to_owned(),
            tier,
            pinned,
            relevance,
            last_access_tick: last_access,
            token_estimate: tokens,
            content_digest: digest.to_owned(),
        }
    }

    #[test]
    fn build_canonical_order() {
        let state = WorkingSetState::build(vec![
            entry("b", WorkingSetTier::Cold, false, 10, 0, 0),
            entry("a", WorkingSetTier::Warm, false, 20, 0, 0),
        ])
        .expect("build");
        assert_eq!(
            state
                .entries()
                .iter()
                .map(|e| e.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn build_duplicate_refusal() {
        let err = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Cold, false, 10, 0, 0),
            entry("a", WorkingSetTier::Warm, false, 20, 0, 0),
        ])
        .unwrap_err();
        assert_eq!(
            err,
            SchedulerError::DuplicateNode { node_id: "a".to_owned() }
        );
    }

    #[test]
    fn relevance_out_of_range_refusal_on_build() {
        let err = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            101,
            0,
            0,
        )])
        .unwrap_err();
        assert_eq!(
            err,
            SchedulerError::RelevanceOutOfRange {
                node_id: "a".to_owned(),
                value: 101
            }
        );
    }

    #[test]
    fn pin_forces_hot() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            10,
            0,
            0,
        )])
        .expect("build");
        let outcome = state
            .apply_event(SchedulerEvent::Pin { node_id: "a".to_owned() })
            .expect("pin");
        assert_eq!(outcome.tier_before, WorkingSetTier::Cold);
        assert_eq!(outcome.tier_after, WorkingSetTier::Hot);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
        assert!(state.entry("a").unwrap().pinned);
    }

    #[test]
    fn access_updates_recency() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            10,
            0,
            0,
        )])
        .expect("build");
        // Advance tick so recency would be low without access.
        let cfg = SchedulerConfig::default();
        for _ in 0..5 {
            state.advance_tick(&cfg);
        }
        assert!(state.tick_value() == 5);
        // Access at tick 5: last_access becomes 5, score improves.
        let outcome = state
            .apply_event(SchedulerEvent::Access { node_id: "a".to_owned() })
            .expect("access");
        assert_eq!(state.entry("a").unwrap().last_access_tick, 5);
        // With relevance 10 (40) + recency 30 + pin 0 =70 <120 still Cold, so stays Cold.
        assert_eq!(outcome.tier_after, WorkingSetTier::Cold);
        // Now set high relevance then access should promote.
        state
            .apply_event(SchedulerEvent::Relevance {
                node_id: "a".to_owned(),
                relevance: 80,
            })
            .expect("relevance");
        // score 80*4=320 +30 =350 => Hot
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
    }

    #[test]
    fn relevance_out_of_range_via_event() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            10,
            0,
            0,
        )])
        .expect("build");
        let err = state
            .apply_event(SchedulerEvent::Relevance {
                node_id: "a".to_owned(),
                relevance: 200,
            })
            .unwrap_err();
        assert_eq!(
            err,
            SchedulerError::RelevanceOutOfRange {
                node_id: "a".to_owned(),
                value: 200
            }
        );
    }

    #[test]
    fn tick_promotion_after_access() {
        // Low relevance Cold node becomes Hot after access + high relevance.
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            90,
            0,
            0,
        )])
        .expect("build");
        // Immediately after build, tick 0: score 90*4=360+30=390 => Hot, but entry is Cold.
        // Tick will promote.
        let cfg = SchedulerConfig::default();
        let report = state.advance_tick(&cfg);
        assert_eq!(report.tick, 1);
        assert_eq!(report.promoted, vec!["a".to_owned()]);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
    }

    #[test]
    fn tick_demotion_neglect_to_cold() {
        // Start Hot with modest relevance, let recency decay.
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Hot,
            false,
            30,
            0,
            0,
        )])
        .expect("build");
        // Tick 0 score 30*4=120+30=150 => Warm, so first tick demotes Hot->Warm.
        let cfg = SchedulerConfig::default();
        let r1 = state.advance_tick(&cfg);
        assert_eq!(r1.demoted, vec!["a".to_owned()]);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
        // Neglect 30 ticks -> recency 0 => score 120 => still Warm.
        for _ in 0..30 {
            state.advance_tick(&cfg);
        }
        // Score 120 => Warm. One more tick with relevance low? Actually need Cold.
        // Lower relevance to 10 => score 40+0=40 => Cold.
        state
            .apply_event(SchedulerEvent::Relevance {
                node_id: "a".to_owned(),
                relevance: 10,
            })
            .expect("relevance");
        let _ = state.advance_tick(&cfg);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Cold);
    }

    #[test]
    fn stale_beats_pin() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Hot,
            true,
            90,
            0,
            0,
        )])
        .expect("build");
        let outcome = state
            .apply_event(SchedulerEvent::Stale { node_id: "a".to_owned() })
            .expect("stale");
        assert_eq!(outcome.tier_after, WorkingSetTier::Warm);
        assert!(state.entry("a").unwrap().pinned);
    }

    #[test]
    fn archive_explicit_only() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Hot,
            false,
            90,
            0,
            0,
        )])
        .expect("build");
        let cfg = SchedulerConfig::default();
        let _ = state.advance_tick(&cfg);
        assert_ne!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
        state
            .apply_event(SchedulerEvent::Archive { node_id: "a".to_owned() })
            .expect("archive");
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
        let _ = state.advance_tick(&cfg);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
    }

    #[test]
    fn budget_lowest_score_demoted_first() {
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, false, 10, 0, 100),
            entry("b", WorkingSetTier::Hot, false, 10, 0, 100),
            entry("c", WorkingSetTier::Hot, false, 90, 0, 100),
        ])
        .expect("build");
        let cfg = SchedulerConfig::new(150).expect("cfg");
        let estimates = vec![
            ("a".to_owned(), 100),
            ("b".to_owned(), 100),
            ("c".to_owned(), 100),
        ];
        let report = state.enforce_budget(&cfg, &estimates).expect("budget");
        // Hot total 300 >150, demote lowest score first: a (110, id a) then b (110).
        // After demoting a: 200 >150 demote b: 100 <=150 stop.
        assert_eq!(report.hot_tokens_before, 300);
        assert_eq!(report.hot_tokens_after, 100);
        assert_eq!(report.demoted, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
        assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Warm);
        assert_eq!(state.entry("c").unwrap().tier, WorkingSetTier::Hot);
    }

    #[test]
    fn pinned_never_budget_demoted() {
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, true, 10, 0, 100),
            entry("b", WorkingSetTier::Hot, false, 90, 0, 100),
        ])
        .expect("build");
        let cfg = SchedulerConfig::new(150).expect("cfg");
        let estimates = vec![("a".to_owned(), 100), ("b".to_owned(), 100)];
        let report = state.enforce_budget(&cfg, &estimates).expect("budget");
        // a is pinned, so only b can be demoted even though a has lower score (10*4+30+50=120).
        assert_eq!(report.demoted, vec!["b".to_owned()]);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
        assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Warm);
    }

    #[test]
    fn budget_boundary_no_demotions() {
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, false, 90, 0, 100),
            entry("b", WorkingSetTier::Hot, false, 90, 0, 100),
        ])
        .expect("build");
        let cfg = SchedulerConfig::new(200).expect("cfg");
        let estimates = vec![("a".to_owned(), 100), ("b".to_owned(), 100)];
        let report = state.enforce_budget(&cfg, &estimates).expect("budget");
        assert_eq!(report.hot_tokens_before, 200);
        assert_eq!(report.hot_tokens_after, 200);
        assert!(report.demoted.is_empty());
    }

    #[test]
    fn report_ordering_canonical() {
        let mut state = WorkingSetState::build(vec![
            entry("c", WorkingSetTier::Cold, false, 90, 0, 0),
            entry("a", WorkingSetTier::Cold, false, 90, 0, 0),
            entry("b", WorkingSetTier::Hot, false, 10, 0, 0),
        ])
        .expect("build");
        let cfg = SchedulerConfig::default();
        let report = state.advance_tick(&cfg);
        // a and c should promote, in canonical order.
        assert_eq!(report.promoted, vec!["a".to_owned(), "c".to_owned()]);
        assert_eq!(report.demoted, vec!["b".to_owned()]);
    }

    #[test]
    fn config_validation() {
        assert!(SchedulerConfig::new(0).is_err());
        assert_eq!(
            SchedulerConfig::new(0).unwrap_err(),
            SchedulerError::InvalidConfig {
                reason: "budget_tokens must be >= 1".to_owned()
            }
        );
        assert!(SchedulerConfig::new(1).is_ok());
        assert_eq!(SchedulerConfig::default().budget_tokens, 4096);
    }

    #[test]
    fn score_deterministic() {
        let e = entry("x", WorkingSetTier::Cold, false, 50, 5, 0);
        // tick 5 => recency 30, pinned 0 => 200+30=230
        assert_eq!(score(&e, 5), 230);
        let pinned = entry("x", WorkingSetTier::Cold, true, 50, 5, 0);
        assert_eq!(score(&pinned, 5), 280);
        // tick 10 => ticks_since 5 => 25 => 200+25=225
        assert_eq!(score(&e, 10), 225);
        // tick 40 => ticks_since 35 => capped 30 => 0 => 200
        assert_eq!(score(&e, 40), 200);
    }

    #[test]
    fn unknown_node_error() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            10,
            0,
            0,
        )])
        .expect("build");
        let err = state
            .apply_event(SchedulerEvent::Access {
                node_id: "missing".to_owned(),
            })
            .unwrap_err();
        assert_eq!(
            err,
            SchedulerError::UnknownNode { node_id: "missing".to_owned() }
        );
        let cfg = SchedulerConfig::default();
        let est = vec![("missing".to_owned(), 10)];
        let err2 = state.enforce_budget(&cfg, &est).unwrap_err();
        assert_eq!(
            err2,
            SchedulerError::UnknownNode { node_id: "missing".to_owned() }
        );
    }

    // --- A1 canonical event dedupe/order/truncate ---
    #[test]
    fn canonical_event_dedupe_order_truncate() {
        let events = vec![
            AccessEvent::new("b"),
            AccessEvent::new("a"),
            AccessEvent::new("b"),
            AccessEvent::new("c"),
        ];
        let canon = canonicalize_events(events);
        assert_eq!(
            canon,
            vec![
                AccessEvent::new("a"),
                AccessEvent::new("b"),
                AccessEvent::new("c")
            ]
        );
        // Truncate 64
        let many: Vec<AccessEvent> =
            (0..70).map(|i| AccessEvent::new(format!("n{:03}", i))).collect();
        // Already sorted, should truncate to 64
        let canon_many = canonicalize_events(many);
        assert_eq!(canon_many.len(), 64);
        assert_eq!(canon_many[0].node_id, "n000");
        assert_eq!(canon_many[63].node_id, "n063");
        // Unsorted many with duplicates
        let unsorted = vec![
            AccessEvent::new("z"),
            AccessEvent::new("a"),
            AccessEvent::new("m"),
            AccessEvent::new("a"),
        ];
        assert_eq!(
            canonicalize_events(unsorted),
            vec![
                AccessEvent::new("a"),
                AccessEvent::new("m"),
                AccessEvent::new("z")
            ]
        );
    }

    #[test]
    fn tick_input_canonicalizes_events() {
        let input = TickInput::new(
            10,
            vec![
                AccessEvent::new("b"),
                AccessEvent::new("a"),
                AccessEvent::new("b"),
            ],
            "rev1",
            vec!["n2".to_owned(), "n1".to_owned()],
            vec![],
        );
        assert_eq!(
            input.events,
            vec![AccessEvent::new("a"), AccessEvent::new("b")]
        );
        assert_eq!(input.new_node_ids, vec!["n1".to_owned(), "n2".to_owned()]);
    }

    // --- A1 demand edge updates recency/relevance with saturation ---
    #[test]
    fn demand_edge_updates_recency_and_relevance_saturating() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            90,
            0,
            100,
        )])
        .expect("build");
        let cfg = SchedulerConfig::default();
        let input = TickInput::new(
            5,
            vec![AccessEvent::new("a")],
            "rev1",
            vec![],
            vec![],
        );
        let _ = state.process_tick(input, &cfg);
        // relevance 90+32 capped 100, last_access 5
        let e = state.entry("a").unwrap();
        assert_eq!(e.relevance, 100);
        assert_eq!(e.last_access_tick, 5);
        // Second event should stay 100
        let input2 = TickInput::new(
            6,
            vec![AccessEvent::new("a")],
            "rev2",
            vec![],
            vec![],
        );
        let _ = state.process_tick(input2, &cfg);
        assert_eq!(state.entry("a").unwrap().relevance, 100);
        assert_eq!(state.entry("a").unwrap().last_access_tick, 6);
    }

    // --- A3 stale-never-promote ---
    #[test]
    fn stale_never_promote_but_updates_scores() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Warm,
            false,
            90,
            0,
            100,
        )])
        .expect("build");
        // Make stale; even with demand event that would promote to Hot, it stays Warm
        let cfg = SchedulerConfig::default();
        // Warm with relevance 90, tick 0 score 90*4+30=390 => Hot, but we will mark stale for next tick
        // Process tick with stale flag and demand event
        let input = TickInput::new(
            1,
            vec![AccessEvent::new("a")],
            "rev1",
            vec![],
            vec!["a".to_owned()],
        );
        let _ = state.process_tick(input, &cfg);
        // Stale demotes Warm->Cold first, then demand updates relevance 90->100 and recency, but promotion gate refuses stale promotion
        // So final should be Cold, not Hot
        let e = state.entry("a").unwrap();
        // relevance should have been updated despite staleness
        assert_eq!(e.relevance, 100);
        assert_eq!(e.last_access_tick, 1);
        assert_eq!(
            e.tier,
            WorkingSetTier::Cold,
            "stale node must not promote to Hot even with high score"
        );
    }

    // --- A2 digest-counted budget ---
    #[test]
    fn digest_counted_budget_counts_unique_once() {
        let digest_dup = "aabbcc".repeat(10) + "00";
        let digest_unique = "ff".repeat(32);
        let mut state = WorkingSetState::build(vec![
            entry_with_digest(
                "a",
                WorkingSetTier::Hot,
                false,
                10,
                0,
                1000,
                &digest_dup,
            ),
            entry_with_digest(
                "b",
                WorkingSetTier::Hot,
                false,
                10,
                0,
                1000,
                &digest_dup,
            ),
            entry_with_digest(
                "c",
                WorkingSetTier::Hot,
                false,
                10,
                0,
                1000,
                &digest_unique,
            ),
        ])
        .expect("build");
        // Hot unique total should be 2000 (a first 1000 + c 1000, b 0 duplicate)
        assert_eq!(hot_unique_total(&state), 2000);
        // With budget 1500, demotion should demote lowest scored (a,b tie a first) but removing a won't reduce total if b still Hot with same digest
        // So after demoting a, total still 2000 (b becomes first of dup, plus c), need to demote b as well to get to 1000
        let cfg = SchedulerConfig::new(1500).expect("cfg");
        let estimates = vec![
            ("a".to_owned(), 1000),
            ("b".to_owned(), 1000),
            ("c".to_owned(), 1000),
        ];
        let report = state.enforce_budget(&cfg, &estimates).expect("budget");
        assert_eq!(report.hot_tokens_before, 2000);
        // Should demote a and b (both dup digest) to reach 1000
        assert_eq!(report.demoted, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
        assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Warm);
        assert_eq!(state.entry("c").unwrap().tier, WorkingSetTier::Hot);
        assert_eq!(
            state
                .enforce_budget(
                    &SchedulerConfig::new(1500).unwrap(),
                    &estimates
                )
                .unwrap()
                .hot_tokens_after,
            1000
        );
    }

    #[test]
    fn budget_demotion_order_under_duplicates() {
        let d1 = "11".repeat(32);
        let d2 = "22".repeat(32);
        // a and b share d1, c has d2. Scores: a low (10), b mid (20), c high (90). Hot all.
        let mut state = WorkingSetState::build(vec![
            entry_with_digest(
                "a",
                WorkingSetTier::Hot,
                false,
                10,
                0,
                1000,
                &d1,
            ),
            entry_with_digest(
                "b",
                WorkingSetTier::Hot,
                false,
                20,
                0,
                1000,
                &d1,
            ),
            entry_with_digest(
                "c",
                WorkingSetTier::Hot,
                false,
                90,
                0,
                1000,
                &d2,
            ),
        ])
        .expect("build");
        let cfg = SchedulerConfig::new(1500).unwrap();
        let est = vec![
            ("a".to_owned(), 1000),
            ("b".to_owned(), 1000),
            ("c".to_owned(), 1000),
        ];
        let report = state.enforce_budget(&cfg, &est).unwrap();
        // Unique total 2000 -> need demote. Lowest score is a (10) -> demote a (still 2000 because b still holds d1), then next lowest is b -> demote b -> total 1000
        assert_eq!(report.demoted, vec!["a".to_owned(), "b".to_owned()]);
    }

    // --- A3 pin quota ---
    #[test]
    fn pin_quota_demotes_lowest_pinned_hot() {
        // Two pinned HOT nodes each 800 tokens, duplicate? unique 1600 >1024 quota -> demote lowest scored pinned
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, true, 10, 0, 800),
            entry("b", WorkingSetTier::Hot, true, 90, 0, 800),
            entry("c", WorkingSetTier::Hot, false, 90, 0, 800),
        ])
        .expect("build");
        let cfg = SchedulerConfig::default();
        // tick with no events but same revision will trigger pin quota via process_tick
        // Use different revision to force pipeline
        let input = TickInput::new(1, vec![], "rev1", vec![], vec![]);
        let report = state.process_tick(input, &cfg);
        // Pinned hot total 1600 >1024, lowest scored pinned is a -> demoted to Warm
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
        assert_eq!(state.entry("b").unwrap().tier, WorkingSetTier::Hot);
        // Budget total after pin quota: b(800) + c(800) =1600 <=4096 so no further demotion
        assert!(
            report.demoted.contains(&"a".to_owned())
                || state.entry("a").unwrap().tier == WorkingSetTier::Warm
        );
    }

    #[test]
    fn pin_quota_within_survives_budget() {
        // Pinned HOT within quota should survive budget demotion even if lowest score
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, true, 10, 0, 500),
            entry("b", WorkingSetTier::Hot, false, 90, 0, 2000),
            entry("c", WorkingSetTier::Hot, false, 90, 0, 2000),
        ])
        .expect("build");
        // Pinned total 500 <=1024, so pin quota no demotion. Budget 4096: total unique 4500 >4096, need demote lowest non-pinned (b and c both 90, tie a would be lowest but pinned protects, so b demoted)
        let cfg = SchedulerConfig::new(4096).unwrap();
        let est = vec![
            ("a".to_owned(), 500),
            ("b".to_owned(), 2000),
            ("c".to_owned(), 2000),
        ];
        let report = state.enforce_budget(&cfg, &est).unwrap();
        assert_eq!(report.demoted, vec!["b".to_owned()]);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Hot);
    }

    // --- A4 coalescing ---
    #[test]
    fn tick_coalescing_identical_input_no_pipeline() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Cold,
            false,
            10,
            0,
            100,
        )])
        .expect("build");
        let cfg = SchedulerConfig::default();
        let input =
            TickInput::new(1, vec![], "rev1", vec!["n1".to_owned()], vec![]);
        let before_runs = state.pipeline_runs();
        let _report1 = state.process_tick(input.clone(), &cfg);
        let after_first = state.pipeline_runs();
        assert_eq!(after_first, before_runs + 1);
        let state_after_first = state.clone();
        // Identical input should coalesce
        let report2 = state.process_tick(input.clone(), &cfg);
        assert_eq!(
            state, state_after_first,
            "coalesced tick must produce identical output state"
        );
        assert_eq!(
            state.pipeline_runs(),
            after_first,
            "no pipeline run on coalesced tick"
        );
        assert_eq!(report2.promoted.len(), 0);
        assert_eq!(report2.demoted.len(), 0);
        assert_eq!(report2.tick, state_after_first.tick_value());
        // Different graph revision should not coalesce
        let input_diff =
            TickInput::new(1, vec![], "rev2", vec!["n1".to_owned()], vec![]);
        let _ = state.process_tick(input_diff, &cfg);
        assert_eq!(state.pipeline_runs(), after_first + 1);
    }

    // --- processing order property ---
    #[test]
    fn processing_order_stale_before_demand_and_promotion() {
        // Stale Warm node with demand event that would otherwise promote, but stale never promotes
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Warm,
            false,
            80,
            0,
            100,
        )])
        .expect("build");
        let cfg = SchedulerConfig::default();
        // Tick 0: Warm 80*4+30=350 Hot promotion would happen, but we mark stale for tick 1 with demand
        let input = TickInput::new(
            1,
            vec![AccessEvent::new("a")],
            "rev1",
            vec![],
            vec!["a".to_owned()],
        );
        let _ = state.process_tick(input, &cfg);
        // Stale demotion Warm->Cold, demand updates relevance, but promotion refused -> stays Cold
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Cold);
        assert_eq!(state.entry("a").unwrap().relevance, 100);
    }

    // --- determinism byte-equal ---
    #[test]
    fn determinism_byte_equal() {
        let build = || {
            WorkingSetState::build(vec![
                entry("b", WorkingSetTier::Cold, false, 20, 0, 100),
                entry("a", WorkingSetTier::Hot, true, 10, 0, 100),
            ])
            .unwrap()
        };
        let mut s1 = build();
        let mut s2 = build();
        let cfg = SchedulerConfig::default();
        let input = TickInput::new(
            1,
            vec![AccessEvent::new("b")],
            "revX",
            vec![],
            vec![],
        );
        let r1 = s1.process_tick(input.clone(), &cfg);
        let r2 = s2.process_tick(input.clone(), &cfg);
        assert_eq!(s1, s2);
        assert_eq!(r1, r2);
        // Same input again (coalesced) still byte-equal
        let r1b = s1.process_tick(input.clone(), &cfg);
        let r2b = s2.process_tick(input.clone(), &cfg);
        assert_eq!(r1b, r2b);
        assert_eq!(s1, s2);
    }

    // --- Decision 90 B1/B2 assembly ---
    fn assembly_fixture() -> (
        WorkingSetState,
        crate::context_graph::ContextGraph,
        crate::context_representation::ContextRepresentationStore,
    ) {
        use crate::context_graph::{
            ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
            ContextNodeKind, estimate_tokens,
        };
        use crate::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        use crate::identity::sha256_hex;
        let a_digest = sha256_hex("body-a".as_bytes());
        let b_digest = sha256_hex("body-b".as_bytes());
        let nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: a_digest.clone(),
                summary: "summary-a".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-a"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: b_digest.clone(),
                summary: "summary-b".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-b"),
            },
            ContextNode {
                id: "c".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: sha256_hex("body-c".as_bytes()),
                summary: "summary-c".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-c"),
            },
        ];
        let edges = vec![
            ContextEdge {
                from: "a".to_owned(),
                to: "b".to_owned(),
                kind: ContextEdgeKind::Contains,
            },
            ContextEdge {
                from: "b".to_owned(),
                to: "c".to_owned(),
                kind: ContextEdgeKind::DependsOn,
            },
        ];
        let graph = ContextGraph::build(nodes, edges).expect("graph");
        let rep_a_summary = "summary-a";
        let rep_a_structured = "structured-facts-a";
        let rep_b_summary = "summary-b";
        let rep_c_summary = "summary-c";
        let set_a = NodeRepresentationSet::build(
            "a".to_owned(),
            vec![
                NodeRepresentation {
                    level: RepresentationLevel::Identity,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of("identity-a"),
                    derived_from: vec![],
                    content: "identity-a".to_owned(),
                },
                NodeRepresentation {
                    level: RepresentationLevel::Summary,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(rep_a_summary),
                    derived_from: vec![],
                    content: rep_a_summary.to_owned(),
                },
                NodeRepresentation {
                    level: RepresentationLevel::Structured,
                    origin: RepresentationOrigin::HostExtracted,
                    content_digest: content_digest_of(rep_a_structured),
                    derived_from: vec![],
                    content: rep_a_structured.to_owned(),
                },
            ],
        )
        .expect("set_a");
        let set_b = NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(rep_b_summary),
                derived_from: vec![],
                content: rep_b_summary.to_owned(),
            }],
        )
        .expect("set_b");
        let set_c = NodeRepresentationSet::build(
            "c".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of(rep_c_summary),
                derived_from: vec![],
                content: rep_c_summary.to_owned(),
            }],
        )
        .expect("set_c");
        let store =
            ContextRepresentationStore::build(vec![set_a, set_b, set_c])
                .expect("store");
        let entries = vec![
            entry("a", WorkingSetTier::Hot, false, 90, 0, 1000),
            entry("b", WorkingSetTier::Hot, false, 90, 0, 1000),
            entry("c", WorkingSetTier::Cold, false, 10, 0, 1000),
        ];
        let state = WorkingSetState::build(entries).expect("state");
        (state, graph, store)
    }

    #[test]
    fn l1_default_assembly_hot_contributes_summary_only() {
        let (mut state, graph, store) = assembly_fixture();
        let cfg = SchedulerConfig::default();
        let ctx = state.assemble(&graph, &store, &cfg);
        assert_eq!(ctx.entries.len(), 2);
        let a_entry =
            ctx.entries.iter().find(|e| e.node_id == "a").expect("a");
        assert_eq!(a_entry.summary, "summary-a");
        assert_eq!(
            a_entry.token_estimate,
            crate::context_graph::estimate_tokens("summary-a")
        );
        assert!(!a_entry.summary.contains("structured"));
        for e in &ctx.entries {
            for stub in &e.neighbor_stubs {
                assert!(!stub.content_digest.is_empty());
                assert_ne!(stub.content_digest, e.summary);
            }
        }
    }

    #[test]
    fn budget_counts_assembled_summaries_unique_digest() {
        use crate::context_graph::{
            ContextGraph, ContextNode, ContextNodeKind, estimate_tokens,
        };
        use crate::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        let digest = content_digest_of("same-summary");
        let nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "a".repeat(64),
                summary: "same-summary".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("same-summary"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "b".repeat(64),
                summary: "same-summary".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("same-summary"),
            },
        ];
        let graph = ContextGraph::build(nodes, vec![]).expect("graph");
        let set_a = NodeRepresentationSet::build(
            "a".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: digest.clone(),
                derived_from: vec![],
                content: "same-summary".to_owned(),
            }],
        )
        .expect("set_a");
        let set_b = NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: digest.clone(),
                derived_from: vec![],
                content: "same-summary".to_owned(),
            }],
        )
        .expect("set_b");
        let store = ContextRepresentationStore::build(vec![set_a, set_b])
            .expect("store");
        let tok = estimate_tokens("same-summary");
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, false, 90, 0, 1000),
            entry("b", WorkingSetTier::Hot, false, 90, 0, 1000),
        ])
        .expect("state");
        let cfg = SchedulerConfig::new(tok).expect("cfg");
        let ctx = state.assemble(&graph, &store, &cfg);
        assert_eq!(ctx.entries.len(), 2);
        assert_eq!(ctx.total_tokens_after, tok);
        // Distinct digests with tight budget should demote lowest scored
        let distinct_nodes = vec![
            ContextNode {
                id: "a".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "c".repeat(64),
                summary: "summary-a-distinct".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-a-distinct"),
            },
            ContextNode {
                id: "b".to_owned(),
                kind: ContextNodeKind::Source,
                content_digest: "d".repeat(64),
                summary: "summary-b-distinct".to_owned(),
                source_bindings: vec![],
                token_estimate: estimate_tokens("summary-b-distinct"),
            },
        ];
        let distinct_graph =
            ContextGraph::build(distinct_nodes, vec![]).expect("graph");
        let d_a = NodeRepresentationSet::build(
            "a".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-a-distinct"),
                derived_from: vec![],
                content: "summary-a-distinct".to_owned(),
            }],
        )
        .expect("set");
        let d_b = NodeRepresentationSet::build(
            "b".to_owned(),
            vec![NodeRepresentation {
                level: RepresentationLevel::Summary,
                origin: RepresentationOrigin::HostExtracted,
                content_digest: content_digest_of("summary-b-distinct"),
                derived_from: vec![],
                content: "summary-b-distinct".to_owned(),
            }],
        )
        .expect("set");
        let distinct_store =
            ContextRepresentationStore::build(vec![d_a, d_b]).expect("store");
        let mut distinct_state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, false, 10, 0, 1000),
            entry("b", WorkingSetTier::Hot, false, 20, 0, 1000),
        ])
        .expect("state");
        let tok_a = estimate_tokens("summary-a-distinct");
        let cfg_tight = SchedulerConfig::new(tok_a).expect("cfg");
        let ctx2 = distinct_state.assemble(
            &distinct_graph,
            &distinct_store,
            &cfg_tight,
        );
        assert_eq!(ctx2.entries.len(), 1);
        assert_eq!(ctx2.entries[0].node_id, "b");
    }

    #[test]
    fn no_auto_escalation_demanded_node_still_summary_only() {
        let (mut state, graph, store) = assembly_fixture();
        let cfg = SchedulerConfig::default();
        let input = TickInput::new(
            10,
            vec![AccessEvent::new("a")],
            "rev1",
            vec![],
            vec![],
        );
        let _ = state.process_tick(input, &cfg);
        let ctx = state.assemble(&graph, &store, &cfg);
        let a = ctx.entries.iter().find(|e| e.node_id == "a").expect("a hot");
        assert_eq!(a.summary, "summary-a");
        assert!(!a.summary.contains("structured"));
    }

    #[test]
    fn neighbor_stubs_bounded_and_ordered_and_no_content_leak() {
        use crate::context_graph::{
            ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
            ContextNodeKind, estimate_tokens,
        };
        use crate::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        use crate::identity::sha256_hex;
        let mut nodes = vec![ContextNode {
            id: "a".to_owned(),
            kind: ContextNodeKind::Source,
            content_digest: sha256_hex("center".as_bytes()),
            summary: "center".to_owned(),
            source_bindings: vec![],
            token_estimate: estimate_tokens("center"),
        }];
        for i in 0..12 {
            let id = format!("n{i:02}");
            nodes.push(ContextNode {
                id: id.clone(),
                kind: ContextNodeKind::Source,
                content_digest: sha256_hex(id.as_bytes()),
                summary: format!("summary-{id}"),
                source_bindings: vec![],
                token_estimate: estimate_tokens(&format!("summary-{id}")),
            });
        }
        let mut edges = Vec::new();
        for i in 0..12 {
            let id = format!("n{i:02}");
            edges.push(ContextEdge {
                from: "a".to_owned(),
                to: id.clone(),
                kind: ContextEdgeKind::Contains,
            });
        }
        let graph = ContextGraph::build(nodes.clone(), edges).expect("graph");
        let mut sets = Vec::new();
        for n in &nodes {
            sets.push(
                NodeRepresentationSet::build(
                    n.id.clone(),
                    vec![NodeRepresentation {
                        level: RepresentationLevel::Summary,
                        origin: RepresentationOrigin::HostExtracted,
                        content_digest: content_digest_of(&n.summary),
                        derived_from: vec![],
                        content: n.summary.clone(),
                    }],
                )
                .expect("set"),
            );
        }
        let store = ContextRepresentationStore::build(sets).expect("store");
        let mut state = WorkingSetState::build(vec![SchedulerEntry {
            node_id: "a".to_owned(),
            tier: WorkingSetTier::Hot,
            pinned: false,
            relevance: 90,
            last_access_tick: 0,
            token_estimate: 1000,
            content_digest: sha256_hex("center".as_bytes()),
        }])
        .expect("state");
        let cfg = SchedulerConfig::default();
        let ctx = state.assemble(&graph, &store, &cfg);
        let a = ctx.entries.iter().find(|e| e.node_id == "a").expect("a");
        assert_eq!(a.neighbor_stubs.len(), 8);
        let expected: Vec<String> =
            (0..8).map(|i| format!("n{i:02}")).collect();
        let actual: Vec<String> =
            a.neighbor_stubs.iter().map(|s| s.node_id.clone()).collect();
        assert_eq!(actual, expected);
        for stub in &a.neighbor_stubs {
            assert!(stub.content_digest.len() == 64);
            let neighbor_node = graph.node(&stub.node_id).expect("node");
            assert_eq!(stub.content_digest, neighbor_node.content_digest);
            assert!(!stub.content_digest.contains("summary"));
        }
    }

    #[test]
    fn stub_bytes_counted_in_budget_demotion() {
        use crate::context_graph::{
            ContextEdge, ContextEdgeKind, ContextGraph, ContextNode,
            ContextNodeKind, estimate_tokens,
        };
        use crate::context_representation::{
            ContextRepresentationStore, NodeRepresentation,
            NodeRepresentationSet, RepresentationLevel, RepresentationOrigin,
            content_digest_of,
        };
        use crate::identity::sha256_hex;
        // 9 HOT nodes each 512 tokens (max summary) => 4608 >4096 triggers demotion; stubs add further
        let big_summary_base = "s".repeat(2047);
        let mut nodes = Vec::new();
        let mut sets = Vec::new();
        let mut entries = Vec::new();
        for i in 0..9 {
            let id = format!("n{i}");
            let summary = format!("{big_summary_base}{i}");
            let summary_tokens = estimate_tokens(&summary);
            let summary_digest = content_digest_of(&summary);
            nodes.push(ContextNode {
                id: id.clone(),
                kind: ContextNodeKind::Source,
                content_digest: sha256_hex(format!("body-{id}").as_bytes()),
                summary: summary.clone(),
                source_bindings: vec![],
                token_estimate: summary_tokens,
            });
            sets.push(
                NodeRepresentationSet::build(
                    id.clone(),
                    vec![NodeRepresentation {
                        level: RepresentationLevel::Summary,
                        origin: RepresentationOrigin::HostExtracted,
                        content_digest: summary_digest.clone(),
                        derived_from: vec![],
                        content: summary.clone(),
                    }],
                )
                .expect("set"),
            );
            entries.push(SchedulerEntry {
                node_id: id.clone(),
                tier: WorkingSetTier::Hot,
                pinned: false,
                relevance: (10 + i as u8),
                last_access_tick: 0,
                token_estimate: 1000,
                content_digest: sha256_hex(format!("body-{id}").as_bytes()),
            });
        }
        let mut edges = Vec::new();
        for i in 0..8 {
            edges.push(ContextEdge {
                from: format!("n{i}"),
                to: format!("n{}", i + 1),
                kind: ContextEdgeKind::Contains,
            });
        }
        let graph = ContextGraph::build(nodes, edges).expect("graph");
        let store = ContextRepresentationStore::build(sets).expect("store");
        let mut state = WorkingSetState::build(entries).expect("state");
        let cfg = SchedulerConfig::new(4096).expect("cfg");
        let ctx = state.assemble(&graph, &store, &cfg);
        assert!(
            ctx.demoted.contains(&"n0".to_owned()) || ctx.entries.len() < 9
        );
        assert!(ctx.total_tokens_before > ctx.total_tokens_after);
        assert!(ctx.total_tokens_before >= 512);
    }

    #[test]
    fn assembly_determinism_byte_equal() {
        let (mut s1, graph, store) = assembly_fixture();
        let (mut s2, graph2, store2) = assembly_fixture();
        let cfg = SchedulerConfig::default();
        let c1 = s1.assemble(&graph, &store, &cfg);
        let c2 = s2.assemble(&graph2, &store2, &cfg);
        assert_eq!(c1, c2);
        assert_eq!(s1, s2);
    }

    #[test]
    fn benchmark_byte_identity_guard() {
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, false, 90, 0, 100),
            entry("b", WorkingSetTier::Hot, false, 90, 0, 100),
        ])
        .expect("build");
        let cfg = SchedulerConfig::default();
        let before = state.clone();
        let _ = state.process_tick(
            TickInput::new(1, vec![], "rev1", vec![], vec![]),
            &cfg,
        );
        assert_eq!(state.pipeline_runs(), before.pipeline_runs() + 1);
    }
}
