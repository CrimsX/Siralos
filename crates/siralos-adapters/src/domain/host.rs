//! Domain host: lifecycle semantics plus the Component Model runtime
//! (Stage 3R R6, ADR 0034).
//!
//! The host owns installation/enablement/activation state through
//! `siralos-core::domain`, and it executes activated domains through
//! the versioned WIT world (`wit/domain-abi.wit`). Every activation
//! re-verifies the exact component bytes: the digest is computed from
//! the bytes the host accepts, and any stale or wrong identity fails
//! before any semantic work. Calls are fuel-bounded and input/output
//! bounded; traps stay contained in the runtime and surface as typed
//! failures.

use crate::domain::effects::{
    EffectMediation, EffectMediationBounds, EffectMediator, MediatedAnswer,
    validate_effect_request,
};
use crate::workspace::fs::{BoundedReadOutcome, read_complete_bounded};

use siralos_core::domain::capability::{CapabilityGrant, HostAuthority};
use siralos_core::domain::failure::{DomainFailure, ResourceExceededKind};
use siralos_core::domain::lifecycle::{
    ActivationRequest, ActiveDomain, DomainLifecycle, LifecycleState,
    RuntimeCheckResult,
};
use siralos_core::domain::package::{
    DomainAbi, DomainPackage, PackageDigest, verify_package_digest,
};
use siralos_core::identity::sha256_hex;

use std::fs::{File, Metadata, OpenOptions};
use std::path::{Path, PathBuf};

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{
    ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView,
};

wasmtime::component::bindgen!({
    path: "wit/domain-abi.wit",
    world: "siralos-domain",
});

/// Re-export the world's generated effect request type.
pub use siralos::domain_abi::domain_api::EffectRequest;

/// Host runtime bounds for one domain host.
#[derive(Debug, Clone)]
pub struct DomainHostBounds {
    /// Maximum component bytes accepted at install/activation.
    pub max_component_bytes: usize,
    /// Maximum query input bytes per call.
    pub max_query_bytes: usize,
    /// Maximum semantic result bytes per call.
    pub max_result_bytes: usize,
    /// Maximum guest memory bytes (wasmtime store limit).
    pub max_memory_bytes: u64,
    /// Fuel granted per call (execution/work budget).
    pub fuel_per_call: u64,
    /// Effect mediation bounds.
    pub effects: EffectMediationBounds,
}

impl Default for DomainHostBounds {
    fn default() -> Self {
        Self {
            max_component_bytes: 16 * 1024 * 1024,
            max_query_bytes: 64 * 1024,
            max_result_bytes: 64 * 1024,
            max_memory_bytes: 64 * 1024 * 1024,
            fuel_per_call: 100_000,
            effects: EffectMediationBounds {
                max_answer_bytes: 64 * 1024,
                max_workspace_read_bytes: 512 * 1024,
                max_host_calls: 64,
            },
        }
    }
}

/// The typed outcome of one semantic query call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryOutcome {
    /// The query succeeded with a bounded semantic result.
    Ok {
        /// The package id the guest bound.
        package_id: String,
        /// The query text echoed by the guest.
        query: String,
        /// The guest-computed node count.
        node_count: u32,
        /// The guest-observed source bytes.
        source_bytes: u32,
    },
    /// The guest rejected the query with a bounded reason.
    Rejected {
        /// Bounded guest reason.
        reason: String,
    },
    /// The host refused the call (no session, cancelled, bound).
    Refused(DomainFailure),
    /// The runtime trapped or exhausted a bound.
    Failed(DomainFailure),
}

/// Host state stored in the wasmtime store. The store limits live in
/// the state so the limiter closure can borrow them with the state's
/// lifetime (the canonical wasmtime pattern).
struct HostState {
    mediator: EffectMediator,
    limits: StoreLimits,
    /// The minimal WASI context granted to the component: the
    /// wasm32-wasip2 std plumbing interfaces with an empty environment,
    /// no arguments, and no filesystem preopens. This carries no
    /// filesystem, network, or process authority.
    wasi: WasiCtx,
    table: ResourceTable,
    /// The typed outcome of the most recent mediation attempt, so the
    /// Host retains machine-branchable resource classifications (for
    /// example HostCalls) even though the guest protocol only carries a
    /// bounded disposition.
    last_mediation: Option<EffectMediation>,
    /// Exact request corresponding to `last_mediation`; a guest return value
    /// is never authoritative without this host-observed binding.
    last_mediation_request: Option<EffectRequest>,
}

fn guest_answer_text_len(
    answer: &exports::siralos::domain_abi::domain_api::HostAnswer,
) -> Option<usize> {
    match answer {
        exports::siralos::domain_abi::domain_api::HostAnswer::Ok(text)
        | exports::siralos::domain_abi::domain_api::HostAnswer::Denied(text)
        | exports::siralos::domain_abi::domain_api::HostAnswer::Error(text) => {
            Some(text.len())
        }
        exports::siralos::domain_abi::domain_api::HostAnswer::Cancelled => {
            None
        }
    }
}

fn answer_text_len(answer: &MediatedAnswer) -> Option<usize> {
    match answer {
        MediatedAnswer::Ok(text)
        | MediatedAnswer::Denied(text)
        | MediatedAnswer::Error(text) => Some(text.len()),
        MediatedAnswer::Cancelled => None,
    }
}

fn effect_requests_equal(left: &EffectRequest, right: &EffectRequest) -> bool {
    match (left, right) {
        (
            EffectRequest::WorkspaceRead((left_path, left_max)),
            EffectRequest::WorkspaceRead((right_path, right_max)),
        ) => left_path == right_path && left_max == right_max,
        (
            EffectRequest::ProcessExec(left),
            EffectRequest::ProcessExec(right),
        ) => left == right,
        _ => false,
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

// The world's linker requires a Host implementation for every
// interface it names. The domain-api interface is the component's
// EXPORT surface: the host calls it, never the reverse, so these
// methods are never invoked by the runtime for this host.
impl siralos::domain_abi::domain_api::Host for HostState {
    fn bind(
        &mut self,
        _identity: siralos::domain_abi::domain_api::PackageIdentity,
    ) -> Result<(), String> {
        unreachable!("the domain-api export is provided by the component")
    }

    fn query(
        &mut self,
        _text: String,
    ) -> Result<siralos::domain_abi::domain_api::SemanticResult, String> {
        unreachable!("the domain-api export is provided by the component")
    }

    fn request_effect(
        &mut self,
        _request: EffectRequest,
    ) -> siralos::domain_abi::domain_api::HostAnswer {
        unreachable!("the domain-api export is provided by the component")
    }
}

impl siralos::domain_abi::host_effects::Host for HostState {
    fn perform(
        &mut self,
        request: EffectRequest,
    ) -> siralos::domain_abi::domain_api::HostAnswer {
        // Charge the Host-call budget for every import, including malformed
        // requests. Validation remains ahead of capability/filesystem work,
        // but an invalid guest call must not bypass the per-session budget.
        let valid = validate_effect_request(&request).is_ok();
        let outcome = self.mediator.mediate(&request);
        self.last_mediation = Some(outcome.clone());
        if !valid {
            self.last_mediation_request = None;
            return siralos::domain_abi::domain_api::HostAnswer::Error(
                "invalid effect request".to_owned(),
            );
        }
        self.last_mediation_request = Some(request.clone());
        match outcome {
            EffectMediation::Answer(answer) => match answer {
                MediatedAnswer::Ok(text) => {
                    siralos::domain_abi::domain_api::HostAnswer::Ok(text)
                }
                MediatedAnswer::Denied(reason) => {
                    siralos::domain_abi::domain_api::HostAnswer::Denied(reason)
                }
                MediatedAnswer::Cancelled => {
                    siralos::domain_abi::domain_api::HostAnswer::Cancelled
                }
                MediatedAnswer::Error(reason) => {
                    siralos::domain_abi::domain_api::HostAnswer::Error(reason)
                }
            },
            EffectMediation::ResourceExceeded(_) => {
                // The guest still receives a bounded disposition; the
                // Host's typed classification is retained separately.
                siralos::domain_abi::domain_api::HostAnswer::Error(
                    "host-call budget exceeded".to_owned(),
                )
            }
        }
    }
}

/// One loaded, bound, active domain session. The authoritative active
/// domain (binding + grant) stays in the lifecycle; the session holds
/// the runtime handles.
struct HostSession {
    store: Store<HostState>,
    instance: SiralosDomain,
    cancelled: bool,
}

/// The domain host: lifecycle state plus the runtime boundary.
pub struct DomainHost {
    lifecycle: DomainLifecycle,
    supported_abi: DomainAbi,
    authority: HostAuthority,
    component_path: PathBuf,
    workspace_root: PathBuf,
    bounds: DomainHostBounds,
    session: Option<HostSession>,
}

fn open_component_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NONBLOCK prevents a FIFO substitution from blocking the host;
        // O_NOFOLLOW refuses a leaf link before any bytes are read.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open the reparse point itself; a leaf link is rejected by the
        // handle metadata check rather than followed to its target.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

// On Unix this compares the stable device/inode identity and nanosecond
// mutation timestamps. Stable std does not expose a Windows file index, so
// the Windows branch uses the strongest portable metadata snapshot and the
// post-read path/handle checks below.
fn same_component_identity(initial: &Metadata, observed: &Metadata) -> bool {
    if !initial.is_file() || !observed.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return initial.file_type() == observed.file_type()
            && initial.dev() == observed.dev()
            && initial.ino() == observed.ino()
            && initial.len() == observed.len()
            && initial.mtime() == observed.mtime()
            && initial.mtime_nsec() == observed.mtime_nsec()
            && initial.ctime() == observed.ctime()
            && initial.ctime_nsec() == observed.ctime_nsec();
    }
    #[cfg(not(unix))]
    {
        initial.file_type() == observed.file_type()
            && initial.len() == observed.len()
            && initial.modified().ok() == observed.modified().ok()
    }
}

const MAX_COMPONENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_QUERY_BYTES: usize = 1024 * 1024;
const MAX_RESULT_BYTES: usize = 1024 * 1024;
const MAX_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FUEL_PER_CALL: u64 = 10_000_000;
/// Ceiling for the node COUNT a guest may claim in one semantic result. A
/// count is bounded on its own scale; the byte bound belongs to
/// `max_result_bytes`.
const MAX_RESULT_NODES: u64 = 1_000_000;

fn bounded_host_bounds(mut bounds: DomainHostBounds) -> DomainHostBounds {
    bounds.max_component_bytes =
        bounds.max_component_bytes.min(MAX_COMPONENT_BYTES);
    bounds.max_query_bytes = bounds.max_query_bytes.min(MAX_QUERY_BYTES);
    bounds.max_result_bytes = bounds.max_result_bytes.min(MAX_RESULT_BYTES);
    bounds.max_memory_bytes = bounds.max_memory_bytes.min(MAX_MEMORY_BYTES);
    bounds.fuel_per_call = bounds.fuel_per_call.min(MAX_FUEL_PER_CALL);
    bounds.effects.max_answer_bytes =
        bounds.effects.max_answer_bytes.min(MAX_RESULT_BYTES);
    bounds.effects.max_workspace_read_bytes = bounds
        .effects
        .max_workspace_read_bytes
        .min(MAX_COMPONENT_BYTES as u64);
    bounds.effects.max_host_calls = bounds.effects.max_host_calls.min(10_000);
    bounds
}

impl DomainHost {
    /// A host for one domain slot over the given component file.
    pub fn new(
        supported_abi: DomainAbi,
        authority: HostAuthority,
        component_path: PathBuf,
        workspace_root: PathBuf,
        bounds: DomainHostBounds,
    ) -> Self {
        let bounds = bounded_host_bounds(bounds);
        Self {
            lifecycle: DomainLifecycle::new(),
            supported_abi,
            authority,
            component_path,
            workspace_root,
            bounds,
            session: None,
        }
    }

    /// The current lifecycle state.
    pub fn state(&self) -> LifecycleState {
        self.lifecycle.state()
    }

    /// The installed package, if any.
    pub fn installed_package(&self) -> Option<&DomainPackage> {
        self.lifecycle.installed_package()
    }

    /// The active session, if any.
    pub fn active(&self) -> Option<&ActiveDomain> {
        self.lifecycle.active()
    }

    /// Read the exact component bytes (bounded, regular file).
    ///
    /// The opened handle is the read authority: after the initial
    /// no-link/regular-file check, the handle and its final path metadata
    /// must still identify the same file. The bounded reader reads at most
    /// `max_component_bytes + 1`, so a file that grows after the metadata
    /// check cannot cause an unbounded host allocation.
    fn component_bytes(&self) -> Result<Vec<u8>, DomainFailure> {
        let initial = std::fs::symlink_metadata(&self.component_path)
            .map_err(|_error| DomainFailure::Unavailable {
                reason: "cannot inspect component".to_owned(),
            })?;
        if initial.file_type().is_symlink() || !initial.is_file() {
            return Err(DomainFailure::Unavailable {
                reason: "component must be a regular file".to_owned(),
            });
        }
        if initial.len() > self.bounds.max_component_bytes as u64 {
            return Err(DomainFailure::InvalidInput {
                reason: "component exceeds the byte bound".to_owned(),
            });
        }
        let initial_canonical = std::fs::canonicalize(&self.component_path)
            .map_err(|_error| DomainFailure::Unavailable {
                reason: "cannot resolve component path".to_owned(),
            })?;

        let mut file =
            open_component_file(&self.component_path).map_err(|_error| {
                DomainFailure::Unavailable {
                    reason: "cannot open component".to_owned(),
                }
            })?;
        let opened =
            file.metadata().map_err(|_error| DomainFailure::Unavailable {
                reason: "cannot inspect opened component".to_owned(),
            })?;
        if !same_component_identity(&initial, &opened) {
            return Err(DomainFailure::Unavailable {
                reason: "component identity changed before read".to_owned(),
            });
        }

        let bytes = match read_complete_bounded(
            &mut file,
            self.bounds.max_component_bytes,
        ) {
            Ok(BoundedReadOutcome::Complete(bytes)) => bytes,
            Ok(BoundedReadOutcome::TooLarge) => {
                return Err(DomainFailure::InvalidInput {
                    reason: "component exceeds the byte bound".to_owned(),
                });
            }
            Err(_error) => {
                return Err(DomainFailure::Unavailable {
                    reason: "cannot read component".to_owned(),
                });
            }
        };

        let after_read =
            file.metadata().map_err(|_error| DomainFailure::Unavailable {
                reason: "cannot recheck component".to_owned(),
            })?;
        let path_after_read = std::fs::symlink_metadata(&self.component_path)
            .map_err(|_error| DomainFailure::Unavailable {
                reason: "cannot recheck component path".to_owned(),
            })?;
        let final_canonical = std::fs::canonicalize(&self.component_path)
            .map_err(|_error| DomainFailure::Unavailable {
                reason: "cannot recheck component path".to_owned(),
            })?;
        if !same_component_identity(&opened, &after_read)
            || !same_component_identity(&initial, &path_after_read)
            || initial_canonical != final_canonical
            || bytes.len() as u64 != after_read.len()
        {
            return Err(DomainFailure::Unavailable {
                reason: "component identity changed during read".to_owned(),
            });
        }
        Ok(bytes)
    }

    /// Explicitly install a locally supplied package. The host reads
    /// the exact component bytes and verifies the digest itself.
    pub fn install(
        &mut self,
        package: DomainPackage,
    ) -> Result<(), DomainFailure> {
        let bytes = self.component_bytes()?;
        let computed = PackageDigest::parse(&sha256_hex(&bytes))?;
        verify_package_digest(package.digest(), &computed)?;
        self.lifecycle.install(package)
    }

    /// Explicitly remove the installed package.
    pub fn uninstall(&mut self) -> Result<(), DomainFailure> {
        self.lifecycle.uninstall()
    }

    /// Explicitly enable the installed package.
    pub fn enable(&mut self) -> Result<(), DomainFailure> {
        self.lifecycle.enable()
    }

    /// Explicitly disable the installed package.
    pub fn disable(&mut self) -> Result<(), DomainFailure> {
        self.lifecycle.disable()
    }

    /// Cancel the current session: further calls are refused with the
    /// typed cancelled outcome.
    pub fn cancel(&mut self) {
        if let Some(session) = &mut self.session {
            session.cancelled = true;
            session.store.data_mut().mediator.cancel();
        }
    }

    /// End the current run/session-scoped activation.
    pub fn deactivate(&mut self) -> Result<(), DomainFailure> {
        self.session = None;
        self.lifecycle.deactivate()
    }

    /// Stop the current session after a guest fault: the instance is
    /// no longer trustworthy, so the activation ends with the typed
    /// failure (containment, not recovery). The package stays installed
    /// and enabled; a new explicit activation starts a fresh session.
    fn stop_session(&mut self) {
        self.session = None;
        self.lifecycle.deactivate().ok();
    }

    /// Activate the installed, enabled package for this session.
    ///
    /// Order: pure lifecycle/authority gates run first, then exact bytes are
    /// re-verified and the component loaded/instantiated, then the exact
    /// identity is bound into the guest. The final commit revalidates the
    /// prepared activation against the current lifecycle episode and fails
    /// typed if anything changed after preparation, publishing no HostSession
    /// and leaving the lifecycle unchanged.
    pub fn activate(
        &mut self,
        request: ActivationRequest,
        runtime: RuntimeCheckResult,
    ) -> Result<ActiveDomain, DomainFailure> {
        if self.session.is_some() {
            return Err(DomainFailure::Active);
        }
        // 1. Pure lifecycle/authority gates run before any component I/O
        // or guest instantiation. A disabled, profile-denied, or
        // out-of-authority request cannot make the Host read a component.
        let prepared = self.lifecycle.prepare_activation(
            &request,
            &self.supported_abi,
            &self.authority,
            &runtime,
        )?;
        // 2. Exact bytes: the host recomputes the digest itself.
        let bytes = self.component_bytes()?;
        let computed = PackageDigest::parse(&sha256_hex(&bytes))?;
        verify_package_digest(request.digest(), &computed)?;
        // 2. Load: malformed bytes fail as invalid input.
        let engine = self.engine()?;
        let component =
            Component::from_binary(&engine, &bytes).map_err(|_error| {
                DomainFailure::InvalidInput {
                    reason: "component bytes are malformed".to_owned(),
                }
            })?;
        // 3. ABI identity: the component must export the exact
        //    versioned world interface. The WIT package version is part
        //    of the export name, so a component built against any other
        //    ABI version fails closed here, before instantiation and
        //    before any semantic work. This is the boundary-level
        //    complement to the lifecycle ABI check.
        let expected_export = expected_domain_export(&self.supported_abi);
        let export_names: Vec<String> = component
            .component_type()
            .exports(&engine)
            .map(|(name, _)| name.to_string())
            .collect();
        if !export_names.iter().any(|name| name == &expected_export) {
            return Err(DomainFailure::UnsupportedAbi {
                expected: self.supported_abi.as_str().to_owned(),
                found: request.abi().as_str().to_owned(),
            });
        }
        // Lifecycle preparation already ran before component I/O above.
        // 4. Instantiate: version/world-incompatible components fail
        //    explicitly; the component imports exactly host-effects,
        //    so any other import also fails here. The store is created
        //    with the prepared effective grant.
        // The provisional store's mediator is configured with the
        // NON-AUTHORITATIVE provisional grant (computed from the
        // prepare-time authority). The authoritative ActiveDomain
        // grant is recomputed by the final commit from the
        // commit-time Host authority; the provisional grant is
        // discarded with the store if that commit fails.
        //
        // Provisional-effect authority is mechanically safe: activate()
        // is synchronous, `authority` is an immutable private field
        // with no mutator (so no concurrent policy change exists), and
        // the SAME authority feeds both prepare (the provisional
        // grant) and the final commit (the final grant). A guest that
        // invokes host effects during instantiation or bind is
        // therefore mediated by exactly the grant the final commit
        // authorizes: the conformance guest deliberately exercises
        // bind-time effects (effect-bind / exec-bind markers) and the
        // conformance suite proves both the permitted and the
        // out-of-grant-denied paths.
        let mut store =
            self.store(prepared.provisional_grant().clone(), &engine)?;
        // Instantiation executes the component's canonical-ABI
        // initialization, which also consumes fuel; grant the call
        // budget for it.
        store.set_fuel(self.bounds.fuel_per_call).map_err(|_error| {
            DomainFailure::Unavailable {
                reason: "fuel unavailable".to_owned(),
            }
        })?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(
            |_error| DomainFailure::Unavailable {
                reason: "cannot link wasi plumbing".to_owned(),
            },
        )?;
        SiralosDomain::add_to_linker::<
            HostState,
            wasmtime::component::HasSelf<HostState>,
        >(&mut linker, |state: &mut HostState| state)
        .map_err(|_error| DomainFailure::Unavailable {
            reason: "cannot link host effects".to_owned(),
        })?;
        let instance =
            SiralosDomain::instantiate(&mut store, &component, &linker)
                .map_err(|_error| DomainFailure::UnsupportedAbi {
                    expected: self.supported_abi.as_str().to_owned(),
                    found: request.abi().as_str().to_owned(),
                })?;
        // 5. Bind the exact activation identity into the guest. The
        //    guest's exported interface takes the export-side identity
        //    type (the component ABI distinguishes export/import
        //    wrappers; both carry the identical WIT record).
        let identity =
            exports::siralos::domain_abi::domain_api::PackageIdentity {
                package_id: request.package_id().as_str().to_owned(),
                package_digest: request.digest().as_str().to_owned(),
                abi: request.abi().as_str().to_owned(),
            };
        // 5. Bind the exact activation identity into the guest. The
        //    guest's bind is fallible untrusted execution: a trap, a
        //    rejection, or a resource failure here leaves the lifecycle
        //    Enabled with no session — the authoritative commit happens
        //    only after every fallible step.
        let bound = instance
            .interface0
            .call_bind(&mut store, &identity)
            .map_err(|error| {
                // Nothing has been committed yet: the lifecycle is
                // still Enabled and no session exists, so the typed
                // failure is returned as-is.
                classify_trap(&error, self.bounds.max_result_bytes)
            })?;
        if let Err(reason) = bound {
            if reason.len() > self.bounds.max_result_bytes {
                return Err(DomainFailure::ResourceExceeded {
                    kind: ResourceExceededKind::OutputBytes,
                });
            }
            let _ = reason;
            return Err(DomainFailure::InvalidOutput {
                reason: "guest rejected the activation identity".to_owned(),
            });
        }
        // 6. Commit: the single authoritative Enabled -> Active
        //    transition. No fallible operation remains afterwards. A
        //    stale commit (any lifecycle transition since preparation)
        //    fails typed and publishes no HostSession; the provisional
        //    store and instance are simply dropped with the local
        //    state, so no rollback machinery is needed.
        let active = self.lifecycle.commit_activation(
            prepared,
            &self.supported_abi,
            &self.authority,
            runtime,
        )?;
        self.session = Some(HostSession { store, instance, cancelled: false });
        Ok(active)
    }

    /// One bounded semantic query call against the active session. A
    /// guest fault stops the session with the typed failure: the
    /// instance is no longer trustworthy after a trap.
    pub fn query(&mut self, text: &str) -> QueryOutcome {
        let cancelled =
            self.session.as_ref().is_some_and(|session| session.cancelled);
        if self.session.is_none() {
            return QueryOutcome::Refused(DomainFailure::NotActive);
        }
        if cancelled {
            return QueryOutcome::Refused(DomainFailure::Cancelled);
        }
        if text.len() > self.bounds.max_query_bytes {
            return QueryOutcome::Refused(DomainFailure::InvalidInput {
                reason: "query exceeds the input byte bound".to_owned(),
            });
        }
        let call_result = {
            let session =
                self.session.as_mut().expect("session checked above");
            if let Err(_error) =
                session.store.set_fuel(self.bounds.fuel_per_call)
            {
                return QueryOutcome::Failed(DomainFailure::Unavailable {
                    reason: "fuel unavailable".to_owned(),
                });
            }
            session.store.data_mut().last_mediation = None;
            session.store.data_mut().last_mediation_request = None;
            session.instance.interface0.call_query(&mut session.store, text)
        };
        // The guest may have consumed the effect budget during the
        // query; the Host observes the typed resource failure even
        // though the guest protocol carried only bounded dispositions.
        let mediation = self
            .session
            .as_ref()
            .and_then(|session| session.store.data().last_mediation.clone());
        if let Some(EffectMediation::ResourceExceeded(kind)) = mediation {
            return QueryOutcome::Failed(DomainFailure::ResourceExceeded {
                kind,
            });
        }
        match call_result {
            Ok(Ok(result)) => {
                // One deterministic aggregate accounting rule over the
                // complete returned representation: every
                // guest-controlled variable-length field counts toward
                // the single semantic result bound.
                let output_bytes =
                    result.package_id.len() + result.query.len();
                // The package identity is the cross-instance binding: a guest
                // cannot answer for a different activation. The result's
                // `query` field is NOT compared to the request text -- the
                // conformance guest returns its result payload there (the
                // `pad:<n>` query answers with a padded string), so demanding
                // an exact echo would refuse legitimate results. It is bounded
                // by the aggregate result bound below instead.
                if result.package_id
                    != self
                        .active()
                        .map(|active| active.binding().package_id().as_str())
                        .unwrap_or_default()
                {
                    return QueryOutcome::Failed(DomainFailure::InvalidOutput {
                        reason: "query result identity did not match the request"
                            .to_owned(),
                    });
                }
                if usize::try_from(result.source_bytes).unwrap_or(usize::MAX)
                    > self.bounds.max_result_bytes
                {
                    return QueryOutcome::Failed(
                        DomainFailure::ResourceExceeded {
                            kind: ResourceExceededKind::OutputBytes,
                        },
                    );
                }
                // `node_count` is a COUNT, not a byte count: it is bounded by
                // its own ceiling, never against the byte bound.
                if u64::from(result.node_count) > MAX_RESULT_NODES {
                    return QueryOutcome::Failed(
                        DomainFailure::ResourceExceeded {
                            kind: ResourceExceededKind::Memory,
                        },
                    );
                }
                if output_bytes > self.bounds.max_result_bytes {
                    return QueryOutcome::Failed(
                        DomainFailure::ResourceExceeded {
                            kind: ResourceExceededKind::OutputBytes,
                        },
                    );
                }
                QueryOutcome::Ok {
                    package_id: result.package_id,
                    query: result.query,
                    node_count: result.node_count,
                    source_bytes: result.source_bytes,
                }
            }
            Ok(Err(reason)) => {
                // Guest rejection reasons are guest-controlled output
                // and cannot bypass the semantic result bound.
                if reason.len() > self.bounds.max_result_bytes {
                    return QueryOutcome::Failed(
                        DomainFailure::ResourceExceeded {
                            kind: ResourceExceededKind::OutputBytes,
                        },
                    );
                }
                QueryOutcome::Rejected { reason }
            }
            Err(error) => {
                let failure =
                    classify_trap(&error, self.bounds.max_result_bytes);
                self.stop_session();
                QueryOutcome::Failed(failure)
            }
        }
    }

    /// One mediated effect request against the active session. The
    /// domain forwards the request; the host validates it against the
    /// grant and returns the typed answer. A guest fault stops the
    /// session with the typed failure.
    pub fn request_effect(
        &mut self,
        request: EffectRequest,
    ) -> Result<MediatedAnswer, DomainFailure> {
        if self.session.is_none() {
            return Err(DomainFailure::NotActive);
        }
        if self.session.as_ref().is_some_and(|session| session.cancelled) {
            return Err(DomainFailure::Cancelled);
        }
        // Validate the untrusted strings before converting them into the
        // export-side WIT type. The generated ABI owns the incoming
        // allocation, but it must not make an arbitrary guest request
        // eligible for another host-side clone or mediator call.
        validate_effect_request(&request)?;
        // Normalize before both the guest export and the host-side
        // mediation record. The export cap is part of the request identity;
        // retaining the caller's unclamped value would make a valid capped
        // request fail the post-call equality check.
        let request = match request {
            EffectRequest::WorkspaceRead((path, max_bytes)) => {
                let bounded_max_bytes = u64::from(max_bytes)
                    .min(self.bounds.effects.max_workspace_read_bytes)
                    .min(u64::from(u32::MAX))
                    as u32;
                EffectRequest::WorkspaceRead((path, bounded_max_bytes))
            }
            request @ EffectRequest::ProcessExec(_) => request,
        };
        let export_request = match &request {
            EffectRequest::WorkspaceRead((path, max_bytes)) => {
                exports::siralos::domain_abi::domain_api::EffectRequest::WorkspaceRead((
                    path.clone(),
                    *max_bytes,
                ))
            }
            EffectRequest::ProcessExec(command) => {
                exports::siralos::domain_abi::domain_api::EffectRequest::ProcessExec(
                    command.clone(),
                )
            }
        };
        let call_result = {
            let session =
                self.session.as_mut().expect("session checked above");
            if let Err(_error) =
                session.store.set_fuel(self.bounds.fuel_per_call)
            {
                return Err(DomainFailure::Unavailable {
                    reason: "fuel unavailable".to_owned(),
                });
            }
            session.store.data_mut().last_mediation = None;
            session.store.data_mut().last_mediation_request = None;
            session
                .instance
                .interface0
                .call_request_effect(&mut session.store, &export_request)
        };
        let guest_answer = match call_result {
            Ok(answer) => answer,
            Err(error) => {
                let failure =
                    classify_trap(&error, self.bounds.max_result_bytes);
                self.stop_session();
                return Err(failure);
            }
        };
        if guest_answer_text_len(&guest_answer).is_some_and(|bytes| {
            bytes > self.bounds.max_result_bytes
                || bytes > self.bounds.effects.max_answer_bytes
        }) {
            self.stop_session();
            return Err(DomainFailure::ResourceExceeded {
                kind: ResourceExceededKind::OutputBytes,
            });
        }
        // The guest may only observe the Host import's disposition. Require
        // that the import ran for this exact validated request and use the
        // Host-recorded outcome; a forged guest `HostAnswer` is not accepted.
        let mediation = self
            .session
            .as_ref()
            .and_then(|session| session.store.data().last_mediation.clone());
        let observed_request = self.session.as_ref().and_then(|session| {
            session.store.data().last_mediation_request.as_ref()
        });
        let Some(observed_request) = observed_request else {
            self.stop_session();
            return Err(DomainFailure::InvalidOutput {
                reason: "effect answer was not host-mediated".to_owned(),
            });
        };
        if !effect_requests_equal(&request, observed_request) {
            self.stop_session();
            return Err(DomainFailure::InvalidOutput {
                reason: "effect mediation did not match the request"
                    .to_owned(),
            });
        }
        let Some(mediation) = mediation else {
            self.stop_session();
            return Err(DomainFailure::InvalidOutput {
                reason: "effect answer was not host-mediated".to_owned(),
            });
        };
        let answer = match mediation {
            EffectMediation::ResourceExceeded(kind) => {
                return Err(DomainFailure::ResourceExceeded { kind });
            }
            EffectMediation::Answer(answer) => answer,
        };
        if answer_text_len(&answer)
            .is_some_and(|bytes| bytes > self.bounds.effects.max_answer_bytes)
        {
            return Err(DomainFailure::ResourceExceeded {
                kind: ResourceExceededKind::OutputBytes,
            });
        }
        Ok(answer)
    }

    fn engine(&self) -> Result<Engine, DomainFailure> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_component_model(true);
        Engine::new(&config).map_err(|_error| DomainFailure::Unavailable {
            reason: "cannot create engine".to_owned(),
        })
    }

    fn store(
        &self,
        grant: CapabilityGrant,
        engine: &Engine,
    ) -> Result<Store<HostState>, DomainFailure> {
        let mediator = EffectMediator::new(
            grant,
            self.workspace_root.clone(),
            self.bounds.effects,
        );
        let limits = StoreLimitsBuilder::new()
            .memory_size(
                usize::try_from(self.bounds.max_memory_bytes)
                    .unwrap_or(usize::MAX),
            )
            .build();
        let mut store = Store::new(
            engine,
            HostState {
                mediator,
                limits,
                wasi: WasiCtxBuilder::new().build(),
                table: ResourceTable::new(),
                last_mediation: None,
                last_mediation_request: None,
            },
        );
        store.limiter(|state| &mut state.limits);
        Ok(store)
    }
}

/// The versioned export name the world requires: the WIT package
/// identity plus the exported interface name
/// (siralos:domain-abi/domain-api@1.0.0). The package version is
/// part of the name, so unknown or incompatible ABI versions fail
/// closed explicitly.
fn expected_domain_export(supported_abi: &DomainAbi) -> String {
    let (package, version) = supported_abi
        .as_str()
        .split_once("@")
        .expect("the supported ABI is validated before use");
    format!("{package}/domain-api@{version}")
}

/// Classify a runtime error into a typed domain failure. The trap
/// message lives in the error's cause chain, so the full debug chain
/// is classified (fuel/memory bounds before generic guest faults).
fn classify_trap(error: &wasmtime::Error, maximum: usize) -> DomainFailure {
    if let Some(trap) = error.downcast_ref::<wasmtime::Trap>() {
        match trap {
            wasmtime::Trap::OutOfFuel => {
                return DomainFailure::ResourceExceeded {
                    kind: ResourceExceededKind::Fuel,
                };
            }
            wasmtime::Trap::MemoryOutOfBounds
            | wasmtime::Trap::AllocationTooLarge => {
                return DomainFailure::ResourceExceeded {
                    kind: ResourceExceededKind::Memory,
                };
            }
            _ => {}
        }
    }
    // Host-created errors (and older Wasmtime wrappers) may not downcast to
    // `Trap`. Format only the root message into a bounded sink; never format
    // the full backtrace/context chain or retain an unbounded diagnostic.
    struct BoundedDisplay {
        text: String,
        limit: usize,
    }
    impl std::fmt::Write for BoundedDisplay {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            let remaining = self.limit.saturating_sub(self.text.len());
            if remaining == 0 {
                return Ok(());
            }
            let piece = if value.len() <= remaining {
                value
            } else {
                let mut end = remaining.min(value.len());
                while end > 0 && !value.is_char_boundary(end) {
                    end -= 1;
                }
                &value[..end]
            };
            self.text.push_str(piece);
            Ok(())
        }
    }
    let mut bounded = BoundedDisplay { text: String::new(), limit: maximum };
    let _ = std::fmt::Write::write_fmt(
        &mut bounded,
        format_args!("{}", error.root_cause()),
    );
    let root = bounded.text;
    if root.len() <= maximum {
        if root.contains("all fuel consumed") {
            return DomainFailure::ResourceExceeded {
                kind: ResourceExceededKind::Fuel,
            };
        }
        if root.contains("memory limit") {
            return DomainFailure::ResourceExceeded {
                kind: ResourceExceededKind::Memory,
            };
        }
    }
    DomainFailure::GuestFault { detail: "guest trap".to_owned() }
}
#[cfg(test)]
mod tests {
    use super::classify_trap;
    use siralos_core::domain::failure::{DomainFailure, ResourceExceededKind};

    #[test]
    fn memory_limit_traps_classify_as_memory_resource_exhaustion() {
        let error = wasmtime::Error::msg("wasm trap: memory limit exceeded");
        match classify_trap(&error, 4096) {
            DomainFailure::ResourceExceeded { kind } => {
                assert_eq!(kind, ResourceExceededKind::Memory);
            }
            other => panic!("unexpected classification: {other:?}"),
        }
    }

    #[test]
    fn fuel_traps_classify_as_fuel_resource_exhaustion() {
        let error = wasmtime::Error::msg(
            "wasm trap: all fuel consumed by WebAssembly",
        );
        match classify_trap(&error, 4096) {
            DomainFailure::ResourceExceeded { kind } => {
                assert_eq!(kind, ResourceExceededKind::Fuel);
            }
            other => panic!("unexpected classification: {other:?}"),
        }
    }
}
