//! Typed domain failure outcomes (Stage 3R R6).
//!
//! The Host observes failures as typed, stable-coded outcomes so future
//! bounded recovery (R11) can branch on semantics instead of string
//! matching. R6 implements no recovery machinery: a failure may stop or
//! reject an activation, and capability denial never triggers automatic
//! permission escalation.

use crate::domain::capability::CapabilityId;

/// The resource class that was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceExceededKind {
    /// Guest execution/work budget exhausted.
    Fuel,
    /// Guest memory budget exhausted.
    Memory,
    /// The call input exceeded its byte bound.
    InputBytes,
    /// The call output exceeded its byte bound.
    OutputBytes,
    /// The host-call budget was exhausted.
    HostCalls,
}

impl ResourceExceededKind {
    /// Stable machine-branchable code for this resource class.
    pub fn code(self) -> &'static str {
        match self {
            Self::Fuel => "FUEL",
            Self::Memory => "MEMORY",
            Self::InputBytes => "INPUT_BYTES",
            Self::OutputBytes => "OUTPUT_BYTES",
            Self::HostCalls => "HOST_CALLS",
        }
    }
}

/// Typed Host-observed domain failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainFailure {
    /// No package is installed.
    NotInstalled,
    /// The installed package is not enabled.
    Disabled,
    /// The exact same package is already installed.
    AlreadyInstalled,
    /// The package is already enabled.
    AlreadyEnabled,
    /// The package is already disabled.
    AlreadyDisabled,
    /// The requested operation is impossible while the domain is active.
    Active,
    /// The requested operation needs an active session, and none exists.
    NotActive,
    /// The package or request ABI is not the supported ABI.
    UnsupportedAbi {
        /// The ABI the host supports.
        expected: String,
        /// The ABI that was found or requested.
        found: String,
    },
    /// The package identity (id and/or exact digest) does not match.
    IdentityMismatch {
        /// Stable human-readable reason (never a recovery branch key).
        detail: String,
    },
    /// The Host policy denied the requested capabilities.
    CapabilityDenied {
        /// The requested capabilities outside Host authority, ordered.
        missing: Vec<CapabilityId>,
    },
    /// The activation request exceeds the installed package's declared
    /// capabilities. The package declaration is the authority ceiling
    /// for its own activation; a request may only narrow it.
    UndeclaredCapability {
        /// The requested capabilities absent from the package
        /// declaration, in canonical order.
        missing: Vec<CapabilityId>,
    },
    /// A resource or runtime bound was exceeded.
    ResourceExceeded {
        /// Which resource class was exceeded.
        kind: ResourceExceededKind,
    },
    /// The input was malformed or violated its bound.
    InvalidInput {
        /// Stable human-readable reason.
        reason: String,
    },
    /// The domain produced output that violated its bound.
    InvalidOutput {
        /// Stable human-readable reason.
        reason: String,
    },
    /// The guest faulted/trapped; Host state remains intact.
    GuestFault {
        /// Bounded diagnostic detail from the runtime.
        detail: String,
    },
    /// The operation was cancelled/interrupted.
    Cancelled,
    /// The operation cannot currently be performed.
    Unavailable {
        /// Stable human-readable reason.
        reason: String,
    },
}

impl DomainFailure {
    /// Stable machine-branchable failure code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotInstalled => "NOT_INSTALLED",
            Self::Disabled => "DISABLED",
            Self::AlreadyInstalled => "ALREADY_INSTALLED",
            Self::AlreadyEnabled => "ALREADY_ENABLED",
            Self::AlreadyDisabled => "ALREADY_DISABLED",
            Self::Active => "ACTIVE",
            Self::NotActive => "NOT_ACTIVE",
            Self::UnsupportedAbi { .. } => "UNSUPPORTED_ABI",
            Self::IdentityMismatch { .. } => "IDENTITY_MISMATCH",
            Self::CapabilityDenied { .. } => "CAPABILITY_DENIED",
            Self::UndeclaredCapability { .. } => "UNDECLARED_CAPABILITY",
            Self::ResourceExceeded { .. } => "RESOURCE_EXCEEDED",
            Self::InvalidInput { .. } => "INVALID_INPUT",
            Self::InvalidOutput { .. } => "INVALID_OUTPUT",
            Self::GuestFault { .. } => "GUEST_FAULT",
            Self::Cancelled => "CANCELLED",
            Self::Unavailable { .. } => "UNAVAILABLE",
        }
    }
}

/// Stable failure code of a typed failure (convenience accessor).
pub fn failure_code(failure: &DomainFailure) -> &'static str {
    failure.code()
}

#[cfg(test)]
mod tests {
    use super::{DomainFailure, ResourceExceededKind};
    use crate::domain::capability::CapabilityId;

    #[test]
    fn codes_are_stable_and_distinct() {
        let failures = [
            DomainFailure::NotInstalled,
            DomainFailure::Disabled,
            DomainFailure::AlreadyInstalled,
            DomainFailure::AlreadyEnabled,
            DomainFailure::AlreadyDisabled,
            DomainFailure::Active,
            DomainFailure::NotActive,
            DomainFailure::UnsupportedAbi {
                expected: "siralos:domain-abi@1.0.0".to_owned(),
                found: "siralos:domain-abi@1.1.0".to_owned(),
            },
            DomainFailure::IdentityMismatch {
                detail: "stale bytes".to_owned(),
            },
            DomainFailure::CapabilityDenied {
                missing: vec![CapabilityId::parse("process-exec").unwrap()],
            },
            DomainFailure::UndeclaredCapability {
                missing: vec![CapabilityId::parse("process-exec").unwrap()],
            },
            DomainFailure::ResourceExceeded {
                kind: ResourceExceededKind::Fuel,
            },
            DomainFailure::InvalidInput { reason: "bad".to_owned() },
            DomainFailure::InvalidOutput { reason: "bad".to_owned() },
            DomainFailure::GuestFault { detail: "trap".to_owned() },
            DomainFailure::Cancelled,
            DomainFailure::Unavailable { reason: "no runtime".to_owned() },
        ];
        let mut codes: Vec<&str> =
            failures.iter().map(DomainFailure::code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), failures.len());
    }
}
