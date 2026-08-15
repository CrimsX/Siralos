//! Component Model / WIT conformance tests (Stage 3R R6, ADR 0034).
//!
//! These tests execute the REAL production boundary: the versioned WIT
//! world (`siralos-adapters/wit/domain-abi.wit`), the exact checked-in
//! synthetic conformance component bytes, and the production host
//! (`siralos-adapters::domain`). No path here is mocked: the digest is
//! computed from the accepted component bytes, activation binds the
//! exact identity, effects are host-mediated against the grant, traps
//! stay contained, and fuel bounds pathological guests.
//!
//! Fixture digests are asserted against the checked-in component bytes;
//! when a fixture is rebuilt (tests/domain-conformance/README.md), the
//! digest constants below must be updated in the same change.

use siralos_adapters::domain::{
    DomainHost, DomainHostBounds, EffectRequest, MediatedAnswer, QueryOutcome,
};
use siralos_core::domain::capability::HostAuthority;
use siralos_core::domain::failure::{DomainFailure, ResourceExceededKind};
use siralos_core::domain::lifecycle::{ActivationRequest, RuntimeCheckResult};
use siralos_core::domain::package::{DomainAbi, DomainPackage};
use siralos_core::identity::sha256_hex;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const ABI: &str = "siralos:domain-abi@1.0.0";

/// Exact SHA-256 of `fixtures/conformance-domain.component.wasm`.
const CONFORMANCE_DIGEST: &str =
    "e8fc793632a9a1050364e1c5eed0eb16b55c645ccd589f472cf06a80039ac8f8";

/// Exact SHA-256 of `fixtures/incompatible-domain.component.wasm`.
const INCOMPATIBLE_DIGEST: &str =
    "0001c6c4fea47187120e70f1c4f32f2ff4f44d3d876c8f0c71d2e7be7c9b747b";

fn fixtures() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join("tests/domain-conformance/fixtures")
}

fn temp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("siralos-r6-{label}-{unique}"));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir creation");
    path
}

fn digest_hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn authority(capabilities: &[&str]) -> HostAuthority {
    HostAuthority::parse(
        &capabilities
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("authority parses")
}

fn request(id: &str, digest: &str) -> ActivationRequest {
    ActivationRequest::parse(id, digest, ABI, &["workspace-read".to_owned()])
        .expect("request parses")
}

fn package(id: &str, digest: &str, abi: &str) -> DomainPackage {
    DomainPackage::parse(id, digest, abi, &["workspace-read".to_owned()])
        .expect("package parses")
}

fn make_host(
    component: &Path,
    root: &Path,
    bounds: DomainHostBounds,
) -> DomainHost {
    DomainHost::new(
        DomainAbi::parse(ABI).expect("abi parses"),
        authority(&["workspace-read"]),
        component.to_path_buf(),
        root.to_path_buf(),
        bounds,
    )
}

fn default_bounds() -> DomainHostBounds {
    DomainHostBounds::default()
}

/// Install + enable + activate a fixture host end to end.
fn activate_fixture(
    host: &mut DomainHost,
    digest: &str,
) -> Result<(), DomainFailure> {
    host.install(package("conformance-domain", digest, ABI))?;
    host.enable()?;
    host.activate(
        request("conformance-domain", digest),
        RuntimeCheckResult::Ready,
    )
    .map(|_| ())
}

#[test]
fn fixture_bytes_produce_the_bound_digests() {
    let conformance = fixtures().join("conformance-domain.component.wasm");
    let incompatible = fixtures().join("incompatible-domain.component.wasm");
    assert_eq!(
        sha256_hex(&fs::read(&conformance).expect("fixture bytes")),
        CONFORMANCE_DIGEST
    );
    assert_eq!(
        sha256_hex(&fs::read(&incompatible).expect("fixture bytes")),
        INCOMPATIBLE_DIGEST
    );
}

#[test]
fn component_loads_and_binds_only_when_abi_compatible() {
    let root = temp_dir("load");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    // The component imports exactly the world's two interfaces plus
    // the minimal wasm32-wasip2 std plumbing (cli/io/clocks). No
    // filesystem, network, process, or ambient WASI surface exists.
    let engine = wasmtime::Engine::default();
    let bytes = fs::read(fixtures().join("conformance-domain.component.wasm"))
        .unwrap();
    let component =
        wasmtime::component::Component::from_binary(&engine, &bytes).unwrap();
    let imports: Vec<String> = component
        .component_type()
        .imports(&engine)
        .map(|(name, _)| name.to_string())
        .collect();
    for name in &imports {
        assert!(
            name.starts_with("siralos:domain-abi/")
                || name.starts_with("wasi:io/")
                || name.starts_with("wasi:clocks/")
                || name.starts_with("wasi:cli/"),
            "unexpected component import: {name}"
        );
        assert!(
            !name.starts_with("wasi:filesystem")
                && !name.starts_with("wasi:http")
                && !name.starts_with("wasi:sockets")
                && !name.starts_with("wasi:cli/run")
                && !name.starts_with("wasi:process"),
            "ambient capability import: {name}"
        );
    }
}

#[test]
fn wrong_declared_digest_is_rejected_at_install() {
    let root = temp_dir("digest");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    // The host computes the digest from the bytes; a lying descriptor
    // is rejected before any lifecycle change.
    let failure = host
        .install(package("conformance-domain", &digest_hex(7), ABI))
        .expect_err("digest mismatch rejects install");
    assert_eq!(failure.code(), "IDENTITY_MISMATCH");
    assert_eq!(host.state().as_str(), "absent");
}

#[test]
fn stale_bytes_fail_activation_before_semantic_work() {
    let root = temp_dir("stale");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    host.install(package("conformance-domain", CONFORMANCE_DIGEST, ABI))
        .expect("install");
    host.enable().expect("enable");
    // The bytes under the slot change after install (the slot file is
    // replaced with different component bytes); activation must
    // recompute the digest and refuse before any semantic work.
    let slot = root.join("slot.component.wasm");
    fs::copy(fixtures().join("conformance-domain.component.wasm"), &slot)
        .expect("copy slot bytes");
    let mut slot_host = make_host(&slot, &root, default_bounds());
    slot_host
        .install(package("conformance-domain", CONFORMANCE_DIGEST, ABI))
        .expect("install");
    slot_host.enable().expect("enable");
    fs::copy(fixtures().join("incompatible-domain.component.wasm"), &slot)
        .expect("replace slot bytes");
    let failure = slot_host
        .activate(
            request("conformance-domain", CONFORMANCE_DIGEST),
            RuntimeCheckResult::Ready,
        )
        .expect_err("stale bytes reject activation");
    assert_eq!(failure.code(), "IDENTITY_MISMATCH");
    // No session was created and the lifecycle stays enabled.
    assert_eq!(slot_host.state().as_str(), "enabled");
}

#[test]
fn semantic_query_succeeds_when_correctly_bound() {
    let root = temp_dir("query");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    let failure = host
        .activate(
            ActivationRequest::parse(
                "conformance-domain",
                CONFORMANCE_DIGEST,
                ABI,
                &["workspace-read".to_owned()],
            )
            .expect("request parses"),
            RuntimeCheckResult::Ready,
        )
        .expect_err("not installed yet");
    assert_eq!(failure.code(), "NOT_INSTALLED");
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    match host.query("alpha beta gamma") {
        QueryOutcome::Ok { package_id, query, node_count, source_bytes } => {
            assert_eq!(package_id, "conformance-domain");
            assert_eq!(query, "alpha beta gamma");
            assert_eq!(node_count, 3);
            assert_eq!(source_bytes, 16);
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

#[test]
fn mediated_permitted_capability_works_and_denied_stays_denied() {
    let root = temp_dir("effects");
    fs::write(root.join("notes.txt"), "fixture line one\nfixture line two\n")
        .expect("fixture write");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    // Permitted mediated read: the host validates the request, checks
    // the grant, applies the bounds, and returns only the result.
    match host.request_effect(EffectRequest::WorkspaceRead((
        "notes.txt".to_owned(),
        4096,
    ))) {
        Ok(MediatedAnswer::Ok(content)) => {
            assert_eq!(content, "fixture line one\nfixture line two");
        }
        other => panic!("unexpected answer: {other:?}"),
    }
    // Denied capability stays denied (typed, never escalating).
    match host.request_effect(EffectRequest::ProcessExec("whoami".to_owned()))
    {
        Ok(MediatedAnswer::Denied(reason)) => {
            assert!(reason.contains("process-exec"));
        }
        other => panic!("unexpected answer: {other:?}"),
    }
    // The guest-initiated denial path also reports the denial.
    match host.query("denied") {
        QueryOutcome::Rejected { reason } => {
            assert!(reason.contains("denied"));
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

#[test]
fn guest_trap_is_contained_and_host_state_stays_intact() {
    let root = temp_dir("trap");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    match host.query("trap") {
        QueryOutcome::Failed(failure) => {
            assert_eq!(failure.code(), "GUEST_FAULT");
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
    // Containment: the fault is typed, the activation is stopped, and
    // the host state survives — the lifecycle is back to enabled and a
    // fresh explicit activation serves calls again.
    assert_eq!(host.state().as_str(), "enabled");
    assert!(host.active().is_none());
    host.activate(
        request("conformance-domain", CONFORMANCE_DIGEST),
        RuntimeCheckResult::Ready,
    )
    .expect("reactivation after the fault");
    match host.query("still alive") {
        QueryOutcome::Ok { node_count, .. } => assert_eq!(node_count, 2),
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

#[test]
fn pathological_guest_is_fuel_bounded_and_cannot_hang_the_make_host() {
    let root = temp_dir("fuel");
    let mut bounds = default_bounds();
    bounds.fuel_per_call = 20_000;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    let started = Instant::now();
    match host.query("loop") {
        QueryOutcome::Failed(failure) => {
            assert_eq!(
                failure.code(),
                "RESOURCE_EXCEEDED",
                "unexpected failure: {failure:?}"
            );
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "pathological guest must terminate quickly"
    );
}

#[test]
fn malformed_component_fails_with_a_typed_error() {
    let root = temp_dir("malformed");
    let malformed = root.join("malformed.component.bin");
    fs::write(&malformed, b"this is not a wasm component at all")
        .expect("garbage write");
    let garbage_digest = sha256_hex(&fs::read(&malformed).unwrap());
    let mut host = make_host(&malformed, &root, default_bounds());
    host.install(package("conformance-domain", &garbage_digest, ABI))
        .expect("install accepts exact bytes");
    host.enable().expect("enable");
    let failure = host
        .activate(
            request("conformance-domain", &garbage_digest),
            RuntimeCheckResult::Ready,
        )
        .expect_err("malformed component fails activation");
    assert_eq!(failure.code(), "INVALID_INPUT");
}

#[test]
fn version_incompatible_component_fails_instantiation_explicitly() {
    let root = temp_dir("abi");
    let mut host = make_host(
        &fixtures().join("incompatible-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    // The descriptor claims the supported ABI, but the bytes were
    // built against siralos:domain-abi@1.1.0: instantiation fails
    // explicitly and nothing runs.
    host.install(package("conformance-domain", INCOMPATIBLE_DIGEST, ABI))
        .expect("install");
    host.enable().expect("enable");
    let failure = host
        .activate(
            request("conformance-domain", INCOMPATIBLE_DIGEST),
            RuntimeCheckResult::Ready,
        )
        .expect_err("incompatible component fails activation");
    assert_eq!(failure.code(), "UNSUPPORTED_ABI");
    // The lifecycle remains enabled: a correct package can still be
    // installed after an explicit transition.
    assert_eq!(host.state().as_str(), "enabled");
}

#[test]
fn input_bounds_are_enforced_before_the_call() {
    let root = temp_dir("bounds");
    let mut bounds = default_bounds();
    bounds.max_query_bytes = 16;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    match host.query("this input is far too long for the bound") {
        QueryOutcome::Refused(failure) => {
            assert_eq!(failure.code(), "INVALID_INPUT");
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
    // No active session refuses calls with the typed outcome.
    let mut fresh = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    match fresh.query("hello") {
        QueryOutcome::Refused(failure) => {
            assert_eq!(failure.code(), "NOT_ACTIVE");
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

#[test]
fn cancellation_refuses_further_calls_with_a_typed_outcome() {
    let root = temp_dir("cancel");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    host.cancel();
    match host.query("hello") {
        QueryOutcome::Refused(failure) => {
            assert_eq!(failure.code(), "CANCELLED");
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
    match host.request_effect(EffectRequest::WorkspaceRead((
        "notes.txt".to_owned(),
        64,
    ))) {
        Err(failure) => assert_eq!(failure.code(), "CANCELLED"),
        other => panic!("unexpected answer: {other:?}"),
    }
    // Deactivation still ends the session cleanly.
    host.deactivate().expect("deactivate");
    assert_eq!(host.state().as_str(), "enabled");
}

#[test]
fn host_call_budget_is_enforced() {
    let root = temp_dir("calls");
    let mut bounds = default_bounds();
    bounds.effects.max_host_calls = 2;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    for _ in 0..2 {
        let _ =
            host.request_effect(EffectRequest::ProcessExec("x".to_owned()));
    }
    let failure = host
        .request_effect(EffectRequest::ProcessExec("x".to_owned()))
        .expect_err("budget exhaustion is a typed failure");
    assert!(matches!(
        failure,
        DomainFailure::ResourceExceeded {
            kind: ResourceExceededKind::HostCalls,
        }
    ));
}

/// Request with an explicit capability set.
fn request_with_caps(
    id: &str,
    digest: &str,
    capabilities: &[&str],
) -> ActivationRequest {
    ActivationRequest::parse(
        id,
        digest,
        ABI,
        &capabilities.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
    )
    .expect("request parses")
}

/// Package with an explicit declared capability set.
fn package_with_caps(
    id: &str,
    digest: &str,
    capabilities: &[&str],
) -> DomainPackage {
    DomainPackage::parse(
        id,
        digest,
        ABI,
        &capabilities.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>(),
    )
    .expect("package parses")
}

#[test]
fn bind_rejection_leaves_enabled_and_unpublished() {
    let root = temp_dir("bind-reject");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    host.install(package_with_caps(
        "reject-bind",
        CONFORMANCE_DIGEST,
        &["workspace-read"],
    ))
    .expect("install");
    host.enable().expect("enable");
    let failure = host
        .activate(
            request_with_caps(
                "reject-bind",
                CONFORMANCE_DIGEST,
                &["workspace-read"],
            ),
            RuntimeCheckResult::Ready,
        )
        .expect_err("bind rejection fails activation");
    assert_eq!(failure.code(), "INVALID_OUTPUT");
    assert_eq!(host.state().as_str(), "enabled");
    assert!(host.active().is_none());
    // A normal activation succeeds after the failed provisional attempt.
    host.uninstall().expect("uninstall");
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("reactivation");
    match host.query("still works") {
        QueryOutcome::Ok { .. } => {}
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

#[test]
fn bind_trap_leaves_enabled_and_unpublished() {
    let root = temp_dir("bind-trap");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    host.install(package_with_caps(
        "trap-bind",
        CONFORMANCE_DIGEST,
        &["workspace-read"],
    ))
    .expect("install");
    host.enable().expect("enable");
    let failure = host
        .activate(
            request_with_caps(
                "trap-bind",
                CONFORMANCE_DIGEST,
                &["workspace-read"],
            ),
            RuntimeCheckResult::Ready,
        )
        .expect_err("bind trap fails activation");
    assert_eq!(failure.code(), "GUEST_FAULT");
    assert_eq!(host.state().as_str(), "enabled");
    assert!(host.active().is_none());
    host.uninstall().expect("uninstall");
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("reactivation");
}

#[test]
fn bind_fuel_exhaustion_leaves_enabled_and_unpublished() {
    let root = temp_dir("bind-fuel");
    let mut bounds = default_bounds();
    bounds.fuel_per_call = 20_000;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    host.install(package_with_caps(
        "loop-bind",
        CONFORMANCE_DIGEST,
        &["workspace-read"],
    ))
    .expect("install");
    host.enable().expect("enable");
    let failure = host
        .activate(
            request_with_caps(
                "loop-bind",
                CONFORMANCE_DIGEST,
                &["workspace-read"],
            ),
            RuntimeCheckResult::Ready,
        )
        .expect_err("bind fuel exhaustion fails activation");
    assert_eq!(failure.code(), "RESOURCE_EXCEEDED");
    assert_eq!(host.state().as_str(), "enabled");
    assert!(host.active().is_none());
    host.uninstall().expect("uninstall");
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("reactivation");
}

#[test]
fn active_to_active_is_rejected_and_sessions_are_monotonic() {
    let root = temp_dir("active-active");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    let failure = host
        .activate(
            request("conformance-domain", CONFORMANCE_DIGEST),
            RuntimeCheckResult::Ready,
        )
        .expect_err("active-to-active rejected");
    assert_eq!(failure.code(), "ACTIVE");
    let preserved = host.active().expect("session preserved");
    assert_eq!(preserved.session_id(), 1);
    host.deactivate().expect("deactivate");
    let next = host
        .activate(
            request("conformance-domain", CONFORMANCE_DIGEST),
            RuntimeCheckResult::Ready,
        )
        .expect("reactivation");
    assert_eq!(next.session_id(), 2);
}

#[test]
fn package_declaration_bounds_activation_through_the_host() {
    let root = temp_dir("declaration");
    let mut host = make_host_with_authority(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
        &["workspace-read", "process-exec"],
    );
    // The installed package declares only workspace-read.
    host.install(package_with_caps(
        "conformance-domain",
        CONFORMANCE_DIGEST,
        &["workspace-read"],
    ))
    .expect("install");
    host.enable().expect("enable");
    // Host authority contains both capabilities, so the only
    // failing layer is the package declaration.
    let failure = host
        .activate(
            request_with_caps(
                "conformance-domain",
                CONFORMANCE_DIGEST,
                &["workspace-read", "process-exec"],
            ),
            RuntimeCheckResult::Ready,
        )
        .expect_err("undeclared capability fails activation");
    assert_eq!(failure.code(), "UNDECLARED_CAPABILITY");
    assert_eq!(host.state().as_str(), "enabled");
    assert!(host.active().is_none());
    // The declared subset still activates.
    let active = host
        .activate(
            request("conformance-domain", CONFORMANCE_DIGEST),
            RuntimeCheckResult::Ready,
        )
        .expect("declared subset activates");
    let granted: Vec<&str> =
        active.grant().iter().map(|id| id.as_str()).collect();
    assert_eq!(granted, vec!["workspace-read"]);
}

#[test]
fn semantic_output_aggregate_bound_is_enforced() {
    let root = temp_dir("output-bound");
    let mut bounds = default_bounds();
    // package_id ("conformance-domain", 18 bytes) + query payload
    // must fit the aggregate semantic result bound.
    bounds.max_result_bytes = 82;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    // Exact boundary (18 + 64 = 82) is accepted.
    match host.query("pad:64") {
        QueryOutcome::Ok { node_count, .. } => assert_eq!(node_count, 64),
        other => panic!("unexpected query outcome: {other:?}"),
    }
    // One byte over the aggregate bound is a typed output failure.
    match host.query("pad:65") {
        QueryOutcome::Failed(failure) => {
            assert_eq!(failure.code(), "RESOURCE_EXCEEDED");
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
    // An oversized guest rejection reason cannot bypass the bound.
    match host.query("reject:200") {
        QueryOutcome::Failed(failure) => {
            assert_eq!(failure.code(), "RESOURCE_EXCEEDED");
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
    // The host remains internally consistent afterwards.
    match host.query("pad:1") {
        QueryOutcome::Ok { node_count, .. } => assert_eq!(node_count, 1),
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

#[test]
fn effect_answer_payload_cannot_bypass_the_host_visible_bound() {
    let root = temp_dir("effect-bound");
    let mut bounds = default_bounds();
    bounds.effects.max_answer_bytes = 64;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    let failure = host
        .request_effect(EffectRequest::ProcessExec("big".to_owned()))
        .expect_err("oversized guest-forged answer is rejected");
    assert_eq!(failure.code(), "RESOURCE_EXCEEDED");
    // Normal effect answers still work.
    match host.request_effect(EffectRequest::ProcessExec("x".to_owned())) {
        Ok(MediatedAnswer::Denied(_)) => {}
        other => panic!("unexpected answer: {other:?}"),
    }
}

#[test]
fn host_call_budget_is_a_typed_host_observed_failure() {
    let root = temp_dir("host-calls");
    let mut bounds = default_bounds();
    bounds.effects.max_host_calls = 5;
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        bounds,
    );
    activate_fixture(&mut host, CONFORMANCE_DIGEST).expect("activation");
    for _ in 0..5 {
        let _ =
            host.request_effect(EffectRequest::ProcessExec("x".to_owned()));
    }
    let failure = host
        .request_effect(EffectRequest::ProcessExec("x".to_owned()))
        .expect_err("budget exhaustion is a typed failure");
    assert!(matches!(
        failure,
        DomainFailure::ResourceExceeded {
            kind: ResourceExceededKind::HostCalls,
        }
    ));
    // The guest-initiated path records the same typed classification.
    match host.query("calls:100") {
        QueryOutcome::Failed(failure) => {
            assert!(matches!(
                failure,
                DomainFailure::ResourceExceeded {
                    kind: ResourceExceededKind::HostCalls,
                }
            ));
        }
        other => panic!("unexpected query outcome: {other:?}"),
    }
}

/// Host over a fixture with an explicit Host authority set.
fn make_host_with_authority(
    component: &Path,
    root: &Path,
    bounds: DomainHostBounds,
    capabilities: &[&str],
) -> DomainHost {
    DomainHost::new(
        DomainAbi::parse(ABI).expect("abi parses"),
        authority(capabilities),
        component.to_path_buf(),
        root.to_path_buf(),
        bounds,
    )
}
#[test]
fn activation_is_session_scoped_and_binds_exact_identity() {
    let root = temp_dir("session");
    let mut host = make_host(
        &fixtures().join("conformance-domain.component.wasm"),
        &root,
        default_bounds(),
    );
    let first = activate_fixture(&mut host, CONFORMANCE_DIGEST);
    assert!(first.is_ok());
    let active = host.active().expect("active session");
    assert_eq!(active.session_id(), 1);
    assert_eq!(active.binding().package_id().as_str(), "conformance-domain");
    assert_eq!(active.binding().digest().as_str(), CONFORMANCE_DIGEST);
    assert_eq!(active.binding().abi().as_str(), ABI);
    // A second activation is refused while active.
    let second = host.activate(
        request("conformance-domain", CONFORMANCE_DIGEST),
        RuntimeCheckResult::Ready,
    );
    assert_eq!(second.expect_err("active").code(), "ACTIVE");
    host.deactivate().expect("deactivate");
    let third = host.activate(
        request("conformance-domain", CONFORMANCE_DIGEST),
        RuntimeCheckResult::Ready,
    );
    assert_eq!(third.expect("reactivation").session_id(), 2);
    // The package id must match the installed package exactly.
    host.deactivate().expect("deactivate");
    let wrong_id = host.activate(
        request("other-domain", CONFORMANCE_DIGEST),
        RuntimeCheckResult::Ready,
    );
    assert_eq!(wrong_id.expect_err("wrong id").code(), "IDENTITY_MISMATCH");
}
