//! WIT guest implementation (assurance contract Part 17).
//!
//! The guest performs local semantic work and *requests* any host
//! effect; it holds no filesystem or process authority itself.

wit_bindgen::generate!({
    path: "../world.wit",
    world: "siralos-domain",
});

use std::sync::Mutex;

use exports::siralos::domain_abi::domain_api::{
    Guest, HostAnswer, PackageIdentity, SemanticResult, DeclaredCapability,
};

static STATE: Mutex<Option<String>> = Mutex::new(None);

struct SiralosDomain;

impl Guest for SiralosDomain {
    fn bind(identity: PackageIdentity) -> Result<(), String> {
        *STATE.lock().expect("guest state lock") = Some(identity.package_id);
        Ok(())
    }

    fn query(text: String) -> Result<SemanticResult, String> {
        let package_id = STATE
            .lock()
            .expect("guest state lock")
            .clone()
            .unwrap_or_else(|| "<unbound>".to_string());
        Ok(SemanticResult {
            package_id,
            query: text.clone(),
            node_count: 0,
            source_bytes: text.len() as u32,
        })
    }

    fn request_workspace_read(path: String, max_bytes: u32) -> HostAnswer {
        // The domain asks; the host performs. The answer echoes the
        // mediated request for the host to fulfill.
        HostAnswer::Ok(format!("mediated read of {path} (max {max_bytes} bytes)"))
    }

    fn request_capability(capability: DeclaredCapability) -> HostAnswer {
        match capability {
            DeclaredCapability::WorkspaceRead => HostAnswer::Ok("granted".to_string()),
            DeclaredCapability::ProcessExec => {
                HostAnswer::Denied("process capability not granted by host policy".to_string())
            }
        }
    }
}

export!(SiralosDomain);
