//! Generic validation result model (Stage 3R R5).
//!
//! The status vocabulary is extracted from the reference check-result
//! semantics (ADR 0010). The critical distinction is preserved: SOURCE
//! INVALID is a valid language-intelligence outcome carrying bounded
//! diagnostics, while infrastructure failures (unavailable, unsupported,
//! denied, conflict, cancelled, timed out, failed) are typed statuses
//! with a bounded message. Validation never fabricates diagnostics and
//! never promotes a stale revision: results bind to the exact source
//! revision they validated.

use crate::language::diagnostic::DiagnosticSet;

/// Typed validation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationStatus {
    /// The source is valid (no diagnostics).
    Valid,
    /// The source is invalid; bounded diagnostics describe the findings.
    Invalid,
    /// Validation is not supported for this input.
    Unsupported,
    /// Validation is unavailable (e.g. no execution surface).
    Unavailable,
    /// Infrastructure failure.
    Failed,
    /// The operation was cancelled.
    Cancelled,
    /// The operation timed out.
    TimedOut,
    /// The operation was denied by policy.
    Denied,
    /// The operation conflicted with current state.
    Conflict,
}

impl ValidationStatus {
    /// The canonical protocol string for this status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Unsupported => "unsupported",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Denied => "denied",
            Self::Conflict => "conflict",
        }
    }
}

/// One generic validation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    /// Typed status.
    pub status: ValidationStatus,
    /// Bounded, revision-bound diagnostics for invalid sources.
    pub diagnostics: Option<DiagnosticSet>,
    /// Bounded infrastructure message for non-valid/invalid statuses.
    pub message: Option<String>,
}

impl ValidationResult {
    /// A valid-source result.
    pub fn valid() -> Self {
        Self {
            status: ValidationStatus::Valid,
            diagnostics: None,
            message: None,
        }
    }

    /// An invalid-source result with bounded diagnostics.
    pub fn invalid(diagnostics: DiagnosticSet) -> Self {
        Self {
            status: ValidationStatus::Invalid,
            diagnostics: Some(diagnostics),
            message: None,
        }
    }

    /// An infrastructure status with a bounded message.
    pub fn infrastructure(
        status: ValidationStatus,
        message: impl Into<String>,
    ) -> Self {
        Self { status, diagnostics: None, message: Some(message.into()) }
    }
}
