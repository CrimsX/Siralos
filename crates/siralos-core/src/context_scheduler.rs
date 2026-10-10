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

mod assembly;
mod input;
mod policy;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub use assembly::{AssembledContext, AssembledEntry, NeighborStub};
pub use input::{
    AccessEvent, SearchScore, TickInput, canonicalize_events,
    canonicalize_search_scores,
};
pub use policy::{SchedulerConfig, score};
pub use state::{SchedulerTick, TickView, WorkingSetState};
pub use types::{
    BudgetReport, SchedulerEntry, SchedulerError, SchedulerEvent,
    SchedulerOutcome, TickReport, WorkingSetTier,
};

pub(crate) use assembly::stub_token_estimate;

/// Maximum number of scheduler nodes.
pub(crate) const MAX_SCHEDULER_NODES: usize = 512;
/// Default hot threshold in relevance units (human-readable).
pub const DEFAULT_HOT_THRESHOLD: u8 = 70;
/// Default warm threshold in relevance units (human-readable).
pub const DEFAULT_WARM_THRESHOLD: u8 = 30;
/// Default working-set budget in tokens.
pub(crate) const DEFAULT_BUDGET_TOKENS: usize = 4096;
/// Pinned HOT quota (decision 89 A3).
pub(crate) const PINNED_HOT_BUDGET_TOKENS: usize = 1024;
/// Maximum canonical events per TickInput (A1).
pub(crate) const MAX_TICK_EVENTS: usize = 64;
/// Maximum canonical search-score entries per TickInput (decision 93).
pub(crate) const MAX_SEARCH_SCORES: usize = 64;
