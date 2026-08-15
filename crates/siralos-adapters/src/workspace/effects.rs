//! Fail-closed mutation-preparation boundary (R4, ADR 0005).
//!
//! The reference reports every provider-accessible workspace mutation
//! effect as `unavailable` before any write, approval, or checkpoint,
//! because no identity-bound commit primitive exists. The Rust
//! candidate preserves that observable truth: preparation performs no
//! filesystem operation, validates no input beyond the cancellation
//! check, and returns the typed unavailable disposition; application
//! is equally unavailable. No new write capability is introduced
//! without a mechanically justified accepted decision.

/// The provider-accessible mutation tool surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationTool {
    /// `workspace.create_file`.
    CreateFile,
    /// `workspace.edit_file`.
    EditFile,
    /// `workspace.delete_file`.
    DeleteFile,
}

impl MutationTool {
    /// The canonical tool name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateFile => "workspace.create_file",
            Self::EditFile => "workspace.edit_file",
            Self::DeleteFile => "workspace.delete_file",
        }
    }
}
/// Outcome of one mutation preparation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparationOutcome {
    /// The operation is unavailable; nothing was written, approved,
    /// or checkpointed, and no path or input was inspected beyond the
    /// cancellation check (mirroring the reference fail-closed order).
    Unavailable {
        /// The tool that refused.
        tool: MutationTool,
        /// Truthful fail-closed reason.
        message: String,
    },
    /// The preparation was cancelled before any refusal.
    Cancelled {
        /// The tool that was cancelled.
        tool: MutationTool,
    },
}

/// Outcome of one mutation application attempt (unreachable while
/// preparation is unavailable; kept typed and fail-closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationOutcome {
    /// The operation is unavailable; no workspace bytes change.
    Unavailable {
        /// The tool that refused.
        tool: MutationTool,
        /// Truthful fail-closed reason.
        message: String,
    },
}

/// Prepare a provider-accessible mutation. The reference always
/// refuses before any write, approval, or checkpoint, and so does the
/// Rust candidate; the input payload is deliberately not inspected.
pub fn prepare_mutation(
    tool: MutationTool,
    cancelled: bool,
) -> PreparationOutcome {
    if cancelled {
        return PreparationOutcome::Cancelled { tool };
    }
    PreparationOutcome::Unavailable {
        tool,
        message: format!(
            "{} is unavailable: the Rust candidate has no identity-bound commit primitive, so the operation fails closed before any write, approval, or checkpoint.",
            tool.as_str(),
        ),
    }
}

/// Apply a prepared mutation. Always unavailable at R4; no workspace
/// bytes change and no checkpoint is created.
pub fn apply_mutation(tool: MutationTool) -> ApplicationOutcome {
    ApplicationOutcome::Unavailable {
        tool,
        message: format!(
            "{} is unavailable: application requires an identity-bound commit primitive that does not exist in the Rust candidate; no bytes were changed and no checkpoint was created.",
            tool.as_str(),
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::{
        ApplicationOutcome, MutationTool, PreparationOutcome, apply_mutation,
        prepare_mutation,
    };

    #[test]
    fn preparation_is_fail_closed_and_cancellation_is_typed() {
        for tool in [
            MutationTool::CreateFile,
            MutationTool::EditFile,
            MutationTool::DeleteFile,
        ] {
            assert!(matches!(
                prepare_mutation(tool, false),
                PreparationOutcome::Unavailable { .. },
            ));
            assert_eq!(
                prepare_mutation(tool, true),
                PreparationOutcome::Cancelled { tool },
            );
            assert!(matches!(
                apply_mutation(tool),
                ApplicationOutcome::Unavailable { .. },
            ));
        }
    }
}
