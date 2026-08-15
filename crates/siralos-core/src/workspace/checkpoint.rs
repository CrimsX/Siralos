//! Checkpoint contracts, invariants, and pure classification logic
//! (R4, ADR 0006).
//!
//! A checkpoint is a Siralos-owned recovery record, never a Git commit,
//! a stash, conversation state, or model authority. The operation-state
//! relationship is the canonical invariant (create: absent->present;
//! update: present->present; delete: present->absent), validated from
//! typed core logic so prepared records and stored metadata can never
//! disagree. Undo planning and startup reconciliation classification
//! are pure functions over exact recorded state: they never guess and
//! never mutate the workspace.

/// Checkpoint lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointState {
    /// Recorded before application (recovery window).
    Prepared,
    /// Applied and verified.
    Applied,
    /// Restored (undo completed).
    Undone,
    /// Reconciliation: the before-state won.
    Abandoned,
    /// The checkpoint conflicted with newer work.
    Conflicted,
    /// Neither before nor after state matched (no guesses).
    Uncertain,
}

impl CheckpointState {
    /// The canonical protocol string for this state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Applied => "applied",
            Self::Undone => "undone",
            Self::Abandoned => "abandoned",
            Self::Conflicted => "conflicted",
            Self::Uncertain => "uncertain",
        }
    }

    /// Parse a protocol state string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "prepared" => Some(Self::Prepared),
            "applied" => Some(Self::Applied),
            "undone" => Some(Self::Undone),
            "abandoned" => Some(Self::Abandoned),
            "conflicted" => Some(Self::Conflicted),
            "uncertain" => Some(Self::Uncertain),
            _ => None,
        }
    }
}

/// Checkpoint operation (its meaning IS its existence transition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointOperation {
    /// absent -> present.
    Create,
    /// present -> present.
    Update,
    /// present -> absent.
    Delete,
}

impl CheckpointOperation {
    /// The canonical protocol string for this operation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    /// Parse a protocol operation string.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// The existence transitions required by each checkpoint operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationState {
    /// Whether the file exists before the operation.
    pub before_exists: bool,
    /// Whether the file exists after the operation.
    pub after_exists: bool,
}

impl OperationState {
    /// The canonical existence transition for an operation.
    pub fn for_operation(operation: CheckpointOperation) -> Self {
        match operation {
            CheckpointOperation::Create => {
                Self { before_exists: false, after_exists: true }
            }
            CheckpointOperation::Update => {
                Self { before_exists: true, after_exists: true }
            }
            CheckpointOperation::Delete => {
                Self { before_exists: true, after_exists: false }
            }
        }
    }
}

/// Exact file state recorded by a checkpoint (existence + identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointFileState {
    /// Whether the file existed at this point.
    pub exists: bool,
    /// Exact SHA-256 when the file existed, otherwise `None`.
    pub sha256: Option<String>,
    /// Exact byte length when the file existed, otherwise `None`.
    pub byte_length: Option<u64>,
}

/// Preview line counts carried by a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointPreview {
    /// Added lines in the mutation preview.
    pub added_lines: u64,
    /// Removed lines in the mutation preview.
    pub removed_lines: u64,
}

/// One stored file checkpoint (version 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCheckpoint {
    /// Stored record schema version (1).
    pub version: u64,
    /// Checkpoint id (`cp_<hex>`).
    pub id: String,
    /// Owning workspace fingerprint.
    pub workspace_fingerprint: String,
    /// Workspace-relative path of the affected file.
    pub relative_path: String,
    /// The mutation operation.
    pub operation: CheckpointOperation,
    /// The tool that recorded the checkpoint.
    pub tool_name: String,
    /// Record creation time (host-recorded; fixtures control it).
    pub created_at: String,
    /// Lifecycle state.
    pub state: CheckpointState,
    /// Exact before-state.
    pub before: CheckpointFileState,
    /// Exact after-state.
    pub after: CheckpointFileState,
    /// Mutation preview line counts.
    pub preview: CheckpointPreview,
}

/// Terminal states a prepared/applied checkpoint may transition to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointTerminalState {
    /// The before-state won (mutation never applied).
    Abandoned,
    /// The checkpoint conflicted with newer work.
    Conflicted,
    /// Neither state matched; no guess is made.
    Uncertain,
    /// The after-state won (mutation applied).
    Applied,
}

impl CheckpointTerminalState {
    /// The canonical protocol string for this terminal state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Abandoned => "abandoned",
            Self::Conflicted => "conflicted",
            Self::Uncertain => "uncertain",
            Self::Applied => "applied",
        }
    }
}
/// Why a checkpoint record failed the operation-state invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointInvariantError {
    /// The operation string is not a checkpoint operation.
    InvalidOperation,
    /// The declared existence transition disagrees with the invariant.
    StateMismatch {
        /// The operation as recorded.
        operation: CheckpointOperation,
        /// The required transition.
        expected: OperationState,
        /// The declared transition.
        declared: OperationState,
    },
}

/// Validate the operation/existence invariant for one record. The
/// operation-state relationship is the canonical invariant, never an
/// inference from tool names; invalid combinations fail validation.
pub fn validate_checkpoint_invariant(
    operation: CheckpointOperation,
    before_exists: bool,
    after_exists: bool,
) -> Result<(), CheckpointInvariantError> {
    let expected = OperationState::for_operation(operation);
    let declared = OperationState { before_exists, after_exists };
    if declared == expected {
        Ok(())
    } else {
        Err(CheckpointInvariantError::StateMismatch {
            operation,
            expected,
            declared,
        })
    }
}

/// Current workspace file state used by undo planning and
/// reconciliation (exists + exact SHA-256).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileState {
    /// Whether the file exists.
    pub exists: bool,
    /// Exact SHA-256 when the file exists and is readable, otherwise
    /// `None` (unreadable, linked, or oversized state).
    pub sha256: Option<String>,
}

/// Decision of undo planning for one checkpoint against current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoPlanDecision {
    /// Undo may proceed: create the deleted file.
    ReadyCreate,
    /// Undo may proceed: restore the previous content.
    ReadyRestore,
    /// Undo may proceed: delete the created file.
    ReadyDelete,
    /// Undo would overwrite newer work; conflict, never overwrite.
    Conflict,
}

/// Plan an undo from a checkpoint and the exact current file state,
/// mirroring the reference `planUndo`: the current state must equal
/// the recorded post-state exactly; anything else is a conflict.
pub fn plan_undo(
    checkpoint: &FileCheckpoint,
    current: &WorkspaceFileState,
) -> UndoPlanDecision {
    if checkpoint.after.exists {
        if current.exists
            && current.sha256.as_deref() == checkpoint.after.sha256.as_deref()
        {
            return if checkpoint.before.exists {
                UndoPlanDecision::ReadyRestore
            } else {
                UndoPlanDecision::ReadyDelete
            };
        }
        UndoPlanDecision::Conflict
    } else if !current.exists {
        UndoPlanDecision::ReadyCreate
    } else {
        UndoPlanDecision::Conflict
    }
}

/// Startup reconciliation classification for one pending checkpoint,
/// based only on exact recorded before/after state versus the current
/// file state (never guesses, never mutates the workspace).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationClass {
    /// A prepared checkpoint whose before-state still holds.
    Abandoned,
    /// A prepared checkpoint whose after-state now holds.
    Applied,
    /// A prepared checkpoint where neither state holds.
    Uncertain,
    /// An applied checkpoint whose file returned to the before-state
    /// (the recoverable undo-commit window).
    UndoneAfterRestore,
}

/// Classify one pending (prepared or applied) checkpoint against the
/// exact current file state. `None` means the checkpoint needs no
/// reconciliation (an applied checkpoint that still matches its
/// recorded after-state).
pub fn classify_reconciliation(
    checkpoint: &FileCheckpoint,
    current: &WorkspaceFileState,
) -> Option<ReconciliationClass> {
    let before = WorkspaceFileState {
        exists: checkpoint.before.exists,
        sha256: checkpoint.before.sha256.clone(),
    };
    let after = WorkspaceFileState {
        exists: checkpoint.after.exists,
        sha256: checkpoint.after.sha256.clone(),
    };
    match checkpoint.state {
        CheckpointState::Prepared => {
            if states_equal(current, &before) {
                Some(ReconciliationClass::Abandoned)
            } else if states_equal(current, &after) {
                Some(ReconciliationClass::Applied)
            } else {
                Some(ReconciliationClass::Uncertain)
            }
        }
        CheckpointState::Applied => {
            if states_equal(current, &before) && !states_equal(current, &after)
            {
                Some(ReconciliationClass::UndoneAfterRestore)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn states_equal(a: &WorkspaceFileState, b: &WorkspaceFileState) -> bool {
    if a.exists != b.exists {
        return false;
    }
    if !a.exists {
        return true;
    }
    a.sha256.is_some() && a.sha256 == b.sha256
}

/// Core-facing checkpoint store contract (inspection surface only at
/// R4: creation and retention capacity remain unavailable, exactly as
/// the reference reports them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointStoreModel {
    /// New checkpoint preparation is unavailable (no identity-bound
    /// commit primitive); historical data may still be inspected.
    CreationUnavailable,
}
#[cfg(test)]
mod tests {
    use super::{
        CheckpointFileState, CheckpointOperation, CheckpointPreview,
        CheckpointState, CheckpointTerminalState, FileCheckpoint,
        ReconciliationClass, UndoPlanDecision, WorkspaceFileState,
        classify_reconciliation, plan_undo, validate_checkpoint_invariant,
    };

    fn checkpoint(
        id: &str,
        operation: CheckpointOperation,
        state: CheckpointState,
        before: CheckpointFileState,
        after: CheckpointFileState,
    ) -> FileCheckpoint {
        FileCheckpoint {
            version: 1,
            id: id.to_owned(),
            workspace_fingerprint: "fingerprint".to_owned(),
            relative_path: "player.gd".to_owned(),
            operation,
            tool_name: "workspace.edit_file".to_owned(),
            created_at: "2024-01-01T00:00:00.000Z".to_owned(),
            state,
            before,
            after,
            preview: CheckpointPreview { added_lines: 1, removed_lines: 1 },
        }
    }

    fn present(sha256: &str) -> CheckpointFileState {
        CheckpointFileState {
            exists: true,
            sha256: Some(sha256.to_owned()),
            byte_length: Some(8),
        }
    }

    fn absent() -> CheckpointFileState {
        CheckpointFileState { exists: false, sha256: None, byte_length: None }
    }

    fn current(exists: bool, sha256: Option<&str>) -> WorkspaceFileState {
        WorkspaceFileState { exists, sha256: sha256.map(str::to_owned) }
    }

    #[test]
    fn operation_state_invariants_are_exact() {
        assert!(
            validate_checkpoint_invariant(
                CheckpointOperation::Create,
                false,
                true,
            )
            .is_ok()
        );
        assert!(
            validate_checkpoint_invariant(
                CheckpointOperation::Update,
                true,
                true,
            )
            .is_ok()
        );
        assert!(
            validate_checkpoint_invariant(
                CheckpointOperation::Delete,
                true,
                false,
            )
            .is_ok()
        );
        assert!(
            validate_checkpoint_invariant(
                CheckpointOperation::Create,
                true,
                true,
            )
            .is_err()
        );
        assert!(
            validate_checkpoint_invariant(
                CheckpointOperation::Delete,
                false,
                false,
            )
            .is_err()
        );
        assert!(
            validate_checkpoint_invariant(
                CheckpointOperation::Update,
                false,
                true,
            )
            .is_err()
        );
    }

    #[test]
    fn undo_planning_conflicts_never_overwrite() {
        let update = checkpoint(
            "cp-1",
            CheckpointOperation::Update,
            CheckpointState::Applied,
            present("before"),
            present("after"),
        );
        assert_eq!(
            plan_undo(&update, &current(true, Some("after"))),
            UndoPlanDecision::ReadyRestore,
        );
        assert_eq!(
            plan_undo(&update, &current(true, Some("newer"))),
            UndoPlanDecision::Conflict,
        );

        let created = checkpoint(
            "cp-2",
            CheckpointOperation::Create,
            CheckpointState::Applied,
            absent(),
            present("after"),
        );
        assert_eq!(
            plan_undo(&created, &current(true, Some("after"))),
            UndoPlanDecision::ReadyDelete,
        );
        assert_eq!(
            plan_undo(&created, &current(false, None)),
            UndoPlanDecision::Conflict,
        );

        let deleted = checkpoint(
            "cp-3",
            CheckpointOperation::Delete,
            CheckpointState::Applied,
            present("before"),
            absent(),
        );
        assert_eq!(
            plan_undo(&deleted, &current(false, None)),
            UndoPlanDecision::ReadyCreate,
        );
        assert_eq!(
            plan_undo(&deleted, &current(true, Some("newer"))),
            UndoPlanDecision::Conflict,
        );
    }

    #[test]
    fn reconciliation_classifies_from_exact_state_only() {
        let prepared = checkpoint(
            "cp-1",
            CheckpointOperation::Update,
            CheckpointState::Prepared,
            present("before"),
            present("after"),
        );
        assert_eq!(
            classify_reconciliation(&prepared, &current(true, Some("before"))),
            Some(ReconciliationClass::Abandoned),
        );
        assert_eq!(
            classify_reconciliation(&prepared, &current(true, Some("after"))),
            Some(ReconciliationClass::Applied),
        );
        assert_eq!(
            classify_reconciliation(&prepared, &current(true, Some("other"))),
            Some(ReconciliationClass::Uncertain),
        );

        let applied = checkpoint(
            "cp-2",
            CheckpointOperation::Update,
            CheckpointState::Applied,
            present("before"),
            present("after"),
        );
        assert_eq!(
            classify_reconciliation(&applied, &current(true, Some("before"))),
            Some(ReconciliationClass::UndoneAfterRestore),
        );
        assert_eq!(
            classify_reconciliation(&applied, &current(true, Some("after"))),
            None,
        );
    }

    #[test]
    fn terminal_state_strings_round_trip() {
        for state in [
            CheckpointTerminalState::Abandoned,
            CheckpointTerminalState::Conflicted,
            CheckpointTerminalState::Uncertain,
            CheckpointTerminalState::Applied,
        ] {
            assert_eq!(
                CheckpointState::parse(state.as_str()),
                Some(match state {
                    CheckpointTerminalState::Abandoned =>
                        CheckpointState::Abandoned,
                    CheckpointTerminalState::Conflicted =>
                        CheckpointState::Conflicted,
                    CheckpointTerminalState::Uncertain =>
                        CheckpointState::Uncertain,
                    CheckpointTerminalState::Applied =>
                        CheckpointState::Applied,
                }),
            );
        }
    }
}
