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
};

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

use std::path::PathBuf;

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
        let outcome = self.mediator.mediate(&request);
        self.last_mediation = Some(outcome.clone());
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

impl DomainHost {
    /// A host for one domain slot over the given component file.
    pub fn new(
        supported_abi: DomainAbi,
        authority: HostAuthority,
        component_path: PathBuf,
        workspace_root: PathBuf,
        bounds: DomainHostBounds,
    ) -> Self {
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
    fn component_bytes(&self) -> Result<Vec<u8>, DomainFailure> {
        let metadata = std::fs::symlink_metadata(&self.component_path)
            .map_err(|error| DomainFailure::Unavailable {
                reason: format!("cannot inspect component: {error}"),
            })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(DomainFailure::Unavailable {
                reason: "component must be a regular file".to_owned(),
            });
        }
        if metadata.len() > self.bounds.max_component_bytes as u64 {
            return Err(DomainFailure::InvalidInput {
                reason: "component exceeds the byte bound".to_owned(),
            });
        }
        let bytes = std::fs::read(&self.component_path).map_err(|error| {
            DomainFailure::Unavailable {
                reason: format!("cannot read component: {error}"),
            }
        })?;
        if bytes.len() > self.bounds.max_component_bytes {
            return Err(DomainFailure::InvalidInput {
                reason: "component exceeds the byte bound".to_owned(),
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
    /// Order: exact bytes are re-verified first, then the component is
    /// loaded and instantiated (malformed or version-incompatible
    /// bytes fail before any lifecycle mutation), then the lifecycle
    /// decision runs, then the exact identity is bound into the guest.
    /// The final commit revalidates the prepared activation against
    /// the current lifecycle episode and fails typed if anything
    /// changed after preparation, publishing no HostSession and
    /// leaving the lifecycle unchanged.
    pub fn activate(
        &mut self,
        request: ActivationRequest,
        runtime: RuntimeCheckResult,
    ) -> Result<ActiveDomain, DomainFailure> {
        if self.session.is_some() {
            return Err(DomainFailure::Active);
        }
        // 1. Exact bytes: the host recomputes the digest itself.
        let bytes = self.component_bytes()?;
        let computed = PackageDigest::parse(&sha256_hex(&bytes))?;
        verify_package_digest(request.digest(), &computed)?;
        // 2. Load: malformed bytes fail as invalid input.
        let engine = self.engine()?;
        let component =
            Component::from_binary(&engine, &bytes).map_err(|error| {
                DomainFailure::InvalidInput {
                    reason: format!("malformed component: {error}"),
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
        // 3. Lifecycle preparation: every typed gate (identity,
        //    protocol, package-declaration capability ceiling, Host
        //    policy, runtime check) runs now WITHOUT committing any
        //    authoritative state. The prepared grant drives the
        //    mediator.
        let prepared = self.lifecycle.prepare_activation(
            &request,
            &self.supported_abi,
            &self.authority,
            &runtime,
        )?;
        // 4. Instantiate: version/world-incompatible components fail
        //    explicitly; the component imports exactly host-effects,
        //    so any other import also fails here. The store is created
        //    with the prepared effective grant.
        // The provisional store's mediator is configured with the
        // NON-AUTHORITATIVE provisional grant (computed from the
        // prepare-time authority). The authoritative ActiveDomain
        // grant is recomputed by the final commit from the
        // commit-time Host authority; the provisional grant is
        // discarded with the store if that commit fails. Within this
        // single-threaded activate() call the host authority is
        // immutable (there is no authority mutator), and the
        // conformance guest never invokes host effects from bind, so
        // no guest code can exercise the provisional grant before the
        // final commit.
        let mut store =
            self.store(prepared.provisional_grant().clone(), &engine)?;
        // Instantiation executes the component's canonical-ABI
        // initialization, which also consumes fuel; grant the call
        // budget for it.
        store.set_fuel(self.bounds.fuel_per_call).map_err(|error| {
            DomainFailure::Unavailable {
                reason: format!("fuel unavailable: {error}"),
            }
        })?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(
            |error| DomainFailure::Unavailable {
                reason: format!("cannot link wasi plumbing: {error}"),
            },
        )?;
        SiralosDomain::add_to_linker::<
            HostState,
            wasmtime::component::HasSelf<HostState>,
        >(&mut linker, |state: &mut HostState| state)
        .map_err(|error| DomainFailure::Unavailable {
            reason: format!("cannot link host effects: {error}"),
        })?;
        let instance =
            SiralosDomain::instantiate(&mut store, &component, &linker)
                .map_err(|error| {
                    eprintln!("domain-host: instantiation failed: {error}");
                    DomainFailure::UnsupportedAbi {
                        expected: self.supported_abi.as_str().to_owned(),
                        found: request.abi().as_str().to_owned(),
                    }
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
            return Err(DomainFailure::InvalidOutput {
                reason: format!("guest rejected the identity: {reason}"),
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
            if let Err(error) =
                session.store.set_fuel(self.bounds.fuel_per_call)
            {
                return QueryOutcome::Failed(DomainFailure::Unavailable {
                    reason: format!("fuel unavailable: {error}"),
                });
            }
            session.store.data_mut().last_mediation = None;
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
            if let Err(error) =
                session.store.set_fuel(self.bounds.fuel_per_call)
            {
                return Err(DomainFailure::Unavailable {
                    reason: format!("fuel unavailable: {error}"),
                });
            }
            session.store.data_mut().last_mediation = None;
            session
                .instance
                .interface0
                .call_request_effect(&mut session.store, &export_request)
        };
        let answer = match call_result {
            Ok(answer) => answer,
            Err(error) => {
                let failure =
                    classify_trap(&error, self.bounds.max_result_bytes);
                self.stop_session();
                return Err(failure);
            }
        };
        // The Host retains the typed mediation outcome (for example
        // HostCalls exhaustion) even though the guest protocol only
        // carried a bounded disposition.
        let mediation = self
            .session
            .as_ref()
            .and_then(|session| session.store.data().last_mediation.clone());
        if let Some(EffectMediation::ResourceExceeded(kind)) = mediation {
            return Err(DomainFailure::ResourceExceeded { kind });
        }
        // Guest-produced answer payloads are guest-controlled output
        // and cannot bypass the effect answer bound.
        let payload = match &answer {
            exports::siralos::domain_abi::domain_api::HostAnswer::Ok(text)
            | exports::siralos::domain_abi::domain_api::HostAnswer::Denied(
                text,
            )
            | exports::siralos::domain_abi::domain_api::HostAnswer::Error(
                text,
            ) => Some(text.len()),
            exports::siralos::domain_abi::domain_api::HostAnswer::Cancelled => {
                None
            }
        };
        if payload
            .is_some_and(|bytes| bytes > self.bounds.effects.max_answer_bytes)
        {
            return Err(DomainFailure::ResourceExceeded {
                kind: ResourceExceededKind::OutputBytes,
            });
        }
        Ok(match answer {
            exports::siralos::domain_abi::domain_api::HostAnswer::Ok(text) => {
                MediatedAnswer::Ok(text)
            }
            exports::siralos::domain_abi::domain_api::HostAnswer::Denied(
                reason,
            ) => MediatedAnswer::Denied(reason),
            exports::siralos::domain_abi::domain_api::HostAnswer::Cancelled => {
                MediatedAnswer::Cancelled
            }
            exports::siralos::domain_abi::domain_api::HostAnswer::Error(
                reason,
            ) => MediatedAnswer::Error(reason),
        })
    }

    fn engine(&self) -> Result<Engine, DomainFailure> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.wasm_component_model(true);
        Engine::new(&config).map_err(|error| DomainFailure::Unavailable {
            reason: format!("cannot create engine: {error}"),
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

/// Bound a guest reason string.
fn bound_reason(reason: &str, maximum: usize) -> String {
    let mut reason = reason.to_owned();
    if reason.len() > maximum {
        reason.truncate(maximum);
    }
    reason
}

/// Classify a runtime error into a typed domain failure. The trap
/// message lives in the error's cause chain, so the full debug chain
/// is classified (fuel/memory bounds before generic guest faults).
fn classify_trap(error: &wasmtime::Error, maximum: usize) -> DomainFailure {
    let chain = format!("{error:?}");
    if chain.contains("all fuel consumed") {
        return DomainFailure::ResourceExceeded {
            kind: ResourceExceededKind::Fuel,
        };
    }
    if chain.contains("memory limit") {
        return DomainFailure::ResourceExceeded {
            kind: ResourceExceededKind::Memory,
        };
    }
    DomainFailure::GuestFault { detail: bound_reason(&chain, maximum) }
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
