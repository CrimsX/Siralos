//! Generic workspace and project foundation (Stage 3R R4, ADR 0016).
//!
//! Domain-neutral host semantics for workspace identity, safe
//! workspace-relative paths, bounded read/list/search contracts,
//! revision handles and the session-scoped revision registry, prepared
//! (never applied) mutation models, checkpoint contracts and
//! invariants, and the read-only Git result model.
//!
//! `siralos-core` owns no filesystem, process, or domain behavior:
//! actual workspace reads, bounded traversal, and concrete checkpoint
//! inspection live in `siralos-adapters`. This module deliberately
//! contains no language-specific parsing -- that belongs to later
//! milestones (R5/R8). Domain project files (for example a game-engine
//! project descriptor) remain ordinary opaque workspace data here.
//!
//! Authority model: a revision handle is ergonomic identity, never
//! authority; possession grants no read, write, approval, capability,
//! or path access. Prepared effects are proposals, not approval and
//! not application. Effectful workspace mutation remains fail-closed
//! (unavailable) exactly as the TypeScript reference reports it, until
//! a mechanically identity-bound commit primitive exists.

pub mod bounds;
pub mod checkpoint;
pub mod effect;
pub mod git;
pub mod path;
pub mod revision;

pub use bounds::{WORKSPACE_LIMITS, WorkspaceLimits};
pub use checkpoint::{
    CheckpointFileState, CheckpointOperation, CheckpointState,
    CheckpointStoreModel, CheckpointTerminalState, FileCheckpoint,
    OperationState, UndoPlanDecision, classify_reconciliation, plan_undo,
    validate_checkpoint_invariant,
};
pub use effect::{
    MutationOperation, PreparedCreateFile, PreparedDeleteFile,
    PreparedEditFile, PreparedMutation, PreparedMutationKind,
    PreparedMutationState,
};
pub use git::{GitErrorCode, GitInspectionDisposition};
pub use path::{
    PathValidationError, WorkspaceRelativePath,
    is_protected_behavioral_config_path, is_protected_write_target,
};
pub use revision::{
    ObservedReadMode, ObservedWorkspaceRead, WorkspaceRevisionIdentity,
    WorkspaceRevisionRegistry, WorkspaceRevisionRegistryOptions,
    compute_workspace_revision_handle,
};
