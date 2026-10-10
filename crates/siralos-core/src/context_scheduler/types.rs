//! Scheduler vocabulary: tiers, entries, events, reports, and errors.

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

    pub(super) fn order(self) -> u8 {
        match self {
            Self::Hot => 0,
            Self::Warm => 1,
            Self::Cold => 2,
            Self::Archive => 3,
        }
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
    pub(super) fn node_id(&self) -> &str {
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
