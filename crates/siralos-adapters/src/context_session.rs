//! Activation B3b (decision 99): host-owned session wiring of the context
//! subsystem behind the default-off `[profile.context_system]` opt-in.
//!
//! This module owns the host-side "session wiring" seam only — it never
//! renders, never persists, never spawns, and never lets the model inject
//! events or scores. The demand edge (events + search scores) is derived
//! exclusively from host-observed `ToolObservation`s through the existing
//! helpers in `tool::context_events` (`compose_tick_input`), and the
//! scheduler semantics (quota, stale-never-promote, ring cap, coalescing)
//! are inherited unchanged from `siralos_core::context_scheduler`
//! (decisions 89-93).
//!
//! The opt-in is narrowing-only: it authorizes read-only context machinery
//! (a bounds-bounded workspace scan, three read-only context tools over an
//! immutable snapshot, and in-memory scheduler ticks). A widening
//! interpretation is impossible by construction — the key adds no
//! capability beyond registering read-only tools and updating an
//! in-memory working set. Nothing is written to disk; no output format
//! changes (B4 owns the audit surface).

use std::path::Path;

use siralos_core::context_graph::ContextGraph;
use siralos_core::context_representation::{
    ContextRepresentationStore, RepresentationLevel,
};
use siralos_core::context_scheduler::{
    SchedulerConfig, SchedulerEntry, TickReport, WorkingSetState,
    WorkingSetTier,
};
use siralos_core::tool::Tool;

use siralos_core::context_metrics::ContextMetrics;

use crate::context_scan::{
    ScanError, WorkspaceContext, build_workspace_context,
};
use crate::tool::context::{
    ContextExpandTool, ContextInspectTool, ContextSearchTool, ContextToolState,
};
use crate::tool::context_events::{ToolObservation, compose_tick_input};

/// The deterministic clock port that supplies tick `now` values (decision
/// 89 A1). The session owns the clock; it is an input, never a model
/// surface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionClock {
    /// Monotonic logical tick counter. The session advances it per observed
    /// round so coalescing and recency are deterministic.
    pub tick: u64,
}

impl SessionClock {
    /// The current `now` value.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// Advance the logical clock by exactly one and return the new `now`.
    #[must_use]
    pub fn advance(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }
}

/// The yield of one session-build attempt: the host-held subsystem state
/// plus an optional fail-closed diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSystemBuild {
    /// The live, host-held context subsystem state. `None` when OFF
    /// (absent key / enabled=false) or when the build failed (fail-closed).
    pub session: Option<ContextSystemSession>,
    /// Truthful host-side diagnostic recorded when a requested build
    /// failed (`ScanError::Unavailable`); `None` otherwise. The session
    /// continues with the subsystem OFF — never fatal, never partial.
    pub diagnostic: Option<String>,
}

/// The live, read-only context subsystem held by one session when the
/// opt-in is on AND the workspace build succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSystemSession {
    /// The host-side WorkspaceContext (scan + classified graph + L0/L1 store).
    pub workspace: WorkspaceContext,
    /// The scheduler working set over the classified graph nodes (decision
    /// 79 slice 3 re-pinned by decisions 89-93).
    pub working_set: WorkingSetState,
    /// Scheduler configuration (deterministic budget).
    pub config: SchedulerConfig,
    /// Graph revision string for deterministic coalescing (static within a
    /// session — the graph is built once at startup).
    pub graph_revision: String,
    /// The logical clock supplying tick `now` values.
    pub clock: SessionClock,
    /// Pure in-memory metrics collector (decision 91) — saturating counters
    /// and the capped 64-record tick ring, derived deterministically from
    /// tick inputs/outputs.
    pub metrics: ContextMetrics,
}

/// Build the initial scheduler working set over the classified graph.
///
/// Each graph node becomes one `SchedulerEntry`, initially Cold with
/// relevance 0, last_access 0, and a deterministic token estimate from its
/// L1 summary (matching the decision 90 assembly accounting). The node
/// cap is enforced by `WorkingSetState::build` (512 nodes).
fn build_working_set(
    graph: &ContextGraph,
    store: &ContextRepresentationStore,
) -> WorkingSetState {
    let mut entries: Vec<SchedulerEntry> =
        Vec::with_capacity(graph.nodes().len());
    for node in graph.nodes() {
        // Resolve the L1 summary for a deterministic token estimate,
        // matching the decision 90 assembly accounting.
        let summary_text = store
            .set(&node.id)
            .and_then(|set| {
                siralos_core::context_representation::resolve_representation(
                    set,
                    RepresentationLevel::Summary,
                )
            })
            .map(|rep| rep.content.clone())
            .unwrap_or_else(|| node.summary.clone());
        let token_estimate =
            siralos_core::context_graph::estimate_tokens(&summary_text);
        entries.push(SchedulerEntry {
            node_id: node.id.clone(),
            tier: WorkingSetTier::Cold,
            pinned: false,
            relevance: 0,
            last_access_tick: 0,
            token_estimate,
            content_digest: node.content_digest.clone(),
        });
    }
    // The graph is built over the bounded scan (<=256 nodes in a session),
    // so 512 is never approached; a typed error still refuses any overflow.
    WorkingSetState::build(entries)
        .expect("session working set over the classified graph")
}

/// Attempt the decision 99 session build.
///
/// `enabled` is the applied profile's `[profile.context_system].enabled`
/// opt-in. When false, nothing is scanned, no working set is built, no
/// tools are registered, and no ticks run — the session behaves exactly
/// as before B3b (byte-transparent). When true, the workspace is scanned
/// with the pinned B1 defaults; any typed build failure records a
/// host-side diagnostic and leaves the subsystem OFF (never fatal, never
/// partial: no tools are registered if the build failed).
#[must_use]
pub fn build_context_system(root: &Path, enabled: bool) -> ContextSystemBuild {
    if !enabled {
        return ContextSystemBuild { session: None, diagnostic: None };
    }
    match build_workspace_context(
        root,
        crate::context_scan::DEFAULT_SCAN_BOUNDS,
    ) {
        Ok(workspace) => {
            let working_set =
                build_working_set(&workspace.graph, &workspace.store);
            let session = ContextSystemSession {
                workspace,
                working_set,
                config: SchedulerConfig::default(),
                graph_revision: "context-graph-v1".to_owned(),
                clock: SessionClock::default(),
                metrics: ContextMetrics::new(),
            };
            ContextSystemBuild { session: Some(session), diagnostic: None }
        }
        Err(ScanError::Unavailable { message }) => {
            // Fail-closed: the diagnostic is recorded, the session continues
            // with the subsystem fully off. No partial subsystem is ever
            // registered.
            ContextSystemBuild {
                session: None,
                diagnostic: Some(format!(
                    "context subsystem unavailable: {message}"
                )),
            }
        }
    }
}

impl ContextSystemSession {
    /// The derived `ContextToolState` snapshot over the current working
    /// set — the immutable source the three read-only context tools are
    /// registered over. `current_digests` binds each node id to its
    /// content digest so staleness is computable (within a static session
    /// content does not change, so no node is stale).
    #[must_use]
    pub fn tool_state(&self) -> ContextToolState {
        let current_digests: Vec<(String, String)> = self
            .workspace
            .graph
            .nodes()
            .iter()
            .map(|node| (node.id.clone(), node.content_digest.clone()))
            .collect();
        ContextToolState::new(
            self.workspace.graph.clone(),
            self.workspace.store.clone(),
            self.working_set.clone(),
            current_digests,
        )
    }

    /// Register the three read-only context tools over the derived
    /// snapshot. The tools are constructed over immutable snapshots, so a
    /// demand tick that raises a node's priority does not mutate a
    /// registered tool; the host refreshes the snapshot (`tool_state`)
    /// whenever it (re)builds the tool set.
    ///
    /// Decision 114 Q2 — accepted policy (documented): snapshots are
    /// build-time by design (immutable per decision 82/92), tiers may lag
    /// behind ticks until session restart, ranks and content are always
    /// fresh.
    #[must_use]
    pub fn register_tools(&self) -> Vec<Box<dyn Tool>> {
        let snapshot = self.tool_state();
        vec![
            Box::new(ContextSearchTool::new(snapshot.clone())),
            Box::new(ContextInspectTool::new(snapshot.clone())),
            Box::new(ContextExpandTool::new(snapshot.clone())),
        ]
    }

    /// Drive one demand-loop step after a completed tool round.
    ///
    /// Derives host-observed observations into a canonical bounded
    /// `TickInput` (events + search scores) via the existing helper, then
    /// processes one tick. Coalescing applies naturally: a no-op tick
    /// (no events, no graph delta) produces no working-set change without
    /// re-running the pipeline (the decision 89 coalescing guard). Each
    /// completed round advances the logical clock by exactly one, so the
    /// demand edge's `recency = now` stays deterministic.
    #[must_use]
    pub fn drive_tick(
        &mut self,
        observations: &[ToolObservation],
    ) -> TickReport {
        let _ = self.clock.advance();
        let now = self.clock.now();
        let tick_input = compose_tick_input(
            now,
            self.graph_revision.clone(),
            Vec::new(),
            Vec::new(),
            observations,
        );
        let before = self.working_set.clone();
        let report =
            self.working_set.process_tick(tick_input.clone(), &self.config);
        let assembled = self.working_set.assemble(
            &self.workspace.graph,
            &self.workspace.store,
            &self.config,
        );
        self.metrics.record_tick(
            &tick_input,
            &before,
            &self.working_set,
            &report,
            Some(&assembled),
        );
        report
    }

    /// The current scheduler working-set state for inspection and tests.
    #[must_use]
    pub fn working_set_entry(&self, node_id: &str) -> Option<&SchedulerEntry> {
        self.working_set.entry(node_id)
    }

    /// Whether the subsystem is ON (present) with the given working-set
    /// node present.
    #[must_use]
    pub fn node_priority(
        &self,
        node_id: &str,
    ) -> Option<(u8, WorkingSetTier)> {
        self.working_set_entry(node_id)
            .map(|entry| (entry.relevance, entry.tier))
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionClock, build_context_system};
    use siralos_core::context_scheduler::WorkingSetTier;
    use std::path::Path;

    fn tmp_root(label: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "siralos-context-session-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn write(root: &Path, rel: &str, content: &str) {
        let target = root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content.as_bytes()).unwrap();
    }

    #[test]
    fn off_path_is_byte_transparent() {
        let root = tmp_root("off");
        write(&root, "a.txt", "hello");
        let build = build_context_system(&root, false);
        // No scan, no graph, no working set, no tools, no diagnostic.
        assert!(build.session.is_none());
        assert!(build.diagnostic.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn on_builds_context_and_registers_exactly_three_tools() {
        let root = tmp_root("on");
        write(&root, "a.txt", "alpha content");
        write(&root, "b.txt", "beta content");
        let build = build_context_system(&root, true);
        let session = build.session.expect("session built");
        assert!(build.diagnostic.is_none());
        // Every scanned file is a graph node and a working-set entry.
        assert_eq!(session.workspace.graph.nodes().len(), 2);
        assert_eq!(session.working_set.entries().len(), 2);
        // Exactly the three read-only context tools are registered.
        let tools = session.register_tools();
        assert_eq!(tools.len(), 3);
        let mut names: Vec<String> =
            tools.iter().map(|t| t.definition().name.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "context.expand".to_owned(),
                "context.inspect".to_owned(),
                "context.search".to_owned()
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn demand_loop_raises_touched_node_priority() {
        let root = tmp_root("demand");
        write(&root, "a.txt", "alpha body");
        write(&root, "b.txt", "beta body");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session").clone();
        // Both nodes start Cold with relevance 0.
        assert_eq!(session.node_priority("a.txt").expect("a").0, 0);
        assert_eq!(
            session.node_priority("a.txt").expect("a").1,
            WorkingSetTier::Cold
        );
        // Host observes a `context.inspect` call for a.txt succeeding.
        let obs = crate::tool::context_events::ToolObservation::new(
            "context.inspect",
            serde_json::json!({ "node_id": "a.txt" }),
            siralos_core::provider::ToolExecutionResult::Success {
                output: serde_json::json!({ "id": "a.txt" }),
                summary: "inspected a.txt".to_owned(),
            },
        );
        let report = session.drive_tick(std::slice::from_ref(&obs));
        // The demand edge raises relevance by +32 (saturating 100) and
        // sets recency = now, so the node promotes out of Cold.
        assert!(report.promoted.iter().any(|id| id == "a.txt"));
        let (rel, tier) = session.node_priority("a.txt").expect("a");
        assert_eq!(rel, 32);
        assert_eq!(tier, WorkingSetTier::Warm);
        // The untouched node stays Cold.
        assert_eq!(
            session.node_priority("b.txt").expect("b").1,
            WorkingSetTier::Cold
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn coalescing_on_live_path_noop_round_no_state_change() {
        let root = tmp_root("coalesce");
        write(&root, "a.txt", "alpha body");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session").clone();
        // Establish the coalescing baseline with a first no-op round; the
        // working set is static (no graph delta, no events), so this sets
        // the graph revision baseline without moving any node.
        let _ = session.drive_tick(&[]);
        let after_warmup = session.working_set.entries().to_vec();
        let pipeline_after_warmup = session.working_set.pipeline_runs();
        // A subsequent identical no-op round must be coalesced: no state
        // change AND the pipeline does not re-run.
        let _ = session.drive_tick(&[]);
        assert_eq!(session.working_set.entries(), after_warmup.as_slice());
        assert_eq!(
            session.working_set.pipeline_runs(),
            pipeline_after_warmup,
            "a no-op tick must be coalesced (not pumped)",
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fail_closed_on_unavailable_workspace_records_diagnostic() {
        let missing = std::path::Path::new(
            "/tmp/siralos-context-session-missing-workspace-404-not-exist",
        );
        let _ = std::fs::remove_dir_all(missing);
        let build = build_context_system(missing, true);
        // The subsystem is OFF and a truthful diagnostic is recorded; the
        // host may continue the session (never fatal, never partial).
        assert!(build.session.is_none());
        let diagnostic = build.diagnostic.expect("diagnostic");
        assert!(diagnostic.contains("context subsystem unavailable"));
    }

    #[test]
    fn no_injection_snapshot_is_host_observed_only() {
        // The wiring exposes exactly one demand-drive entry point taking
        // host-observed `ToolObservation`s. There is no public path that
        // accepts a model-supplied `Vec<AccessEvent>` or `Vec<SearchScore>`
        // to inject into the working set; the canonical bounded TickInput is
        // derived only inside `drive_tick` from observations.
        let root = tmp_root("no-inject");
        write(&root, "a.txt", "alpha body");
        let build = build_context_system(&root, true);
        let mut session = build.session.expect("session").clone();
        // Injecting a foreign event by hand is impossible through the public
        // API; driving an empty set of observations cannot move any node.
        let _ = session.drive_tick(&[]);
        assert_eq!(
            session.node_priority("a.txt").expect("a").0,
            0,
            "no observation can move a node",
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn build_and_demand_determinism_byte_equal() {
        let root = tmp_root("determinism");
        write(&root, "a.txt", "alpha body");
        write(&root, "b.txt", "beta body");
        let first = build_context_system(&root, true);
        let second = build_context_system(&root, true);
        assert_eq!(first, second);
        // Driving the same observations twice from identical fresh sessions
        // yields byte-equal working sets.
        let mut s1 = first.session.expect("s1").clone();
        let mut s2 = second.session.expect("s2").clone();
        let obs = crate::tool::context_events::ToolObservation::new(
            "context.inspect",
            serde_json::json!({ "node_id": "a.txt" }),
            siralos_core::provider::ToolExecutionResult::Success {
                output: serde_json::json!({ "id": "a.txt" }),
                summary: "inspected a.txt".to_owned(),
            },
        );
        let _ = s1.drive_tick(std::slice::from_ref(&obs));
        let _ = s2.drive_tick(std::slice::from_ref(&obs));
        assert_eq!(s1.working_set, s2.working_set);
        assert_eq!(s1.clock, s2.clock);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clock_advances_one_per_round() {
        let mut clock = SessionClock::default();
        assert_eq!(clock.now(), 0);
        assert_eq!(clock.advance(), 1);
        assert_eq!(clock.advance(), 2);
        assert_eq!(clock.now(), 2);
    }
}
