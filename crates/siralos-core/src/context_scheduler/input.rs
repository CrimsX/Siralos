//! Bounded canonical tick inputs: access events and search scores.

use std::collections::{BTreeMap, BTreeSet};

use super::{MAX_SEARCH_SCORES, MAX_TICK_EVENTS};

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
    let (c, _) = canonicalize_events_with_dropped(events);
    c
}

/// Canonicalize events and return the number dropped beyond the 64 cap.
/// Dropped counts only the truncation beyond 64 after dedupe, not duplicates.
#[must_use]
pub(crate) fn canonicalize_events_with_dropped(
    events: Vec<AccessEvent>,
) -> (Vec<AccessEvent>, usize) {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut uniq: Vec<AccessEvent> = Vec::new();
    for ev in events {
        if seen.insert(ev.node_id.clone()) {
            uniq.push(ev);
        }
    }
    uniq.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    let dropped = uniq.len().saturating_sub(MAX_TICK_EVENTS);
    if uniq.len() > MAX_TICK_EVENTS {
        uniq.truncate(MAX_TICK_EVENTS);
    }
    (uniq, dropped)
}

/// Host-observed search score for retention ordering (decision 92 score, 93 retention).
/// Score is the integer decision-92 search score per hit; absent nodes are treated as 0.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SearchScore {
    /// Target node id.
    pub node_id: String,
    /// Decision-92 integer search score.
    pub score: i32,
}

impl SearchScore {
    /// Create a host-observed search score.
    #[must_use]
    pub fn new(node_id: impl Into<String>, score: i32) -> Self {
        Self { node_id: node_id.into(), score }
    }
}

/// Canonicalize search scores: dedupe by node_id (one per node, keep highest score),
/// order by node_id ascending, truncate to first 64 (decision 93).
#[must_use]
pub fn canonicalize_search_scores(
    scores: Vec<SearchScore>,
) -> Vec<SearchScore> {
    let (c, _) = canonicalize_search_scores_with_dropped(scores);
    c
}

/// Canonicalize search scores and return the number dropped beyond the 64 cap.
/// Dedupe keeps the highest score per node.
#[must_use]
pub(crate) fn canonicalize_search_scores_with_dropped(
    scores: Vec<SearchScore>,
) -> (Vec<SearchScore>, usize) {
    let mut map: BTreeMap<String, i32> = BTreeMap::new();
    for s in scores {
        if s.node_id.is_empty() || s.node_id.len() > 256 {
            continue;
        }
        let entry = map.entry(s.node_id).or_insert(s.score);
        if s.score > *entry {
            *entry = s.score;
        }
    }
    let mut uniq: Vec<SearchScore> = map
        .into_iter()
        .map(|(node_id, score)| SearchScore { node_id, score })
        .collect();
    uniq.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    let dropped = uniq.len().saturating_sub(MAX_SEARCH_SCORES);
    if uniq.len() > MAX_SEARCH_SCORES {
        uniq.truncate(MAX_SEARCH_SCORES);
    }
    (uniq, dropped)
}

/// Host-owned TickInput for the deterministic pipeline (A1, A4, decision 93 search-scores).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickInput {
    /// Deterministic clock (now).
    pub now: u64,
    /// Canonical events (<=64, deduped, sorted).
    pub events: Vec<AccessEvent>,
    /// Number of canonical events dropped beyond the 64 cap (A1 overflow).
    pub events_dropped: usize,
    /// Graph revision digest (for coalescing).
    pub graph_revision: String,
    /// Canonical new-node set (sorted, deduped).
    pub new_node_ids: Vec<String>,
    /// Stale node ids for this tick (any tier).
    pub stale_node_ids: Vec<String>,
    /// Optional bounded latest-observed search scores (decision 93, at most 64,
    /// canonical node_id ascending, one per node, score 0 for absent).
    /// `None` means absent (demotion order unchanged); `Some(vec)` with empty vec is treated as absent as well.
    pub search_scores: Option<Vec<SearchScore>>,
    /// Number of canonical search scores dropped beyond the 64 cap.
    pub search_scores_dropped: usize,
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
        let (canonical_events, events_dropped) =
            canonicalize_events_with_dropped(events);
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
            events_dropped,
            graph_revision: graph_revision.into(),
            new_node_ids: new_nodes_sorted,
            stale_node_ids: stale_sorted,
            search_scores: None,
            search_scores_dropped: 0,
        }
    }

    /// Build with search scores (decision 93): canonicalize scores (dedupe highest, sorted, trunc 64).
    #[must_use]
    pub fn new_with_search_scores(
        now: u64,
        events: Vec<AccessEvent>,
        graph_revision: impl Into<String>,
        new_node_ids: Vec<String>,
        stale_node_ids: Vec<String>,
        search_scores: Vec<SearchScore>,
    ) -> Self {
        let mut base = Self::new(
            now,
            events,
            graph_revision,
            new_node_ids,
            stale_node_ids,
        );
        let (canonical_scores, dropped) =
            canonicalize_search_scores_with_dropped(search_scores);
        if canonical_scores.is_empty() {
            base.search_scores = None;
            base.search_scores_dropped = 0;
        } else {
            base.search_scores = Some(canonical_scores);
            base.search_scores_dropped = dropped;
        }
        base
    }

    /// Attach search scores after construction (canonicalized, bounded). Empty => None.
    #[must_use]
    pub fn with_search_scores(
        mut self,
        search_scores: Vec<SearchScore>,
    ) -> Self {
        let (canonical_scores, dropped) =
            canonicalize_search_scores_with_dropped(search_scores);
        if canonical_scores.is_empty() {
            self.search_scores = None;
            self.search_scores_dropped = 0;
        } else {
            self.search_scores = Some(canonical_scores);
            self.search_scores_dropped = dropped;
        }
        self
    }

    /// Empty input helper (no events, no graph delta).
    #[must_use]
    pub fn empty(now: u64, graph_revision: impl Into<String>) -> Self {
        Self::new(now, vec![], graph_revision, vec![], vec![])
    }
}
