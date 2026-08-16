//! Domain lifecycle state machine (Stage 3R R6).
//!
//! Availability, installation, enablement, and activation are
//! mechanically distinct and never conflated:
//!
//! ```text
//! Absent --install--> Installed --enable--> Enabled --activate--> Active
//!   ^       |            ^                                      |
//!   |       +--uninstall-+                 +------deactivate-----+
//!   +---------------uninstall (from Enabled)---------------------+
//! ```
//!
//! Invalid transitions are typed failures; no implicit transition
//! exists. Activation is run/session scoped and still requires the
//! exact package identity, a compatible protocol, the declared
//! capability requests, the Host policy decision, and the required
//! resource/runtime checks. Workspace contents never install, enable,
//! or activate a domain: R6 has no file heuristic (workspace contents
//! are opaque to the lifecycle).

use crate::domain::capability::{
    CapabilityGrant, CapabilityRequest, HostAuthority, decide_grant,
};
use crate::domain::failure::{DomainFailure, ResourceExceededKind};
use crate::domain::package::{
    DomainAbi, DomainPackage, DomainPackageId, PackageDigest,
};

/// The lifecycle state of one domain slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// No package is installed.
    Absent,
    /// A package is installed but disabled.
    Installed,
    /// A package is installed and enabled (may request activation).
    Enabled,
    /// A package is installed, enabled, and active in a session.
    Active,
}

impl LifecycleState {
    /// The canonical protocol string for this state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Installed => "installed",
            Self::Enabled => "enabled",
            Self::Active => "active",
        }
    }
}

/// The result of the Host resource/runtime check at activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCheckResult {
    /// The runtime can host the domain within its bounds.
    Ready,
    /// A resource bound would be exceeded.
    ResourceExceeded(ResourceExceededKind),
    /// The runtime is currently unavailable.
    Unavailable,
}

impl RuntimeCheckResult {
    /// Parse a canonical protocol value.
    pub fn parse(value: &str) -> Result<Self, DomainFailure> {
        match value {
            "ready" => Ok(Self::Ready),
            "resource-exceeded" => {
                Ok(Self::ResourceExceeded(ResourceExceededKind::Fuel))
            }
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(DomainFailure::InvalidInput {
                reason: "invalid runtime check result".to_owned(),
            }),
        }
    }
}

/// The exact identity a caller requests to activate: the package id,
/// the exact digest, the ABI, and the requested capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRequest {
    package_id: DomainPackageId,
    digest: PackageDigest,
    abi: DomainAbi,
    capabilities: CapabilityRequest,
}

impl ActivationRequest {
    /// Parse a request from untrusted strings.
    pub fn parse(
        package_id: &str,
        digest: &str,
        abi: &str,
        capabilities: &[String],
    ) -> Result<Self, DomainFailure> {
        Ok(Self {
            package_id: DomainPackageId::parse(package_id)?,
            digest: PackageDigest::parse(digest)?,
            abi: DomainAbi::parse(abi)?,
            capabilities: CapabilityRequest::parse(capabilities)?,
        })
    }

    /// The requested package id.
    pub fn package_id(&self) -> &DomainPackageId {
        &self.package_id
    }

    /// The requested exact digest.
    pub fn digest(&self) -> &PackageDigest {
        &self.digest
    }

    /// The requested ABI.
    pub fn abi(&self) -> &DomainAbi {
        &self.abi
    }

    /// The capabilities requested for this activation.
    pub fn capabilities(&self) -> &CapabilityRequest {
        &self.capabilities
    }
}

/// The exact package identity bound by one successful activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationBinding {
    package_id: DomainPackageId,
    digest: PackageDigest,
    abi: DomainAbi,
}

impl ActivationBinding {
    /// Build the binding for the exact request identity.
    pub fn from_request(request: &ActivationRequest) -> Self {
        Self {
            package_id: request.package_id.clone(),
            digest: request.digest.clone(),
            abi: request.abi.clone(),
        }
    }

    /// The bound package id.
    pub fn package_id(&self) -> &DomainPackageId {
        &self.package_id
    }

    /// The bound exact digest.
    pub fn digest(&self) -> &PackageDigest {
        &self.digest
    }

    /// The bound ABI.
    pub fn abi(&self) -> &DomainAbi {
        &self.abi
    }

    /// Whether this binding matches the installed package exactly.
    pub fn matches(&self, package: &DomainPackage) -> bool {
        self.package_id == *package.id()
            && self.digest == *package.digest()
            && self.abi == *package.abi()
    }
}

/// One run/session-scoped active domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveDomain {
    session_id: u64,
    binding: ActivationBinding,
    grant: CapabilityGrant,
}

impl ActiveDomain {
    /// The monotonic session id of this activation.
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    /// The exact identity bound by this activation.
    pub fn binding(&self) -> &ActivationBinding {
        &self.binding
    }

    /// The effective capability grant (never wider than Host
    /// authority).
    pub fn grant(&self) -> &CapabilityGrant {
        &self.grant
    }
}

/// The validated, not-yet-committed outcome of activation preparation:
/// the exact binding, the validated request capabilities, a
/// NON-AUTHORITATIVE provisional grant, and the lifecycle generation
/// that was validated. The authoritative transition happens only when
/// the Host commits the prepared activation, so no fallible runtime
/// step can ever leave the lifecycle Active without a session; the
/// generation fence binds the preparation to the exact lifecycle
/// episode, and the final capability grant is recomputed at commit
/// from the commit-time Host authority, so a prepared activation is
/// never a bearer token for authority from its preparation context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedActivation {
    generation: u64,
    binding: ActivationBinding,
    requested: CapabilityRequest,
    provisional_grant: CapabilityGrant,
}

impl PreparedActivation {
    /// The exact identity that will be bound on commit.
    pub fn binding(&self) -> &ActivationBinding {
        &self.binding
    }

    /// The validated activation request capabilities (bounded by the
    /// package declaration at preparation). The final grant is
    /// recomputed from this request and the commit-time Host
    /// authority; this field is never itself authority.
    pub fn requested_capabilities(&self) -> &CapabilityRequest {
        &self.requested
    }

    /// The provisional grant computed from the PREPARATION-TIME Host
    /// authority. Non-authoritative: it is offered only for
    /// provisional runtime setup and is never used for the
    /// authoritative ActiveDomain grant, which commit recomputes from
    /// the commit-time Host authority.
    pub fn provisional_grant(&self) -> &CapabilityGrant {
        &self.provisional_grant
    }
}

/// A typed reason why an activation is not eligible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EligibilityReason {
    /// No package is installed.
    NotInstalled,
    /// The installed package is disabled.
    Disabled,
    /// The request identity does not match the installed package.
    IdentityMismatch,
    /// The request ABI is not the supported ABI.
    UnsupportedAbi,
    /// The domain is already active in a session.
    Active,
    /// The requested capabilities are outside Host authority.
    CapabilityDenied,
    /// The request exceeds the installed package's declared capabilities.
    UndeclaredCapability,
    /// The resource/runtime check failed.
    ResourceExceeded,
    /// The runtime is unavailable.
    Unavailable,
}

impl EligibilityReason {
    /// Stable machine-branchable code for this reason.
    pub fn code(self) -> &'static str {
        match self {
            Self::NotInstalled => "NOT_INSTALLED",
            Self::Disabled => "DISABLED",
            Self::IdentityMismatch => "IDENTITY_MISMATCH",
            Self::UnsupportedAbi => "UNSUPPORTED_ABI",
            Self::Active => "ACTIVE",
            Self::CapabilityDenied => "CAPABILITY_DENIED",
            Self::UndeclaredCapability => "UNDECLARED_CAPABILITY",
            Self::ResourceExceeded => "RESOURCE_EXCEEDED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

/// The deterministic activation eligibility report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eligibility {
    ready: bool,
    reasons: Vec<EligibilityReason>,
}

impl Eligibility {
    /// Whether the request is eligible (no reasons).
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// The ordered reasons (fixed check order).
    pub fn reasons(&self) -> &[EligibilityReason] {
        &self.reasons
    }
}

/// The requested capabilities absent from the installed package's
/// declaration, in canonical (ordered, deduplicated) request order. The
/// package declaration is the authority ceiling for its own activation
/// requests: a request may only narrow it, never broaden it.
fn undeclared_capabilities(
    request: &CapabilityRequest,
    package: &DomainPackage,
) -> Vec<crate::domain::capability::CapabilityId> {
    request
        .iter()
        .filter(|capability| {
            !package.requested_capabilities().contains(capability)
        })
        .cloned()
        .collect()
}

/// The internal authoritative state: one explicit enum, so impossible
/// boolean combinations cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DomainState {
    Absent,
    Installed(DomainPackage),
    Enabled(DomainPackage),
    Active { package: DomainPackage, active: ActiveDomain },
}

/// Host-owned domain lifecycle state (installation/enablement records
/// plus the current run/session-scoped activation).
///
/// `generation` is the lifecycle identity of the current episode: it
/// advances exactly once on every successful material transition
/// (install, uninstall, enable, disable, activation commit,
/// deactivate), so a prepared activation is bound to the episode it
/// was validated against and any intervening transition makes it
/// stale. Failed operations never advance it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainLifecycle {
    state: DomainState,
    next_session: u64,
    generation: u64,
}

impl Default for DomainLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainLifecycle {
    /// A fresh lifecycle with no package installed.
    pub fn new() -> Self {
        Self { state: DomainState::Absent, next_session: 1, generation: 0 }
    }

    /// The current lifecycle state.
    pub fn state(&self) -> LifecycleState {
        match &self.state {
            DomainState::Absent => LifecycleState::Absent,
            DomainState::Installed(_) => LifecycleState::Installed,
            DomainState::Enabled(_) => LifecycleState::Enabled,
            DomainState::Active { .. } => LifecycleState::Active,
        }
    }

    /// Whether a package is installed (regardless of enablement).
    pub fn available(&self) -> bool {
        !matches!(self.state, DomainState::Absent)
    }

    /// Whether the installed package is enabled.
    pub fn enabled(&self) -> bool {
        matches!(
            self.state,
            DomainState::Enabled(_) | DomainState::Active { .. }
        )
    }

    /// The active domain session, if any.
    pub fn active(&self) -> Option<&ActiveDomain> {
        match &self.state {
            DomainState::Active { active, .. } => Some(active),
            _ => None,
        }
    }

    /// The installed package, if any.
    pub fn installed_package(&self) -> Option<&DomainPackage> {
        match &self.state {
            DomainState::Absent => None,
            DomainState::Installed(package) => Some(package),
            DomainState::Enabled(package) => Some(package),
            DomainState::Active { package, .. } => Some(package),
        }
    }

    /// Explicitly install a locally supplied, Host-verified package.
    /// Installing a different package under an occupied slot fails; a
    /// package update requires an explicit uninstall/install
    /// transition.
    pub fn install(
        &mut self,
        package: DomainPackage,
    ) -> Result<(), DomainFailure> {
        match &self.state {
            DomainState::Absent => {
                self.state = DomainState::Installed(package);
                self.generation += 1;
                Ok(())
            }
            DomainState::Installed(existing)
            | DomainState::Enabled(existing)
            | DomainState::Active { package: existing, .. } => {
                if existing == &package {
                    Err(DomainFailure::AlreadyInstalled)
                } else {
                    Err(DomainFailure::IdentityMismatch {
                        detail:
                            "a different package is installed; uninstall first"
                                .to_owned(),
                    })
                }
            }
        }
    }

    /// Explicitly remove the installed package. Refused while active.
    pub fn uninstall(&mut self) -> Result<(), DomainFailure> {
        match std::mem::replace(&mut self.state, DomainState::Absent) {
            DomainState::Absent => Err(DomainFailure::NotInstalled),
            DomainState::Installed(_) | DomainState::Enabled(_) => {
                self.generation += 1;
                Ok(())
            }
            other => {
                self.state = other;
                Err(DomainFailure::Active)
            }
        }
    }

    /// Explicitly enable the installed package. Enablement grants no
    /// capability: authority remains separate Host state.
    pub fn enable(&mut self) -> Result<(), DomainFailure> {
        match std::mem::replace(&mut self.state, DomainState::Absent) {
            DomainState::Absent => Err(DomainFailure::NotInstalled),
            DomainState::Installed(package) => {
                self.state = DomainState::Enabled(package);
                self.generation += 1;
                Ok(())
            }
            other => {
                self.state = other;
                Err(DomainFailure::AlreadyEnabled)
            }
        }
    }

    /// Explicitly disable the installed package. Refused while active.
    pub fn disable(&mut self) -> Result<(), DomainFailure> {
        match std::mem::replace(&mut self.state, DomainState::Absent) {
            DomainState::Absent => Err(DomainFailure::NotInstalled),
            DomainState::Enabled(package) => {
                self.state = DomainState::Installed(package);
                self.generation += 1;
                Ok(())
            }
            other => {
                let failure = match &other {
                    DomainState::Installed(_) => {
                        DomainFailure::AlreadyDisabled
                    }
                    _ => DomainFailure::Active,
                };
                self.state = other;
                Err(failure)
            }
        }
    }

    fn deep_reasons(
        &self,
        package: &DomainPackage,
        request: &ActivationRequest,
        supported_abi: &DomainAbi,
        authority: &HostAuthority,
        runtime: &RuntimeCheckResult,
    ) -> Vec<EligibilityReason> {
        let mut reasons = Vec::new();
        if request.package_id != *package.id()
            || request.digest != *package.digest()
        {
            reasons.push(EligibilityReason::IdentityMismatch);
        }
        if !request.abi.is_compatible_with(package.abi()) {
            reasons.push(EligibilityReason::IdentityMismatch);
        }
        if !request.abi.is_compatible_with(supported_abi) {
            reasons.push(EligibilityReason::UnsupportedAbi);
        }
        if !undeclared_capabilities(request.capabilities(), package).is_empty()
        {
            reasons.push(EligibilityReason::UndeclaredCapability);
        }
        if matches!(
            decide_grant(request.capabilities(), authority),
            crate::domain::capability::GrantDecision::Denied { .. }
        ) {
            reasons.push(EligibilityReason::CapabilityDenied);
        }
        match runtime {
            RuntimeCheckResult::Ready => {}
            RuntimeCheckResult::ResourceExceeded(_) => {
                reasons.push(EligibilityReason::ResourceExceeded);
            }
            RuntimeCheckResult::Unavailable => {
                reasons.push(EligibilityReason::Unavailable);
            }
        }
        reasons
    }

    /// Report activation eligibility: ready when no typed reason
    /// blocks the request, else the ordered reasons. The earliest
    /// gates (installed, enabled) are reported alone; deeper checks
    /// accumulate in fixed order.
    pub fn eligibility(
        &self,
        request: &ActivationRequest,
        supported_abi: &DomainAbi,
        authority: &HostAuthority,
        runtime: &RuntimeCheckResult,
    ) -> Eligibility {
        let reasons = match &self.state {
            DomainState::Absent => vec![EligibilityReason::NotInstalled],
            DomainState::Installed(_) => vec![EligibilityReason::Disabled],
            DomainState::Active { .. } => vec![EligibilityReason::Active],
            DomainState::Enabled(package) => self.deep_reasons(
                package,
                request,
                supported_abi,
                authority,
                runtime,
            ),
        };
        Eligibility { ready: reasons.is_empty(), reasons }
    }

    /// Prepare an activation without committing any authoritative
    /// state. Pure validation: the installed/enabled gates, the exact
    /// identity (id, digest, and ABI against the installed package),
    /// the Host-supported ABI, the package-declaration capability
    /// ceiling, the Host policy decision, and the resource/runtime
    /// check all run here, so every fallible preparation happens
    /// before the
    /// authoritative Active transition. The result must be committed
    /// with [`DomainLifecycle::commit_activation`].
    pub fn prepare_activation(
        &self,
        request: &ActivationRequest,
        supported_abi: &DomainAbi,
        authority: &HostAuthority,
        runtime: &RuntimeCheckResult,
    ) -> Result<PreparedActivation, DomainFailure> {
        let package = match &self.state {
            DomainState::Absent => return Err(DomainFailure::NotInstalled),
            DomainState::Installed(_) => return Err(DomainFailure::Disabled),
            DomainState::Enabled(package) => package,
            DomainState::Active { .. } => return Err(DomainFailure::Active),
        };
        if request.package_id != *package.id() {
            return Err(DomainFailure::IdentityMismatch {
                detail:
                    "requested package id does not match the installed package"
                        .to_owned(),
            });
        }
        if request.digest != *package.digest() {
            return Err(DomainFailure::IdentityMismatch {
                detail:
                    "requested digest does not match the installed package"
                        .to_owned(),
            });
        }
        if !request.abi.is_compatible_with(package.abi()) {
            return Err(DomainFailure::IdentityMismatch {
                detail:
                    "requested ABI does not match the installed package ABI"
                        .to_owned(),
            });
        }
        if !request.abi.is_compatible_with(supported_abi) {
            return Err(DomainFailure::UnsupportedAbi {
                expected: supported_abi.as_str().to_owned(),
                found: request.abi.as_str().to_owned(),
            });
        }
        // The package declaration is the authority ceiling for its own
        // activation: a request may narrow it, never broaden it.
        let undeclared =
            undeclared_capabilities(request.capabilities(), package);
        if !undeclared.is_empty() {
            return Err(DomainFailure::UndeclaredCapability {
                missing: undeclared,
            });
        }
        let grant = match decide_grant(request.capabilities(), authority) {
            crate::domain::capability::GrantDecision::Granted(grant) => grant,
            crate::domain::capability::GrantDecision::Denied { missing } => {
                return Err(DomainFailure::CapabilityDenied { missing });
            }
        };
        match runtime {
            RuntimeCheckResult::Ready => {}
            RuntimeCheckResult::ResourceExceeded(kind) => {
                return Err(DomainFailure::ResourceExceeded { kind: *kind });
            }
            RuntimeCheckResult::Unavailable => {
                return Err(DomainFailure::Unavailable {
                    reason: "domain runtime is unavailable".to_owned(),
                });
            }
        }
        Ok(PreparedActivation {
            generation: self.generation,
            binding: ActivationBinding::from_request(request),
            requested: request.capabilities().clone(),
            provisional_grant: grant,
        })
    }

    /// Commit a prepared activation: the single authoritative
    /// Enabled -> Active transition. Before any mutation the commit
    /// revalidates the prepared activation against the current
    /// lifecycle AND the authorizing Host context: the generation
    /// captured at preparation must still equal the current
    /// generation, the state must still be Enabled, the prepared
    /// binding must still exactly match the currently enabled package
    /// (stable id, exact digest, and ABI), the bound ABI must still be
    /// supported by this Host, the runtime/resource policy must still
    /// be ready, and the FINAL capability grant is recomputed from the
    /// commit-time Host authority over the validated request
    /// capabilities. Any mismatch is a typed failure without changing
    /// state, allocating a session id, or advancing the generation:
    /// [`DomainFailure::StaleActivation`] for lifecycle staleness,
    /// [`DomainFailure::UnsupportedAbi`] for a Host-incompatible
    /// bound ABI, [`DomainFailure::CapabilityDenied`] for a narrower
    /// commit-time authority, and the typed resource failures for a
    /// runtime that is no longer ready.
    ///
    /// Preparation never creates durable authority: the provisional
    /// grant carried by the prepared activation is never used here,
    /// so a prepared activation cannot import authority from its
    /// preparation context into this commit.
    ///
    /// On success the session id is allocated exactly once, the
    /// lifecycle advances to Active, and the generation advances, so
    /// the published [`ActiveDomain`] always carries an exact package
    /// binding for the enabled package, a grant authorized by the
    /// commit-time Host authority, and a session id from a committed
    /// activation.
    pub fn commit_activation(
        &mut self,
        prepared: PreparedActivation,
        supported_abi: &DomainAbi,
        authority: &HostAuthority,
        runtime: RuntimeCheckResult,
    ) -> Result<ActiveDomain, DomainFailure> {
        // 1. The prepared activation must still belong to this
        //    lifecycle episode. Every successful material transition
        //    advanced the generation, so any such transition since
        //    preparation is detected here.
        if prepared.generation != self.generation {
            return Err(DomainFailure::StaleActivation);
        }
        // 2. The current state must still be Enabled.
        let DomainState::Enabled(package) = &self.state else {
            return Err(DomainFailure::StaleActivation);
        };
        // 3. The prepared binding must still exactly match the
        //    currently enabled package (stable id, exact digest, and
        //    ABI): the complete `ActivationBinding::matches`
        //    invariant. Generation fencing never replaces this
        //    package identity check.
        if !prepared.binding.matches(package) {
            return Err(DomainFailure::StaleActivation);
        }
        // 4. The bound ABI must still be supported by the Host that
        //    authorizes this commit.
        if !prepared.binding.abi().is_compatible_with(supported_abi) {
            return Err(DomainFailure::UnsupportedAbi {
                expected: supported_abi.as_str().to_owned(),
                found: prepared.binding.abi().as_str().to_owned(),
            });
        }
        // 5. The final capability grant is computed NOW from the
        //    commit-time Host authority over the validated request
        //    capabilities: preparation never creates durable
        //    authority, and a PreparedActivation is never a bearer
        //    token for the authority of its preparation context. A
        //    narrower commit authority fails closed with the typed
        //    denial; a wider one can never widen the request.
        let grant = match decide_grant(&prepared.requested, authority) {
            crate::domain::capability::GrantDecision::Granted(grant) => grant,
            crate::domain::capability::GrantDecision::Denied { missing } => {
                return Err(DomainFailure::CapabilityDenied { missing });
            }
        };
        // 6. The runtime/resource policy of the authorizing Host must
        //    still be ready.
        match runtime {
            RuntimeCheckResult::Ready => {}
            RuntimeCheckResult::ResourceExceeded(kind) => {
                return Err(DomainFailure::ResourceExceeded { kind });
            }
            RuntimeCheckResult::Unavailable => {
                return Err(DomainFailure::Unavailable {
                    reason: "domain runtime is unavailable".to_owned(),
                });
            }
        }
        let package =
            match std::mem::replace(&mut self.state, DomainState::Absent) {
                DomainState::Enabled(package) => package,
                other => {
                    self.state = other;
                    return Err(DomainFailure::StaleActivation);
                }
            };
        let active = ActiveDomain {
            session_id: self.next_session,
            binding: prepared.binding,
            grant,
        };
        self.next_session += 1;
        self.generation += 1;
        self.state = DomainState::Active { package, active: active.clone() };
        Ok(active)
    }

    /// Activate the installed, enabled package for this session:
    /// preparation plus commit. Fails closed (typed) on any of: wrong
    /// identity, stale digest, incompatible ABI, undeclared
    /// capabilities, Host policy denial, or a failed resource/runtime
    /// check — before any authoritative state changes. While already
    /// active, activation is rejected with the typed active failure.
    pub fn activate(
        &mut self,
        request: ActivationRequest,
        supported_abi: &DomainAbi,
        authority: &HostAuthority,
        runtime: RuntimeCheckResult,
    ) -> Result<ActiveDomain, DomainFailure> {
        let prepared = self.prepare_activation(
            &request,
            supported_abi,
            authority,
            &runtime,
        )?;
        self.commit_activation(prepared, supported_abi, authority, runtime)
    }
    /// End the current run/session-scoped activation. The package
    /// stays installed and enabled.
    pub fn deactivate(&mut self) -> Result<(), DomainFailure> {
        match std::mem::replace(&mut self.state, DomainState::Absent) {
            DomainState::Active { package, .. } => {
                self.state = DomainState::Enabled(package);
                self.generation += 1;
                Ok(())
            }
            other => {
                self.state = other;
                Err(DomainFailure::NotActive)
            }
        }
    }
}

/// The classification kind of a workspace file with respect to domain
/// acquisition. R6 has exactly one kind: workspace files are opaque.
pub const WORKSPACE_FILE_OPAQUE: &str = "opaque";

/// Classify one workspace file name. R6 deliberately defines no
/// domain heuristic: every workspace file is opaque, so workspace
/// contents can never install, enable, activate, download, or
/// recommend a domain. This is the explicit absence of a heuristic,
/// not a magic detection rule.
pub fn classify_workspace_file(_name: &str) -> &'static str {
    WORKSPACE_FILE_OPAQUE
}

/// The deterministic workspace domain scan report. All side-effect
/// counters are zero by construction: the scan is a pure function over
/// file names and has no access to any installation or activation
/// machinery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDomainScan {
    /// The number of classified file names.
    pub classified: u64,
    /// Domain candidates found (always zero).
    pub candidates: u64,
    /// Implicit installations (always zero).
    pub installs: u64,
    /// Implicit enablements (always zero).
    pub enables: u64,
    /// Implicit activations (always zero).
    pub activations: u64,
    /// Downloads (always zero).
    pub downloads: u64,
    /// Recommendations (always zero).
    pub recommendations: u64,
}

/// Scan workspace file names for implicit domain acquisition. The
/// report proves that no filesystem heuristic grants Domain
/// authority.
pub fn workspace_domain_scan(files: &[String]) -> WorkspaceDomainScan {
    WorkspaceDomainScan {
        classified: files.len() as u64,
        candidates: 0,
        installs: 0,
        enables: 0,
        activations: 0,
        downloads: 0,
        recommendations: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActivationRequest, DomainLifecycle, EligibilityReason, LifecycleState,
        RuntimeCheckResult, classify_workspace_file, workspace_domain_scan,
    };
    use crate::domain::capability::HostAuthority;
    use crate::domain::failure::{DomainFailure, ResourceExceededKind};
    use crate::domain::package::{DomainAbi, DomainPackage};

    const ABI: &str = "siralos:domain-abi@1.0.0";

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn package(id: &str, digest: &str) -> DomainPackage {
        DomainPackage::parse(id, digest, ABI, &ids(&["workspace-read"]))
            .unwrap()
    }

    fn digest(byte: u8) -> String {
        format!("{:02x}", byte).repeat(32)
    }

    fn request(id: &str, digest: &str) -> ActivationRequest {
        ActivationRequest::parse(id, digest, ABI, &ids(&["workspace-read"]))
            .unwrap()
    }

    fn authority() -> HostAuthority {
        HostAuthority::parse(&ids(&["workspace-read"])).unwrap()
    }

    #[test]
    fn absent_cannot_enable_or_activate() {
        let mut lifecycle = DomainLifecycle::new();
        assert_eq!(lifecycle.state(), LifecycleState::Absent);
        assert!(!lifecycle.available());
        assert!(matches!(
            lifecycle.enable(),
            Err(DomainFailure::NotInstalled)
        ));
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest(1)),
                &DomainAbi::parse(ABI).unwrap(),
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::NotInstalled)
        ));
        assert!(matches!(
            lifecycle.uninstall(),
            Err(DomainFailure::NotInstalled)
        ));
        assert!(matches!(
            lifecycle.deactivate(),
            Err(DomainFailure::NotActive)
        ));
        let eligibility = lifecycle.eligibility(
            &request("conformance-domain", &digest(1)),
            &DomainAbi::parse(ABI).unwrap(),
            &authority(),
            &RuntimeCheckResult::Ready,
        );
        assert!(!eligibility.ready());
        assert_eq!(eligibility.reasons(), &[EligibilityReason::NotInstalled]);
    }

    #[test]
    fn install_is_explicit_and_distinct_from_enable() {
        let mut lifecycle = DomainLifecycle::new();
        let first = package("conformance-domain", &digest(1));
        lifecycle.install(first).unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Installed);
        assert!(lifecycle.available());
        assert!(!lifecycle.enabled());
        assert!(lifecycle.active().is_none());
        // The identical package cannot be installed twice.
        assert!(matches!(
            lifecycle.install(package("conformance-domain", &digest(1))),
            Err(DomainFailure::AlreadyInstalled)
        ));
        // A different package under the same slot is stale.
        assert!(matches!(
            lifecycle.install(package("conformance-domain", &digest(2))),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
        // Disabled domains cannot activate.
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest(1)),
                &DomainAbi::parse(ABI).unwrap(),
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::Disabled)
        ));
    }

    #[test]
    fn enable_does_not_grant_authority() {
        let mut lifecycle = DomainLifecycle::new();
        lifecycle.install(package("conformance-domain", &digest(1))).unwrap();
        lifecycle.enable().unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(matches!(
            lifecycle.enable(),
            Err(DomainFailure::AlreadyEnabled)
        ));
        assert!(matches!(lifecycle.disable(), Ok(())));
        assert_eq!(lifecycle.state(), LifecycleState::Installed);
        assert!(matches!(
            lifecycle.disable(),
            Err(DomainFailure::AlreadyDisabled)
        ));
    }

    #[test]
    fn activation_binds_exact_identity_and_grants_within_authority() {
        let mut lifecycle = DomainLifecycle::new();
        let digest1 = digest(1);
        lifecycle.install(package("conformance-domain", &digest1)).unwrap();
        lifecycle.enable().unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        let active = lifecycle
            .activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_eq!(active.session_id(), 1);
        assert_eq!(
            active.binding().package_id().as_str(),
            "conformance-domain",
        );
        assert_eq!(active.binding().digest().as_str(), digest1);
        let granted: Vec<&str> =
            active.grant().iter().map(|id| id.as_str()).collect();
        assert_eq!(granted, vec!["workspace-read"]);
        assert_eq!(lifecycle.state(), LifecycleState::Active);
        assert!(lifecycle.active().is_some());
        // Active -> Active is rejected with the typed active failure,
        // and the original session is preserved exactly (no counter
        // advancement, no binding/grant change).
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::Active)
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Active);
        let preserved = lifecycle.active().expect("session preserved");
        assert_eq!(preserved.session_id(), 1);
        assert_eq!(preserved.binding().digest().as_str(), digest1);
        // The deeper identity/ABI checks run while enabled, not active.
        lifecycle.deactivate().expect("deactivate");
        // Wrong digest fails before any semantic work.
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest(2)),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
        // Wrong id fails.
        assert!(matches!(
            lifecycle.activate(
                request("other-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
        // An ABI that identifies neither the installed package nor
        // the Host fails closed as a package identity mismatch (the
        // package-ABI gate precedes the Host-compatibility gate).
        assert!(matches!(
            lifecycle.activate(
                ActivationRequest::parse(
                    "conformance-domain",
                    &digest1,
                    "siralos:domain-abi@9.9.9",
                    &ids(&["workspace-read"]),
                )
                .unwrap(),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
        // Capability denial is typed and does not escalate.
        let narrow_authority = HostAuthority::parse(&[]).unwrap();
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest1),
                &supported,
                &narrow_authority,
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::CapabilityDenied { missing })
            if missing.len() == 1
        ));
        // Runtime checks gate activation.
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::ResourceExceeded(
                    ResourceExceededKind::Fuel,
                ),
            ),
            Err(DomainFailure::ResourceExceeded { kind })
            if kind == ResourceExceededKind::Fuel
        ));
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Unavailable,
            ),
            Err(DomainFailure::Unavailable { .. })
        ));
    }

    #[test]
    fn session_scoped_activation_and_deactivation() {
        let mut lifecycle = DomainLifecycle::new();
        let digest1 = digest(1);
        lifecycle.install(package("conformance-domain", &digest1)).unwrap();
        lifecycle.enable().unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        let first = lifecycle
            .activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_eq!(first.session_id(), 1);
        lifecycle.deactivate().unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        // Disable and uninstall are refused while active only.
        let second = lifecycle
            .activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_eq!(second.session_id(), 2);
        assert!(matches!(lifecycle.uninstall(), Err(DomainFailure::Active)));
        assert!(matches!(lifecycle.disable(), Err(DomainFailure::Active)));
        lifecycle.deactivate().unwrap();
        lifecycle.disable().unwrap();
        lifecycle.uninstall().unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Absent);
    }

    #[test]
    fn eligibility_accumulates_deeper_reasons_in_fixed_order() {
        let mut lifecycle = DomainLifecycle::new();
        let digest1 = digest(1);
        lifecycle.install(package("conformance-domain", &digest1)).unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        let disabled = lifecycle.eligibility(
            &request("conformance-domain", &digest1),
            &supported,
            &authority(),
            &RuntimeCheckResult::Ready,
        );
        assert_eq!(disabled.reasons(), &[EligibilityReason::Disabled]);
        lifecycle.enable().unwrap();
        // Wrong digest + denied capability + failed runtime check all
        // accumulate, in the fixed check order.
        let request = ActivationRequest::parse(
            "conformance-domain",
            &digest(2),
            "siralos:domain-abi@1.0.0",
            &ids(&["workspace-read", "process-exec"]),
        )
        .unwrap();
        let narrow = HostAuthority::parse(&ids(&["workspace-read"])).unwrap();
        let eligibility = lifecycle.eligibility(
            &request,
            &supported,
            &narrow,
            &RuntimeCheckResult::ResourceExceeded(
                ResourceExceededKind::Memory,
            ),
        );
        assert_eq!(
            eligibility.reasons(),
            &[
                EligibilityReason::IdentityMismatch,
                EligibilityReason::UndeclaredCapability,
                EligibilityReason::CapabilityDenied,
                EligibilityReason::ResourceExceeded,
            ],
        );
        // The same request fails activation with the first reason.
        assert!(matches!(
            lifecycle.activate(
                request,
                &supported,
                &narrow,
                RuntimeCheckResult::ResourceExceeded(
                    ResourceExceededKind::Memory,
                ),
            ),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
    }

    #[test]
    fn active_eligibility_is_explicitly_not_ready() {
        let mut lifecycle = DomainLifecycle::new();
        let digest1 = digest(1);
        lifecycle.install(package("conformance-domain", &digest1)).unwrap();
        lifecycle.enable().unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        lifecycle
            .activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        let eligibility = lifecycle.eligibility(
            &request("conformance-domain", &digest1),
            &supported,
            &authority(),
            &RuntimeCheckResult::Ready,
        );
        assert!(!eligibility.ready());
        assert_eq!(eligibility.reasons(), &[EligibilityReason::Active]);
    }

    #[test]
    fn package_declaration_bounds_activation_requests() {
        let supported = DomainAbi::parse(ABI).unwrap();
        let authority_both =
            HostAuthority::parse(&ids(&["workspace-read", "process-exec"]))
                .unwrap();

        // Equal package/request capability set succeeds.
        let mut lifecycle = DomainLifecycle::new();
        let digest1 = digest(1);
        lifecycle
            .install(
                DomainPackage::parse(
                    "conformance-domain",
                    &digest1,
                    ABI,
                    &ids(&["workspace-read", "process-exec"]),
                )
                .unwrap(),
            )
            .unwrap();
        lifecycle.enable().unwrap();
        let equal = ActivationRequest::parse(
            "conformance-domain",
            &digest1,
            ABI,
            &ids(&["workspace-read", "process-exec"]),
        )
        .unwrap();
        let active = lifecycle
            .activate(
                equal,
                &supported,
                &authority_both,
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        let granted: Vec<&str> =
            active.grant().iter().map(|id| id.as_str()).collect();
        assert_eq!(granted, vec!["process-exec", "workspace-read"]);
        lifecycle.deactivate().unwrap();

        // A strict subset of the declaration succeeds.
        let subset = ActivationRequest::parse(
            "conformance-domain",
            &digest1,
            ABI,
            &ids(&["workspace-read"]),
        )
        .unwrap();
        let active = lifecycle
            .activate(
                subset,
                &supported,
                &authority_both,
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        let granted: Vec<&str> =
            active.grant().iter().map(|id| id.as_str()).collect();
        assert_eq!(granted, vec!["workspace-read"]);
        lifecycle.deactivate().unwrap();

        // A request exceeding the declaration fails typed, even when
        // Host authority contains the extra capability.
        let exceeding = ActivationRequest::parse(
            "conformance-domain",
            &digest1,
            ABI,
            &ids(&["workspace-read", "process-exec", "network-access"]),
        )
        .unwrap();
        assert!(matches!(
            lifecycle.activate(
                exceeding,
                &supported,
                &authority_both,
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::UndeclaredCapability { missing })
            if missing.len() == 1
                && missing[0].as_str() == "network-access"
        ));
        // The lifecycle is untouched by the failed request.
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());

        // Host authority narrows independently of the declaration.
        let narrow_authority =
            HostAuthority::parse(&ids(&["workspace-read"])).unwrap();
        let both = ActivationRequest::parse(
            "conformance-domain",
            &digest1,
            ABI,
            &ids(&["workspace-read", "process-exec"]),
        )
        .unwrap();
        assert!(matches!(
            lifecycle.activate(
                both,
                &supported,
                &narrow_authority,
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::CapabilityDenied { missing })
            if missing.len() == 1
                && missing[0].as_str() == "process-exec"
        ));

        // Undeclared capabilities are reported in canonical order.
        let unordered = ActivationRequest::parse(
            "conformance-domain",
            &digest1,
            ABI,
            &ids(&[
                "network-access",
                "workspace-read",
                "process-exec",
                "telemetry",
            ]),
        )
        .unwrap();
        match lifecycle.activate(
            unordered,
            &supported,
            &authority_both,
            RuntimeCheckResult::Ready,
        ) {
            Err(DomainFailure::UndeclaredCapability { missing }) => {
                let missing: Vec<&str> =
                    missing.iter().map(|id| id.as_str()).collect();
                assert_eq!(missing, vec!["network-access", "telemetry"],);
            }
            other => panic!("unexpected activation outcome: {other:?}"),
        }
    }
    fn enabled_lifecycle(id: &str, digest: &str) -> DomainLifecycle {
        let mut lifecycle = DomainLifecycle::new();
        lifecycle.install(package(id, digest)).unwrap();
        lifecycle.enable().unwrap();
        lifecycle
    }

    /// The typed stale failure is the machine-branchable staleness
    /// code, never a misclassification of another failure class.
    fn assert_stale(result: Result<super::ActiveDomain, DomainFailure>) {
        assert!(matches!(result, Err(DomainFailure::StaleActivation)));
    }

    #[test]
    fn stale_preparation_fails_typed_after_disable() {
        let digest1 = digest(1);
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &DomainAbi::parse(ABI).unwrap(),
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        lifecycle.disable().unwrap();
        let generation_before = lifecycle.generation;
        assert_stale(lifecycle.commit_activation(
            prepared,
            &DomainAbi::parse(ABI).unwrap(),
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        // No mutation: the lifecycle stays Installed, the session
        // counter is untouched, and the generation did not advance.
        assert_eq!(lifecycle.state(), LifecycleState::Installed);
        assert!(lifecycle.active().is_none());
        assert_eq!(lifecycle.next_session, 1);
        assert_eq!(lifecycle.generation, generation_before);
    }

    #[test]
    fn stale_preparation_fails_typed_across_replacement() {
        let digest_a = digest(1);
        let digest_b = digest(2);
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest_a);
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest_a),
                &DomainAbi::parse(ABI).unwrap(),
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        lifecycle.disable().unwrap();
        lifecycle.uninstall().unwrap();
        lifecycle.install(package("other-domain", &digest_b)).unwrap();
        lifecycle.enable().unwrap();
        assert_stale(lifecycle.commit_activation(
            prepared,
            &DomainAbi::parse(ABI).unwrap(),
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        // Package B remains Enabled with no A binding attached and no
        // session allocated.
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        assert_eq!(
            lifecycle.installed_package().map(|p| p.id().as_str()),
            Some("other-domain"),
        );
        assert_eq!(lifecycle.next_session, 1);
    }

    #[test]
    fn stale_preparation_fails_after_same_package_restart() {
        let digest1 = digest(1);
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &DomainAbi::parse(ABI).unwrap(),
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        // Disable and re-enable the SAME package: the validated
        // lifecycle episode changed, so the old preparation is stale.
        lifecycle.disable().unwrap();
        lifecycle.enable().unwrap();
        assert_stale(lifecycle.commit_activation(
            prepared,
            &DomainAbi::parse(ABI).unwrap(),
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        assert_eq!(lifecycle.next_session, 1);
    }

    #[test]
    fn stale_preparation_fails_after_completed_activation() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        let first = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        lifecycle
            .commit_activation(
                first.clone(),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        lifecycle.deactivate().unwrap();
        // A second preparation is bound to the new episode and
        // commits; the first preparation can never cross the completed
        // activation lifecycle.
        let second = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_eq!(
            lifecycle
                .commit_activation(
                    second,
                    &supported,
                    &authority(),
                    RuntimeCheckResult::Ready,
                )
                .unwrap()
                .session_id(),
            2,
        );
        assert_eq!(lifecycle.active().unwrap().session_id(), 2);
        lifecycle.deactivate().unwrap();
        assert_stale(lifecycle.commit_activation(
            first,
            &supported,
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        // Two committed activations, no session consumed by the stale
        // attempt: the next valid commit gets session 3.
        assert_eq!(lifecycle.next_session, 3);
    }

    #[test]
    fn immediate_prepare_commit_binds_exactly_and_allocates_once() {
        let digest1 = digest(1);
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        let active = lifecycle
            .commit_activation(
                prepared,
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        // Exact binding, correct grant, exactly one session allocation.
        assert_eq!(active.session_id(), 1);
        assert_eq!(lifecycle.next_session, 2);
        let package = lifecycle.installed_package().unwrap();
        assert!(active.binding().matches(package));
        assert_eq!(
            active.binding().package_id().as_str(),
            "conformance-domain"
        );
        assert_eq!(active.binding().digest().as_str(), digest1);
        let granted: Vec<&str> =
            active.grant().iter().map(|id| id.as_str()).collect();
        assert_eq!(granted, vec!["workspace-read"]);
        assert_eq!(lifecycle.state(), LifecycleState::Active);
    }

    #[test]
    fn commit_misuse_returns_typed_failure_without_panic() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        // Double commit: the second commit of the same preparation is
        // stale because the first commit advanced the generation.
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        lifecycle
            .commit_activation(
                prepared.clone(),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_stale(lifecycle.commit_activation(
            prepared,
            &DomainAbi::parse(ABI).unwrap(),
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Active);
        assert_eq!(lifecycle.active().unwrap().session_id(), 1);
        assert_eq!(lifecycle.next_session, 2);
        // A preparation from a different lifecycle (fresh slot) is a
        // publicly reachable invalid commit and fails typed, leaving
        // the fresh lifecycle untouched.
        let mut other = DomainLifecycle::new();
        let foreign = enabled_lifecycle("conformance-domain", &digest1)
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_stale(other.commit_activation(
            foreign,
            &supported,
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        assert_eq!(other.state(), LifecycleState::Absent);
        assert_eq!(other.next_session, 1);
    }

    #[test]
    fn failed_operations_do_not_advance_generation() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let mut lifecycle = DomainLifecycle::new();
        // Failed operations on the fresh lifecycle.
        assert!(lifecycle.enable().is_err());
        assert!(lifecycle.disable().is_err());
        assert!(lifecycle.uninstall().is_err());
        assert!(lifecycle.deactivate().is_err());
        assert_eq!(lifecycle.generation, 0);
        lifecycle.install(package("conformance-domain", &digest1)).unwrap();
        assert_eq!(lifecycle.generation, 1);
        // Duplicate install fails without advancing.
        assert!(
            lifecycle
                .install(package("conformance-domain", &digest1))
                .is_err()
        );
        assert_eq!(lifecycle.generation, 1);
        // Already-disabled disable fails without advancing.
        assert!(lifecycle.disable().is_err());
        assert_eq!(lifecycle.generation, 1);
        lifecycle.enable().unwrap();
        assert_eq!(lifecycle.generation, 2);
        // Already-enabled enable fails without advancing.
        assert!(lifecycle.enable().is_err());
        assert_eq!(lifecycle.generation, 2);
        // A rejected activation (wrong digest) fails without advancing.
        assert!(
            lifecycle
                .activate(
                    request("conformance-domain", &digest(2)),
                    &supported,
                    &authority(),
                    RuntimeCheckResult::Ready,
                )
                .is_err()
        );
        assert_eq!(lifecycle.generation, 2);
        // Deactivate without an active session fails without advancing.
        assert!(lifecycle.deactivate().is_err());
        assert_eq!(lifecycle.generation, 2);
    }

    #[test]
    fn session_ids_advance_only_for_committed_activations() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        // A stale commit creates no observable session gap.
        lifecycle.disable().unwrap();
        assert_stale(lifecycle.commit_activation(
            prepared,
            &DomainAbi::parse(ABI).unwrap(),
            &authority(),
            RuntimeCheckResult::Ready,
        ));
        lifecycle.enable().unwrap();
        let active = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        let committed = lifecycle
            .commit_activation(
                active,
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_eq!(committed.session_id(), 1);
        // Deactivate, prepare again, commit: the next monotonic
        // committed session id is 2.
        lifecycle.deactivate().unwrap();
        let active = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        assert_eq!(
            lifecycle
                .commit_activation(
                    active,
                    &supported,
                    &authority(),
                    RuntimeCheckResult::Ready,
                )
                .unwrap()
                .session_id(),
            2,
        );
    }

    fn broad_authority() -> HostAuthority {
        HostAuthority::parse(&ids(&["workspace-read", "process-exec"]))
            .unwrap()
    }

    fn read_authority() -> HostAuthority {
        HostAuthority::parse(&ids(&["workspace-read"])).unwrap()
    }

    fn package_declaring(
        capabilities: &[&str],
        digest: &str,
    ) -> DomainPackage {
        DomainPackage::parse(
            "conformance-domain",
            digest,
            ABI,
            &ids(capabilities),
        )
        .unwrap()
    }

    fn request_capabilities(
        capabilities: &[&str],
        digest: &str,
    ) -> ActivationRequest {
        ActivationRequest::parse(
            "conformance-domain",
            digest,
            ABI,
            &ids(capabilities),
        )
        .unwrap()
    }

    fn denied_missing(
        result: Result<super::ActiveDomain, DomainFailure>,
    ) -> Vec<String> {
        match result {
            Err(DomainFailure::CapabilityDenied { missing }) => {
                missing.iter().map(|id| id.as_str().to_owned()).collect()
            }
            other => panic!("expected capability denial, got: {other:?}"),
        }
    }

    #[test]
    fn same_lifecycle_authority_narrowing_fails_closed() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let mut lifecycle = DomainLifecycle::new();
        lifecycle
            .install(package_declaring(
                &["workspace-read", "process-exec"],
                &digest1,
            ))
            .unwrap();
        lifecycle.enable().unwrap();
        let prepared = lifecycle
            .prepare_activation(
                &request_capabilities(
                    &["workspace-read", "process-exec"],
                    &digest1,
                ),
                &supported,
                &broad_authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        // The commit-time authority narrows to [read]: the prepared
        // activation must NOT carry the earlier broader grant. The
        // generation is unchanged (the failed commit mutates nothing),
        // so this is a pure authority denial, not lifecycle staleness.
        let denied = lifecycle.commit_activation(
            prepared,
            &supported,
            &read_authority(),
            RuntimeCheckResult::Ready,
        );
        assert_eq!(denied_missing(denied), vec!["process-exec"]);
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        assert_eq!(lifecycle.next_session, 1);
    }

    #[test]
    fn prepared_activation_cannot_import_broader_authority_across_lifecycles()
    {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let request = request_capabilities(
            &["workspace-read", "process-exec"],
            &digest1,
        );
        let declared =
            package_declaring(&["workspace-read", "process-exec"], &digest1);
        // Lifecycle A prepares under broad authority.
        let mut a = DomainLifecycle::new();
        a.install(declared.clone()).unwrap();
        a.enable().unwrap();
        let prepared = a
            .prepare_activation(
                &request,
                &supported,
                &broad_authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        // Lifecycle B installs and enables the SAME package, so its
        // generation numerically equals A's — the security invariant
        // must not depend on globally unique generations.
        let mut b = DomainLifecycle::new();
        b.install(declared).unwrap();
        b.enable().unwrap();
        assert_eq!(
            a.generation, b.generation,
            "independent lifecycles coincide numerically by design",
        );
        // Committing A's preparation through B under B's narrower
        // authority must fail: preparation never creates durable
        // authority, and equal generations transfer none.
        let denied = b.commit_activation(
            prepared,
            &supported,
            &read_authority(),
            RuntimeCheckResult::Ready,
        );
        assert_eq!(denied_missing(denied), vec!["process-exec"]);
        assert_eq!(b.state(), LifecycleState::Enabled);
        assert!(b.active().is_none());
        assert_eq!(b.next_session, 1);
    }

    #[test]
    fn cloned_lifecycle_state_does_not_transfer_prepared_authority() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let declared =
            package_declaring(&["workspace-read", "process-exec"], &digest1);
        let mut original = DomainLifecycle::new();
        original.install(declared).unwrap();
        original.enable().unwrap();
        let prepared = original
            .prepare_activation(
                &request_capabilities(
                    &["workspace-read", "process-exec"],
                    &digest1,
                ),
                &supported,
                &broad_authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        // A clone duplicates lifecycle state (same package, same
        // generation value) but carries no authority of its own.
        let mut copy = original.clone();
        assert_eq!(original.generation, copy.generation);
        let denied = copy.commit_activation(
            prepared.clone(),
            &supported,
            &read_authority(),
            RuntimeCheckResult::Ready,
        );
        assert_eq!(denied_missing(denied), vec!["process-exec"]);
        assert_eq!(copy.state(), LifecycleState::Enabled);
        assert!(copy.active().is_none());
        assert_eq!(copy.next_session, 1);
        // Positive control: the SAME prepared activation committed
        // through the copy under an equally broad commit-time
        // authority is authorized BY THAT AUTHORITY — the gate is the
        // commit-time Host policy, not provenance.
        let active = copy
            .commit_activation(
                prepared,
                &supported,
                &broad_authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        let granted: Vec<&str> =
            active.grant().iter().map(|id| id.as_str()).collect();
        assert_eq!(granted, vec!["process-exec", "workspace-read"]);
        assert_eq!(active.session_id(), 1);
    }

    #[test]
    fn wider_commit_authority_never_widens_the_activation_request() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let mut lifecycle = enabled_lifecycle("conformance-domain", &digest1);
        // The request asks only for [read]; preparation happens under
        // a [read] authority and the commit-time authority is broader.
        let prepared = lifecycle
            .prepare_activation(
                &request("conformance-domain", &digest1),
                &supported,
                &read_authority(),
                &RuntimeCheckResult::Ready,
            )
            .unwrap();
        let active = lifecycle
            .commit_activation(
                prepared,
                &supported,
                &broad_authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        let granted: Vec<&str> =
            active.grant().iter().map(|id| id.as_str()).collect();
        assert_eq!(
            granted,
            vec!["workspace-read"],
            "the activation request is the upper bound",
        );
        assert_eq!(active.session_id(), 1);
    }

    /// Every successful grant satisfies the narrowing chain:
    /// grant <= request <= package declaration, and grant <= the
    /// commit-time Host authority.
    #[test]
    fn successful_grants_are_subsets_of_request_declaration_and_authority() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        type GrantCase = (
            Vec<&'static str>,
            Vec<&'static str>,
            Vec<&'static str>,
            Vec<&'static str>,
        );
        let cases: Vec<GrantCase> = vec![
            // request, declaration, prepare+commit authority, expected grant
            (
                vec!["workspace-read"],
                vec!["workspace-read", "process-exec"],
                vec!["workspace-read", "process-exec"],
                vec!["workspace-read"],
            ),
            (
                vec!["workspace-read", "process-exec"],
                vec!["workspace-read", "process-exec"],
                vec!["workspace-read", "process-exec"],
                vec!["process-exec", "workspace-read"],
            ),
            (
                vec!["workspace-read"],
                vec!["workspace-read"],
                vec!["workspace-read", "process-exec"],
                vec!["workspace-read"],
            ),
        ];
        for (request_caps, declaration, authority_caps, expected) in cases {
            let mut lifecycle = DomainLifecycle::new();
            lifecycle
                .install(package_declaring(&declaration, &digest1))
                .unwrap();
            lifecycle.enable().unwrap();
            let authority =
                HostAuthority::parse(&ids(&authority_caps)).unwrap();
            let prepared = lifecycle
                .prepare_activation(
                    &request_capabilities(&request_caps, &digest1),
                    &supported,
                    &authority,
                    &RuntimeCheckResult::Ready,
                )
                .unwrap();
            let active = lifecycle
                .commit_activation(
                    prepared,
                    &supported,
                    &authority,
                    RuntimeCheckResult::Ready,
                )
                .unwrap();
            let granted: Vec<&str> =
                active.grant().iter().map(|id| id.as_str()).collect();
            assert_eq!(granted, expected);
            let installed = lifecycle.installed_package().unwrap();
            assert!(active.binding().matches(installed));
            for capability in &granted {
                assert!(
                    request_caps.contains(capability),
                    "grant must not exceed the request: {capability}",
                );
                assert!(
                    declaration.contains(capability),
                    "grant must not exceed the declaration: {capability}",
                );
                assert!(
                    authority_caps.contains(capability),
                    "grant must not exceed the commit-time authority: {capability}",
                );
            }
        }
    }

    /// Table-driven invariant check: whenever the lifecycle is Active,
    /// the installed package exists, the active binding matches it
    /// exactly, and the session id belongs to a successful committed
    /// activation (the monotonic commit count).
    #[test]
    fn active_state_invariants_hold_across_lifecycle_sequences() {
        let supported = DomainAbi::parse(ABI).unwrap();
        let sequences: Vec<Vec<&str>> = vec![
            vec!["activate"],
            vec!["activate", "deactivate", "activate"],
            vec!["disable", "enable", "activate"],
            vec!["activate", "deactivate", "disable", "enable", "activate"],
            vec!["deactivate", "activate", "activate"],
        ];
        for sequence in sequences {
            let digest1 = digest(1);
            let mut lifecycle =
                enabled_lifecycle("conformance-domain", &digest1);
            let mut commits = 0;
            for step in sequence {
                match step {
                    "activate" => {
                        if lifecycle.state() == LifecycleState::Enabled {
                            let active = lifecycle
                                .activate(
                                    request("conformance-domain", &digest1),
                                    &supported,
                                    &authority(),
                                    RuntimeCheckResult::Ready,
                                )
                                .unwrap();
                            assert_eq!(active.session_id(), commits + 1);
                            commits += 1;
                        } else {
                            assert!(matches!(
                                lifecycle.activate(
                                    request("conformance-domain", &digest1),
                                    &supported,
                                    &authority(),
                                    RuntimeCheckResult::Ready,
                                ),
                                Err(DomainFailure::Active)
                            ));
                        }
                    }
                    "deactivate" => {
                        if lifecycle.state() == LifecycleState::Active {
                            lifecycle.deactivate().unwrap();
                        } else {
                            assert!(matches!(
                                lifecycle.deactivate(),
                                Err(DomainFailure::NotActive)
                            ));
                        }
                    }
                    "disable" => lifecycle.disable().unwrap(),
                    "enable" => lifecycle.enable().unwrap(),
                    _ => panic!("unknown sequence step"),
                }
                if lifecycle.state() == LifecycleState::Active {
                    let package =
                        lifecycle.installed_package().expect("installed");
                    let active = lifecycle.active().expect("active session");
                    assert!(
                        active.binding().matches(package),
                        "active binding must exactly match the installed \
                         package (id, digest, and ABI)",
                    );
                    assert_eq!(
                        active.session_id(),
                        commits,
                        "session id belongs to the last committed activation",
                    );
                }
            }
        }
    }

    /// A Host-compatible request ABI can never substitute for the ABI
    /// declared by the installed package: the requested ABI must
    /// identify the installed package exactly.
    #[test]
    fn package_abi_mismatch_is_rejected_even_when_host_supports_request_abi() {
        let digest1 = digest(1);
        let mut lifecycle = DomainLifecycle::new();
        lifecycle
            .install(
                DomainPackage::parse(
                    "conformance-domain",
                    &digest1,
                    "siralos:domain-abi@1.1.0",
                    &ids(&["workspace-read"]),
                )
                .unwrap(),
            )
            .unwrap();
        lifecycle.enable().unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        // Eligibility must not report ready: the ABI differs from the
        // installed package even though it matches the Host.
        let eligibility = lifecycle.eligibility(
            &request("conformance-domain", &digest1),
            &supported,
            &authority(),
            &RuntimeCheckResult::Ready,
        );
        assert!(!eligibility.ready());
        assert_eq!(
            eligibility.reasons(),
            &[EligibilityReason::IdentityMismatch],
        );
        // Activation is rejected before any PreparedActivation exists,
        // with zero lifecycle mutation and zero session consumption.
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        assert_eq!(lifecycle.next_session, 1);
        // The request ABI that DOES identify the package is then
        // rejected only by the Host-compatibility gate.
        assert!(matches!(
            lifecycle.activate(
                ActivationRequest::parse(
                    "conformance-domain",
                    &digest1,
                    "siralos:domain-abi@1.1.0",
                    &ids(&["workspace-read"]),
                )
                .unwrap(),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::UnsupportedAbi { .. })
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert_eq!(lifecycle.next_session, 1);
    }

    /// Package and request ABI agreeing on an ABI the Host does not
    /// support is the distinct unsupported/incompatible-ABI failure.
    #[test]
    fn matching_package_and_request_abi_unsupported_by_host_fails_typed() {
        let digest1 = digest(1);
        let mut lifecycle = DomainLifecycle::new();
        lifecycle
            .install(
                DomainPackage::parse(
                    "conformance-domain",
                    &digest1,
                    "siralos:domain-abi@1.1.0",
                    &ids(&["workspace-read"]),
                )
                .unwrap(),
            )
            .unwrap();
        lifecycle.enable().unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        let request = ActivationRequest::parse(
            "conformance-domain",
            &digest1,
            "siralos:domain-abi@1.1.0",
            &ids(&["workspace-read"]),
        )
        .unwrap();
        let eligibility = lifecycle.eligibility(
            &request,
            &supported,
            &authority(),
            &RuntimeCheckResult::Ready,
        );
        assert_eq!(
            eligibility.reasons(),
            &[EligibilityReason::UnsupportedAbi]
        );
        assert!(matches!(
            lifecycle.activate(
                request,
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::UnsupportedAbi { .. })
        ));
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        assert_eq!(lifecycle.next_session, 1);
    }

    /// The all-match path binds the exact installed package identity
    /// (id, digest, and ABI) on every successful activation path.
    #[test]
    fn successful_activations_always_satisfy_exact_binding_matches() {
        let digest1 = digest(1);
        let supported = DomainAbi::parse(ABI).unwrap();
        let mut lifecycle = DomainLifecycle::new();
        lifecycle.install(package("conformance-domain", &digest1)).unwrap();
        lifecycle.enable().unwrap();
        let active = lifecycle
            .activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            )
            .unwrap();
        let package = lifecycle.installed_package().unwrap();
        assert!(
            active.binding().matches(package),
            "successful activation must bind the installed package exactly",
        );
        assert_eq!(active.binding().abi().as_str(), ABI);
    }

    #[test]
    fn workspace_files_are_opaque_and_never_acquire() {
        // The marker name is deliberately a neutral string: core
        // sources stay free of product vocabulary, and workspace file
        // names are opaque regardless of their content.
        let files: Vec<String> = vec![
            "scene.project".to_owned(),
            "main.ts".to_owned(),
            "README.md".to_owned(),
        ];
        for file in &files {
            assert_eq!(
                classify_workspace_file(file),
                super::WORKSPACE_FILE_OPAQUE
            );
        }
        let scan = workspace_domain_scan(&files);
        assert_eq!(scan.classified, 3);
        assert_eq!(scan.candidates, 0);
        assert_eq!(scan.installs, 0);
        assert_eq!(scan.enables, 0);
        assert_eq!(scan.activations, 0);
        assert_eq!(scan.downloads, 0);
        assert_eq!(scan.recommendations, 0);
    }

    #[test]
    fn rejected_protocol_requests_do_not_advance_the_generation() {
        let mut lifecycle = DomainLifecycle::new();
        let digest1 = digest(1);
        lifecycle
            .install(
                DomainPackage::parse(
                    "conformance-domain",
                    &digest1,
                    "siralos:domain-abi@1.1.0",
                    &ids(&["workspace-read"]),
                )
                .unwrap(),
            )
            .unwrap();
        lifecycle.enable().unwrap();
        let supported = DomainAbi::parse(ABI).unwrap();
        // Package and request ABI agree but the Host does not support
        // them: the typed unsupported-ABI failure, generation unchanged.
        let first = lifecycle.activate(
            ActivationRequest::parse(
                "conformance-domain",
                &digest1,
                "siralos:domain-abi@1.1.0",
                &ids(&["workspace-read"]),
            )
            .unwrap(),
            &supported,
            &authority(),
            RuntimeCheckResult::Ready,
        );
        assert!(matches!(first, Err(DomainFailure::UnsupportedAbi { .. })));
        assert_eq!(
            lifecycle.generation, 2,
            "failed activate must not advance"
        );
        // A Host-compatible request cannot substitute for the package
        // ABI: the typed identity mismatch, generation unchanged, no
        // session consumed.
        assert!(matches!(
            lifecycle.activate(
                request("conformance-domain", &digest1),
                &supported,
                &authority(),
                RuntimeCheckResult::Ready,
            ),
            Err(DomainFailure::IdentityMismatch { .. })
        ));
        assert_eq!(lifecycle.generation, 2);
        assert_eq!(lifecycle.state(), LifecycleState::Enabled);
        assert!(lifecycle.active().is_none());
        assert_eq!(lifecycle.next_session, 1);
    }
}
