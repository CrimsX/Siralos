//! Structured blocked disposition (Stage 3 milestone 11, ADR 0027).
//!
//! Mirrors `packages/core/src/godot/development/blocked-disposition.ts`.
//! When a unified development task cannot complete, the host produces a
//! typed blocker with a concrete explanation and the list of successful
//! prior changes that are preserved. Completion is never fabricated: an
//! unsupported requirement ends the task honestly as blocked.

use super::DevelopmentError;

/// Typed reason a development task ended blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedReasonKind {
    /// The document cannot be serialized deterministically.
    UnsupportedSerialization,
    /// Runtime verification would be required to proceed.
    RuntimeVerificationRequired,
    /// The requested change is not representable as structured operations.
    MutationNotRepresentable,
    /// The sandbox is unavailable.
    SandboxUnavailable,
    /// The source revision kept going stale.
    RepeatedStaleRevision,
    /// One-time approval was denied.
    ApprovalDenied,
    /// A validation gate could not run.
    ValidationGateUnavailable,
    /// The bounded repair budget was exhausted.
    RepairBudgetExhausted,
    /// Infrastructure failed.
    InfrastructureUnavailable,
}

impl BlockedReasonKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedSerialization => "unsupported_serialization",
            Self::RuntimeVerificationRequired => {
                "runtime_verification_required"
            }
            Self::MutationNotRepresentable => "mutation_not_representable",
            Self::SandboxUnavailable => "sandbox_unavailable",
            Self::RepeatedStaleRevision => "repeated_stale_revision",
            Self::ApprovalDenied => "approval_denied",
            Self::ValidationGateUnavailable => "validation_gate_unavailable",
            Self::RepairBudgetExhausted => "repair_budget_exhausted",
            Self::InfrastructureUnavailable => "infrastructure_unavailable",
        }
    }

    /// Parse a protocol string; unknown values are rejected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unsupported_serialization" => {
                Some(Self::UnsupportedSerialization)
            }
            "runtime_verification_required" => {
                Some(Self::RuntimeVerificationRequired)
            }
            "mutation_not_representable" => {
                Some(Self::MutationNotRepresentable)
            }
            "sandbox_unavailable" => Some(Self::SandboxUnavailable),
            "repeated_stale_revision" => Some(Self::RepeatedStaleRevision),
            "approval_denied" => Some(Self::ApprovalDenied),
            "validation_gate_unavailable" => {
                Some(Self::ValidationGateUnavailable)
            }
            "repair_budget_exhausted" => Some(Self::RepairBudgetExhausted),
            "infrastructure_unavailable" => {
                Some(Self::InfrastructureUnavailable)
            }
            _ => None,
        }
    }
}

/// One typed blocker with concrete explanation and preserved changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedDisposition {
    /// Reason kind.
    pub kind: BlockedReasonKind,
    /// Concrete, bounded explanation of the blocker.
    pub detail: String,
    /// Workspace-relative paths of successful prior changes (preserved).
    pub preserved_changes: Vec<String>,
}

/// Create one blocked disposition; the explanation must be concrete.
pub fn create_blocked_disposition(
    kind: BlockedReasonKind,
    detail: &str,
    preserved_changes: &[String],
) -> Result<BlockedDisposition, DevelopmentError> {
    let detail = detail.trim();
    if detail.is_empty() {
        return Err(DevelopmentError {
            message: "A blocked disposition requires a concrete explanation."
                .to_owned(),
        });
    }
    Ok(BlockedDisposition {
        kind,
        detail: detail.to_owned(),
        preserved_changes: preserved_changes.to_vec(),
    })
}

/// Deterministic single-line reason text for task-state blocking.
#[must_use]
pub fn blocked_reason_text(disposition: &BlockedDisposition) -> String {
    let preserved = if disposition.preserved_changes.is_empty() {
        String::new()
    } else {
        format!(
            " (preserved changes: {})",
            disposition.preserved_changes.join(", ")
        )
    };
    format!(
        "blocked[{}]: {}{}",
        disposition.kind.as_str(),
        disposition.detail,
        preserved
    )
}

#[cfg(test)]
mod tests {
    use super::{
        BlockedReasonKind, blocked_reason_text, create_blocked_disposition,
    };
    use crate::godot::development::DevelopmentError;

    #[test]
    fn dispositions_require_concrete_explanations() {
        assert_eq!(
            create_blocked_disposition(
                BlockedReasonKind::ApprovalDenied,
                "   ",
                &[],
            ),
            Err(DevelopmentError {
                message:
                    "A blocked disposition requires a concrete explanation."
                        .to_owned()
            })
        );
    }

    #[test]
    fn reason_text_lists_preserved_changes_deterministically() {
        let bare = create_blocked_disposition(
            BlockedReasonKind::RuntimeVerificationRequired,
            "runtime evidence is unavailable",
            &[],
        )
        .expect("disposition");
        assert_eq!(
            blocked_reason_text(&bare),
            "blocked[runtime_verification_required]: runtime evidence is unavailable"
        );
        let preserved = create_blocked_disposition(
            BlockedReasonKind::RepeatedStaleRevision,
            "the source revision kept changing",
            &["res://kept-a.tscn".to_owned(), "res://kept-b.gd".to_owned()],
        )
        .expect("disposition");
        assert_eq!(
            blocked_reason_text(&preserved),
            "blocked[repeated_stale_revision]: the source revision kept changing \
             (preserved changes: res://kept-a.tscn, res://kept-b.gd)"
        );
    }
}
