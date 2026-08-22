//! Canonical subsystem ownership index (Stage 3 — Deterministic
//! Execution & Reproducibility, ADR 0029; R10a H2).
//!
//! Architecture/navigation metadata only — never a service registry,
//! never capability. Before an executor creates an overlapping
//! abstraction, context identifies the existing owner. Mirrors
//! `packages/core/src/determinism/discovery.ts` ownership section.

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
        path: "packages/core/src/projection/tool-projector.ts",
        overlaps_with: &["provider tool schema"],
    },
    OwnershipEntry {
        responsibility: "context projection",
        owner: "ContextProjector",
        path: "packages/core/src/projection/context-projector.ts",
        overlaps_with: &["provider context assembly"],
    },
    OwnershipEntry {
        responsibility: "planning depth",
        owner: "PlanningPolicy",
        path: "packages/core/src/planning/planning-policy.ts",
        overlaps_with: &["plan routing"],
    },
    OwnershipEntry {
        responsibility: "task state",
        owner: "TaskRuntime",
        path: "packages/core/src/tasks/task-runtime.ts",
        overlaps_with: &["workflow state"],
    },
    OwnershipEntry {
        responsibility: "acceptance evaluation",
        owner: "AcceptanceEvaluator",
        path: "packages/core/src/executor/",
        overlaps_with: &["completion gate"],
    },
    OwnershipEntry {
        responsibility: "evidence",
        owner: "EvidenceStore",
        path: "packages/core/src/tasks/task-runtime-evidence.ts",
        overlaps_with: &["evidence records"],
    },
    OwnershipEntry {
        responsibility: "approval",
        owner: "ApprovalSystem",
        path: "packages/core/src/security/approval.ts",
        overlaps_with: &["mutation authorization"],
    },
    OwnershipEntry {
        responsibility: "checkpoints",
        owner: "CheckpointStore",
        path: "packages/adapters/src/checkpoints/",
        overlaps_with: &["undo", "recovery"],
    },
    OwnershipEntry {
        responsibility: "workspace revisions",
        owner: "WorkspaceRevisionRegistry",
        path: "packages/core/src/workspace/workspace-revision.ts",
        overlaps_with: &["source identity"],
    },
    OwnershipEntry {
        responsibility: "canonical digests",
        owner: "ArtifactDigest",
        path: "packages/core/src/identity/artifact-digest.ts",
        overlaps_with: &["hashing"],
    },
    OwnershipEntry {
        responsibility: "documentation selection",
        owner: "DocumentationSelection",
        path: "packages/core/src/executor/documentation-context.ts",
        overlaps_with: &["guidance selection"],
    },
    OwnershipEntry {
        responsibility: "executor briefing",
        owner: "ExecutorBriefCompiler",
        path: "packages/core/src/executor/brief-compiler.ts",
        overlaps_with: &["task briefing"],
    },
    OwnershipEntry {
        responsibility: "validation plan",
        owner: "ValidationPlan",
        path: "packages/core/src/determinism/decisions.ts",
        overlaps_with: &["validation selection"],
    },
    OwnershipEntry {
        responsibility: "retry policy",
        owner: "RetryPolicy",
        path: "packages/core/src/determinism/decisions.ts",
        overlaps_with: &["repair loop"],
    },
    OwnershipEntry {
        responsibility: "impact analysis",
        owner: "ImpactAnalyzer",
        path: "packages/core/src/godot/impact/impact-analyzer.ts",
        overlaps_with: &["review context"],
    },
    OwnershipEntry {
        responsibility: "independent review",
        owner: "ChangeReviewer",
        path: "packages/core/src/godot/quality/quality-review.ts",
        overlaps_with: &["quality gate"],
    },
    OwnershipEntry {
        responsibility: "surface routing",
        owner: "DevelopmentSurfaceClassifier",
        path: "packages/core/src/godot/development/development-surface.ts",
        overlaps_with: &["workflow routing"],
    },
    OwnershipEntry {
        responsibility: "nondeterminism audit",
        owner: "NondeterminismAudit",
        path: "scripts/check-nondeterminism.mjs",
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
