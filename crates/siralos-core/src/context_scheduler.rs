//! Deterministic tiered context scheduler — HOT/WARM/COLD working set
//! (decision 79 slice 3, clauses a + f).
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
//! # Tick
//!
//! `tick()` increments the logical clock by one, then for each
//! non-`Archive` entry: `pinned → Hot`, otherwise tier by score.
//! `Archive` entries never re-tier automatically.
//!
//! # Budget (clause f)
//!
//! `enforce_budget()` sums `Hot` tokens from caller-supplied estimates
//! and while `hot_total > budget_tokens` demotes the lowest-scored `Hot`
//! non-pinned entry to `Warm` (tie-break: lower score first, then
//! `node_id` ascending). Pinned `Hot` entries are never budget-demoted.
//! Demotion never deletes underlying authoritative information.

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
}

/// Validated working-set state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingSetState {
    entries: Vec<SchedulerEntry>,
    tick: u64,
    tick_view: TickView,
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
        Ok(Self { entries: sorted, tick: 0, tick_view: TickView(0) })
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
        TickReport { tick: self.tick, promoted, demoted }
    }

    /// Enforce the deterministic budget by demoting lowest-scored Hot
    /// non-pinned entries to Warm. Demotion never deletes.
    pub fn enforce_budget(
        &mut self,
        config: &SchedulerConfig,
        token_estimates: &[(String, usize)],
    ) -> Result<BudgetReport, SchedulerError> {
        let map: BTreeMap<String, usize> =
            token_estimates.iter().map(|(k, v)| (k.clone(), *v)).collect();
        // Unknown node in caller map -> error.
        for key in map.keys() {
            if self.entry(key).is_none() {
                return Err(SchedulerError::UnknownNode {
                    node_id: key.clone(),
                });
            }
        }
        let hot_tokens_before = self.hot_total(&map)?;
        let mut hot_total = hot_tokens_before;
        let mut demoted: Vec<String> = Vec::new();
        while hot_total > config.budget_tokens {
            // Find lowest-scored Hot non-pinned entry.
            let mut candidate: Option<(u64, String)> = None;
            for e in &self.entries {
                if e.tier != WorkingSetTier::Hot || e.pinned {
                    continue;
                }
                if demoted.contains(&e.node_id) {
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
            let victim_token = *map.get(&victim_id).ok_or_else(|| {
                SchedulerError::UnknownNode { node_id: victim_id.clone() }
            })?;
            // Demote to Warm.
            if let Some(entry) = self.entry_mut(&victim_id) {
                entry.tier = WorkingSetTier::Warm;
            }
            demoted.push(victim_id);
            hot_total = hot_total.saturating_sub(victim_token);
        }
        let hot_tokens_after = self.hot_total(&map).unwrap_or(hot_total);
        demoted.sort();
        Ok(BudgetReport { hot_tokens_before, hot_tokens_after, demoted })
    }

    fn hot_total(
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

#[cfg(test)]
mod tests {
    use super::{
        SchedulerConfig, SchedulerEntry, SchedulerError, SchedulerEvent,
        WorkingSetState, WorkingSetTier, score,
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
            .expect("rel");
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Cold);
    }

    #[test]
    fn stale_demotes_even_when_pinned() {
        let mut state = WorkingSetState::build(vec![entry(
            "a",
            WorkingSetTier::Hot,
            true,
            90,
            0,
            0,
        )])
        .expect("build");
        // Stale should demote Hot->Warm even though pinned.
        let outcome = state
            .apply_event(SchedulerEvent::Stale { node_id: "a".to_owned() })
            .expect("stale");
        assert_eq!(outcome.tier_before, WorkingSetTier::Hot);
        assert_eq!(outcome.tier_after, WorkingSetTier::Warm);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Warm);
        // Pinned flag remains true.
        assert!(state.entry("a").unwrap().pinned);
        // Another stale Warm->Cold.
        let outcome2 = state
            .apply_event(SchedulerEvent::Stale { node_id: "a".to_owned() })
            .expect("stale2");
        assert_eq!(outcome2.tier_after, WorkingSetTier::Cold);
        // Cold stays Cold.
        let outcome3 = state
            .apply_event(SchedulerEvent::Stale { node_id: "a".to_owned() })
            .expect("stale3");
        assert_eq!(outcome3.tier_after, WorkingSetTier::Cold);
    }

    #[test]
    fn archive_only_via_explicit_op() {
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
        // Tick never archives.
        let report = state.advance_tick(&cfg);
        assert!(!report.promoted.contains(&"a".to_owned()));
        assert_ne!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
        // Explicit archive.
        let out = state
            .apply_event(SchedulerEvent::Archive { node_id: "a".to_owned() })
            .expect("archive");
        assert_eq!(out.tier_after, WorkingSetTier::Archive);
        // Further ticks keep Archive.
        let report2 = state.advance_tick(&cfg);
        assert_eq!(state.entry("a").unwrap().tier, WorkingSetTier::Archive);
        assert!(!report2.promoted.contains(&"a".to_owned()));
        assert!(!report2.demoted.contains(&"a".to_owned()));
    }

    #[test]
    fn budget_overflow_lowest_score_first_tie_break_node_id() {
        let mut state = WorkingSetState::build(vec![
            entry("a", WorkingSetTier::Hot, false, 20, 0, 100),
            entry("b", WorkingSetTier::Hot, false, 20, 0, 100),
            entry("c", WorkingSetTier::Hot, false, 90, 0, 100),
        ])
        .expect("build");
        // All at tick 0: a and b score 80+30=110 => but wait 20*4=80+30=110 <120 => Cold, yet they are Hot.
        // So their score is 110, c is 360+30=390.
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
}
