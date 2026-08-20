//! UI-neutral Godot application events (R8).
//!
//! Mirrors `packages/core/src/godot/events.ts`.

/// UI-neutral Godot application event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotApplicationEvent {
    /// Discovery started.
    DiscoveryStarted,
    /// One candidate found.
    CandidateFound {
        /// Installation id.
        installation_id: String,
        /// Source label.
        source: String,
    },
    /// Probe started.
    ProbeStarted {
        /// Installation id.
        installation_id: String,
        /// Probe kind.
        probe: String,
    },
    /// Probe completed.
    ProbeCompleted {
        /// Installation id.
        installation_id: String,
        /// Probe kind.
        probe: String,
        /// Status `success` | `degraded` | `failed`.
        status: String,
    },
    /// Project inspected.
    ProjectInspected {
        /// Whether a project was detected.
        detected: bool,
        /// Warning count.
        warnings: usize,
    },
}
