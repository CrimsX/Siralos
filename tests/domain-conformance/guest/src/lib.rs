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