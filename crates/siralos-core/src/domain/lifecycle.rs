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
/// the exact binding and the effective grant. The authoritative
/// transition happens only when the Host commits the prepared
/// activation, so no fallible runtime step can ever leave the lifecycle
/// Active without a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedActivation {
    binding: ActivationBinding,
    grant: CapabilityGrant,
}

impl PreparedActivation {
    /// The exact identity that will be bound on commit.
    pub fn binding(&self) -> &ActivationBinding {
        &self.binding
    }

    /// The effective grant that will apply on commit.
    pub fn grant(&self) -> &CapabilityGrant {
        &self.grant
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainLifecycle {
    state: DomainState,
    next_session: u64,
}

impl Default for DomainLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainLifecycle {
    /// A fresh lifecycle with no package installed.
    pub fn new() -> Self {
        Self { state: DomainState::Absent, next_session: 1 }
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
            DomainState::Installed(_) | DomainState::Enabled(_) => Ok(()),
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
    /// identity, the ABI, the package-declaration capability ceiling,
    /// the Host policy decision, and the resource/runtime check all run
    /// here, so every fallible preparation happens before the
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
            binding: ActivationBinding::from_request(request),
            grant,
        })
    }

    /// Commit a prepared activation: the single authoritative
    /// Enabled -> Active transition. Infallible by construction — the
    /// Host performs every fallible runtime step before calling this —
    /// and it allocates the session id only for committed activations,
    /// so failed provisional attempts never advance the session
    /// counter.
    ///
    /// # Panics
    ///
    /// Panics on an internal invariant violation (the lifecycle is no
    /// longer Enabled); the Host flow guarantees the prepared
    /// activation is committed immediately.
    pub fn commit_activation(
        &mut self,
        prepared: PreparedActivation,
    ) -> ActiveDomain {
        let package =
            match std::mem::replace(&mut self.state, DomainState::Absent) {
                DomainState::Enabled(package) => package,
                _ => {
                    panic!("commit_activation requires the enabled state");
                }
            };
        let active = ActiveDomain {
            session_id: self.next_session,
            binding: prepared.binding,
            grant: prepared.grant,
        };
        self.next_session += 1;
        self.state = DomainState::Active { package, active: active.clone() };
        active
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
        Ok(self.commit_activation(prepared))
    }
    /// End the current run/session-scoped activation. The package
    /// stays installed and enabled.
    pub fn deactivate(&mut self) -> Result<(), DomainFailure> {
        match std::mem::replace(&mut self.state, DomainState::Absent) {
            DomainState::Active { package, .. } => {
                self.state = DomainState::Enabled(package);
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
        // Incompatible ABI fails closed.
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
            Err(DomainFailure::UnsupportedAbi { .. })
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
}
