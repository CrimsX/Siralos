//! Canonical subsystem ownership index (Stage 3 — Deterministic
//! Execution & Reproducibility, ADR 0029; R10a H2).
//!
//! Architecture/navigation metadata only — never a service registry,
//! never capability. Before an executor creates an overlapping
//! abstraction, context identifies the existing owner. `path` names the
//! CURRENT Rust home of each responsibility; Godot-domain
//! responsibilities live in the external `siralos-godot` repository.
//! The TypeScript-era homes this index was first ported from are
//! archived at `5da5cde` (decision 40) and are deliberately not cited
//! here: Rust is the sole source of truth.

use super::ports::stable_sort_by_key;

/// One canonical ownership entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipEntry {
    /// Canonical responsibility identifier.
    pub responsibility: &'static str,
    /// Canonical owner module.
    pub owner: &'static str,
    /// Source path of the owner.
    pub path: &'static str,
    /// Related responsibilities that overlap and must reuse the owner.
    pub overlaps_with: &'static [&'static str],
}

/// The canonical ownership index (declaration order).
pub const OWNERSHIP_INDEX: &[OwnershipEntry] = &[
    OwnershipEntry {
        responsibility: "tool projection",
        owner: "ToolProjector",
        path: "crates/siralos-core/src/tool/",
        overlaps_with: &["provider tool schema"],
    },
    OwnershipEntry {
        responsibility: "context projection",
        owner: "ContextProjector",
        path: "crates/siralos-core/src/projection/",
        overlaps_with: &["provider context assembly"],
    },
    OwnershipEntry {
        responsibility: "planning depth",
        owner: "PlanningPolicy",
        path: "crates/siralos-core/src/planning/policy.rs",
        overlaps_with: &["plan routing"],
    },
    OwnershipEntry {
        responsibility: "task state",
        owner: "TaskRuntime",
        path: "crates/siralos-core/src/task/runtime.rs",
        overlaps_with: &["workflow state"],
    },
    OwnershipEntry {
        responsibility: "acceptance evaluation",
        owner: "AcceptanceEvaluator",
        path: "crates/siralos-core/src/executor/acceptance.rs",
        overlaps_with: &["completion gate"],
    },
    OwnershipEntry {
        responsibility: "evidence",
        owner: "EvidenceStore",
        path: "crates/siralos-core/src/task/evidence.rs",
        overlaps_with: &["evidence records"],
    },
    OwnershipEntry {
        responsibility: "approval",
        owner: "ApprovalSystem",
        path: "crates/siralos-core/src/security.rs",
        overlaps_with: &["mutation authorization"],
    },
    OwnershipEntry {
        responsibility: "checkpoints",
        owner: "CheckpointStore",
        path: "crates/siralos-adapters/src/workspace/checkpoint.rs",
        overlaps_with: &["undo", "recovery"],
    },
    OwnershipEntry {
        responsibility: "workspace revisions",
        owner: "WorkspaceRevisionRegistry",
        path: "crates/siralos-core/src/workspace/revision.rs",
        overlaps_with: &["source identity"],
    },
    OwnershipEntry {
        responsibility: "canonical digests",
        owner: "ArtifactDigest",
        path: "crates/siralos-core/src/identity/artifact.rs",
        overlaps_with: &["hashing"],
    },
    OwnershipEntry {
        responsibility: "documentation selection",
        owner: "DocumentationSelection",
        path: "crates/siralos-core/src/executor/context.rs",
        overlaps_with: &["guidance selection"],
    },
    OwnershipEntry {
        responsibility: "executor briefing",
        owner: "ExecutorBriefCompiler",
        path: "crates/siralos-core/src/executor/brief.rs",
        overlaps_with: &["task briefing"],
    },
    OwnershipEntry {
        responsibility: "validation plan",
        owner: "ValidationPlan",
        path: "crates/siralos-core/src/determinism/decisions.rs",
        overlaps_with: &["validation selection"],
    },
    OwnershipEntry {
        responsibility: "retry policy",
        owner: "RetryPolicy",
        path: "crates/siralos-core/src/determinism/decisions.rs",
        overlaps_with: &["repair loop"],
    },
    OwnershipEntry {
        responsibility: "impact analysis",
        owner: "ImpactAnalyzer",
        path: "siralos-godot/src/godot/impact/analyzer.rs",
        overlaps_with: &["review context"],
    },
    OwnershipEntry {
        responsibility: "independent review",
        owner: "ChangeReviewer",
        path: "siralos-godot/src/godot/impact/",
        overlaps_with: &["quality gate"],
    },
    OwnershipEntry {
        responsibility: "surface routing",
        owner: "DevelopmentSurfaceClassifier",
        path: "siralos-godot/src/godot/development/surface.rs",
        overlaps_with: &["workflow routing"],
    },
    OwnershipEntry {
        responsibility: "nondeterminism audit",
        owner: "NondeterminismAudit",
        path: "tests/differential/run-differential.mjs",
        overlaps_with: &["architecture checks"],
    },
];

/// Deterministic ownership resolution: same responsibility → same
/// owner. Exact case-insensitive match first, then overlap aliases.
#[must_use]
pub fn resolve_owner(responsibility: &str) -> Option<&'static OwnershipEntry> {
    let normalized = responsibility.trim().to_lowercase();
    OWNERSHIP_INDEX
        .iter()
        .find(|entry| entry.responsibility.to_lowercase() == normalized)
        .or_else(|| {
            OWNERSHIP_INDEX.iter().find(|entry| {
                entry
                    .overlaps_with
                    .iter()
                    .any(|overlap| overlap.to_lowercase() == normalized)
            })
        })
}

/// Bounded deterministic listing of all canonical owners sorted by
/// responsibility.
#[must_use]
pub fn list_ownership() -> Vec<OwnershipEntry> {
    let entries: &[OwnershipEntry] = OWNERSHIP_INDEX;
    stable_sort_by_key(entries, |entry: &OwnershipEntry| {
        entry.responsibility.to_owned()
    })
}
