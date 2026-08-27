//! Bounded sanitized diagnostics (R8).
//!
//! Mirrors `packages/core/src/godot/diagnostics.ts`.

/// Severity of a bounded diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticSeverity {
    /// Info.
    Info,
    /// Warning.
    Warning,
    /// Error.
    Error,
}

impl DiagnosticSeverity {
    /// Canonical string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Parse canonical string.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// A bounded, sanitized diagnostic safe to surface to users and providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeDiagnostic {
    /// Severity.
    pub severity: DiagnosticSeverity,
    /// Bounded message.
    pub message: String,
}

impl SafeDiagnostic {
    /// Create a new diagnostic, bounding message length externally if needed.
    #[must_use]
    pub fn new(
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self { severity, message: message.into() }
    }
}
