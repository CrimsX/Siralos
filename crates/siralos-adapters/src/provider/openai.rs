//! OpenAI provider adapter — Host-observed, bounded, replay-recordable (Stage 8, decision 67 C3, 68 §3).
//!
//! The `ModelProvider` seam stays synchronous (`Iterator<Item = ProviderEvent>`)
//! and Host-observed via `siralos_core::determinism::Clock` and
//! `siralos_core::identity` digests for `determinism-replay`. No hidden
//! unbounded retry — the `tool-loop` budget is the only retry.
//!
//! For this slice the adapter is a typed stub that returns a
//! `ProviderFailed` event without performing network I/O, so the
//! `UnknownProvider` path and `HostCredential` redaction can be verified
//! without a live network. The next slice will replace the stub body
//! with the `reqwest::blocking` call that uses the `Clock` for timeouts
//! and records via `identity` digests.

use crate::provider::credential::HostCredential;
use siralos_core::provider::{CancellationSignal, ModelProvider, ModelRequest, ProviderEvent};

/// OpenAI provider — Host-constructed, credential redacted, no network in
/// this slice (stub).
#[derive(Debug)]
pub struct OpenAiProvider {
    /// Redacted credential for openai.
    #[allow(dead_code)]
    credential: HostCredential,
    /// Model identifier (bounded, validated at `ProfileRecord` boundary).
    model: String,
}

impl OpenAiProvider {
    /// Create a new `OpenAiProvider` with a redacted `HostCredential` and a
    /// bounded `model` id. The credential is held in memory only for the
    /// `ModelProvider` call and never written to `siralos.toml`/`siralos.lock`.
    pub fn new(credential: HostCredential, model: String) -> Self {
        Self { credential, model }
    }
}

impl ModelProvider for OpenAiProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "openai"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        // Stub: Host-observed, bounded, no network I/O in this slice.
        // The next slice will use `reqwest::blocking::Client` with the
        // `Clock` for connect/read timeouts and `identity::sha256_hex` for
        // the `determinism-replay` record.
        let _ = &self.model;
        Box::new(std::iter::once(ProviderEvent::Failed(
            "openai provider not yet implemented — use deterministic-fake for replay".to_owned(),
        )))
    }
}

/// Strict `Display` for `OpenAiProvider` — never echoes the credential.
impl std::fmt::Display for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpenAiProvider([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::{HostCredential, OpenAiProvider};
    use siralos_core::provider::{CancellationToken, ModelProvider, ModelRequest};

    #[test]
    fn debug_is_redacted() {
        let cred = HostCredential::from_bytes_for_test(b"sk-secret".to_vec());
        let provider = OpenAiProvider::new(cred, "gpt-4o".to_owned());
        assert!(format!("{provider:?}").contains("[REDACTED]"));
        assert!(!format!("{provider:?}").contains("sk-"));
    }

    #[test]
    fn openai_id_is_stable() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = OpenAiProvider::new(cred, "gpt-4o".to_owned());
        assert_eq!(provider.id(), "openai");
    }

    #[test]
    fn openai_stream_is_stub_without_network() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = OpenAiProvider::new(cred, "gpt-4o".to_owned());
        let request = ModelRequest {
            messages: vec![],
            tools: vec![],
            system: None,
        };
        let token = CancellationToken::new();
        let events: Vec<_> = provider.stream(&request, token.signal()).collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], siralos_core::provider::ProviderEvent::Failed(_)));
    }
}
