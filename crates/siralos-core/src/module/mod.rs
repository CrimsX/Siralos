//! Minimal module capability architecture (Stage 3R R6).
//!
//! Host-owned, module-neutral lifecycle and capability semantics for
//! optional specialized development intelligence. R6 owns the minimum
//! generic contracts required to prove the Host/Module boundary:
//!
//! - package identity (stable id + exact digest + versioned ABI);
//! - installation, enablement, and run-scoped activation as
//!   mechanically distinct states;
//! - declared capability requests and the Host authoritative grant
//!   decision (enablement never implies authority);
//! - exact activation binding (wrong, stale, or incompatible identity
//!   fails before any semantic work);
//! - typed, recovery-ready failure outcomes with stable codes;
//! - the explicit absence of implicit acquisition: workspace contents
//!   never install, enable, or activate a module.
//!
//! This module is purely semantic: it contains no component runtime,
//! filesystem, or process machinery. The production Component Model /
//! WIT boundary (ADR 0034) is implemented by `siralos-adapters` on top
//! of these contracts; the synthetic conformance module proves the
//! boundary without adding any product module semantics.
//!
//! No future Module ecosystem beyond this foundation is introduced
//! here: R6 is the internal lifecycle/capability foundation that later
//! packaging may consume (ADR 0036).

pub mod capability;
pub mod failure;
pub mod lifecycle;
pub mod package;

pub use capability::{
    CapabilityGrant, CapabilityId, CapabilityRequest, GrantDecision,
    HostAuthority, decide_grant,
};
pub use failure::{ModuleFailure, ResourceExceededKind, failure_code};
pub use lifecycle::{
    ActivationBinding, ActivationRequest, ActiveModule, Eligibility,
    EligibilityReason, LifecycleState, ModuleLifecycle, RuntimeCheckResult,
    WORKSPACE_FILE_OPAQUE, WorkspaceModuleScan, classify_workspace_file,
    workspace_module_scan,
};
pub use package::{
    ModuleAbi, ModulePackage, ModulePackageId, PackageDigest,
    verify_package_digest,
};
