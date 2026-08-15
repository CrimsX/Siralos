//! ABI-incompatible conformance guest fixture (Stage 3R R6).
//!
//! Built against a 1.1.0 WIT with an added interface member. The
//! production host (1.0.0) must refuse to instantiate it with a typed
//! failure: unknown/incompatible protocol versions fail explicitly,
//! never by downgrade, reinterpretation, or best-effort matching.

wit_bindgen::generate!({
    path: "wit/domain-abi-1.1.0.wit",
    world: "siralos-domain",
});

use std::sync::Mutex;

use exports::siralos::domain_abi::domain_api::{
    EffectRequest, Guest, HostAnswer, PackageIdentity, SemanticResult,
};
use siralos::domain_abi::domain_api as imports;
use siralos::domain_abi::host_effects::perform;

static STATE: Mutex<Option<PackageIdentity>> = Mutex::new(None);

struct IncompatibleDomain;

impl Guest for IncompatibleDomain {
    fn bind(identity: PackageIdentity) -> Result<(), String> {
        *STATE.lock().expect("guest state lock") = Some(identity);
        Ok(())
    }

    fn query(text: String) -> Result<SemanticResult, String> {
        let package_id = STATE
            .lock()
            .expect("guest state lock")
            .as_ref()
            .map(|identity| identity.package_id.clone())
            .unwrap_or_else(|| "<unbound>".to_string());
        let source_bytes = text.len() as u32;
        Ok(SemanticResult {
            package_id,
            query: text,
            node_count: 0,
            source_bytes,
        })
    }

    fn request_effect(request: EffectRequest) -> HostAnswer {
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

    fn extra() -> u32 {
        1
    }
}

export!(IncompatibleDomain);
