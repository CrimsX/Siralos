//! Synthetic conformance domain guest (Stage 3R R6).
//!
//! A deliberately boring, product-neutral component whose only purpose
//! is to prove the production Host/Domain boundary (ADR 0034): it
//! performs deterministic local semantic work, requests host-mediated
//! effects, and includes deterministic pathological behaviors (trap,
//! unbounded loop) driven by fixed input markers so the host
//! conformance tests can prove trap containment and bounded execution.
//!
//! It contains no product domain semantics and no ambient authority:
//! the component imports exactly `host-effects` and nothing else.

wit_bindgen::generate!({
    path: "../../../crates/siralos-adapters/wit/domain-abi.wit",
    world: "siralos-domain",
});

use std::sync::Mutex;

use exports::siralos::domain_abi::domain_api::{
    EffectRequest, Guest, HostAnswer, PackageIdentity, SemanticResult,
};
use siralos::domain_abi::domain_api as imports;
use siralos::domain_abi::host_effects::perform;

static STATE: Mutex<Option<PackageIdentity>> = Mutex::new(None);

struct ConformanceDomain;

impl Guest for ConformanceDomain {
    fn bind(identity: PackageIdentity) -> Result<(), String> {
        if identity.package_id.is_empty()
            || identity.package_digest.is_empty()
            || identity.abi.is_empty()
        {
            return Err("invalid package identity".to_string());
        }
        // Deterministic bind failure markers (conformance only): the
        // host must treat bind as fallible untrusted execution.
        if identity.package_id == "reject-bind" {
            return Err("identity rejected".to_string());
        }
        if identity.package_id == "trap-bind" {
            panic!("synthetic bind trap");
        }
        if identity.package_id == "loop-bind" {
            loop {
                std::hint::spin_loop();
            }
        }
        // Provisional-effect markers (conformance only): prove that an
        // arbitrary component CAN invoke host-mediated effects during
        // bind, BEFORE the authoritative commit, and that the
        // provisional mediator enforces exactly the grant that the
        // final commit authorizes.
        if identity.package_id == "effect-bind" {
            // A permitted workspace read during bind: bind succeeds
            // only when the provisional mediator grants it and returns
            // the bounded content.
            let answer = perform(&imports::EffectRequest::WorkspaceRead((
                "notes.txt".to_string(),
                4096,
            )));
            return match answer {
                imports::HostAnswer::Ok(_) => {
                    *STATE.lock().expect("guest state lock") =
                        Some(identity);
                    Ok(())
                }
                imports::HostAnswer::Denied(reason) => {
                    Err(format!("denied:{reason}"))
                }
                imports::HostAnswer::Cancelled => {
                    Err("cancelled".to_string())
                }
                imports::HostAnswer::Error(reason) => {
                    Err(format!("error:{reason}"))
                }
            };
        }
        if identity.package_id == "exec-bind" {
            // An out-of-grant process-exec request during bind: the
            // provisional mediator must deny it, so bind rejects
            // deterministically and activation must fail with no
            // HostSession.
            let answer =
                perform(&imports::EffectRequest::ProcessExec("whoami".to_string()));
            return match answer {
                imports::HostAnswer::Denied(reason) => {
                    Err(format!("denied:{reason}"))
                }
                imports::HostAnswer::Ok(_) => {
                    Err("unexpected grant".to_string())
                }
                imports::HostAnswer::Cancelled => {
                    Err("cancelled".to_string())
                }
                imports::HostAnswer::Error(reason) => {
                    Err(format!("error:{reason}"))
                }
            };
        }
        *STATE.lock().expect("guest state lock") = Some(identity);
        Ok(())
    }

    fn query(text: String) -> Result<SemanticResult, String> {
        // Deterministic pathological markers (conformance only).
        if text == "trap" {
            panic!("synthetic guest trap");
        }
        if text == "loop" {
            loop {
                std::hint::spin_loop();
            }
        }
        if text == "bigmem" {
            // Deterministic memory-exhaustion marker (conformance
            // only): allocating beyond the host's store memory limit
            // must trap with the typed memory resource failure.
            let big = vec![0u8; 100 * 1024 * 1024];
            let package_id = STATE
                .lock()
                .expect("guest state lock")
                .as_ref()
                .map(|identity| identity.package_id.clone())
                .unwrap_or_else(|| "<unbound>".to_string());
            return Ok(SemanticResult {
                package_id,
                query: "bigmem".to_string(),
                node_count: big.len() as u32,
                source_bytes: 6,
            });
        }
        if let Some(count) = text.strip_prefix("pad:") {
            if let Ok(size) = count.parse::<usize>() {
                let package_id = STATE
                    .lock()
                    .expect("guest state lock")
                    .as_ref()
                    .map(|identity| identity.package_id.clone())
                    .unwrap_or_else(|| "<unbound>".to_string());
                let padded = "x".repeat(size);
                let source_bytes = padded.len() as u32;
                return Ok(SemanticResult {
                    package_id,
                    query: padded,
                    node_count: size as u32,
                    source_bytes,
                });
            }
        }
        if let Some(count) = text.strip_prefix("reject:") {
            if let Ok(size) = count.parse::<usize>() {
                return Err("y".repeat(size));
            }
        }
        if let Some(count) = text.strip_prefix("calls:") {
            if let Ok(times) = count.parse::<u32>() {
                // Exhaust the Host-call budget through the real
                // boundary: the Host observes the typed resource
                // failure independently of the guest-visible answers.
                for _ in 0..times {
                    let _ = perform(&imports::EffectRequest::ProcessExec(
                        "budget".to_string(),
                    ));
                }
                let package_id = STATE
                    .lock()
                    .expect("guest state lock")
                    .as_ref()
                    .map(|identity| identity.package_id.clone())
                    .unwrap_or_else(|| "<unbound>".to_string());
                return Ok(SemanticResult {
                    package_id,
                    query: "calls-done".to_string(),
                    node_count: times,
                    source_bytes: 10,
                });
            }
        }
        if text == "denied" {
            // The domain itself asks for a capability the Host policy
            // denies; it receives only the typed answer.
            let answer = perform(&imports::EffectRequest::ProcessExec(
                "whoami".to_string(),
            ));
            return match answer {
                imports::HostAnswer::Denied(reason) => {
                    Err(format!("denied: {reason}"))
                }
                imports::HostAnswer::Ok(_) => {
                    Err("unexpected grant".to_string())
                }
                imports::HostAnswer::Cancelled => {
                    Err("cancelled".to_string())
                }
                imports::HostAnswer::Error(reason) => {
                    Err(format!("error: {reason}"))
                }
            };
        }
        let package_id = STATE
            .lock()
            .expect("guest state lock")
            .as_ref()
            .map(|identity| identity.package_id.clone())
            .unwrap_or_else(|| "<unbound>".to_string());
        let source_bytes = text.len() as u32;
        let node_count = text.split_whitespace().count() as u32;
        Ok(SemanticResult {
            package_id,
            query: text,
            node_count,
            source_bytes,
        })
    }

    fn request_effect(request: EffectRequest) -> HostAnswer {
        // Deterministic oversized-payload marker (conformance only):
        // the guest forges an over-bound answer that the host must
        // reject with a typed output failure.
        if let EffectRequest::ProcessExec(command) = &request {
            if command == "big" {
                return HostAnswer::Error("z".repeat(4096));
            }
        }
        // The domain forwards the request; the Host performs it. The
        // domain receives only the typed answer. The export-side and
        // import-side WIT types are distinct Rust types, so the
        // request is converted explicitly.
        let request = match request {
            EffectRequest::WorkspaceRead((path, max_bytes)) => {
                imports::EffectRequest::WorkspaceRead((path, max_bytes))
            }
            EffectRequest::ProcessExec(command) => {
                imports::EffectRequest::ProcessExec(command)
            }
        };
        match perform(&request) {
            imports::HostAnswer::Ok(text) => HostAnswer::Ok(text),
            imports::HostAnswer::Denied(reason) => HostAnswer::Denied(reason),
            imports::HostAnswer::Cancelled => HostAnswer::Cancelled,
            imports::HostAnswer::Error(reason) => HostAnswer::Error(reason),
        }
    }
}

export!(ConformanceDomain);