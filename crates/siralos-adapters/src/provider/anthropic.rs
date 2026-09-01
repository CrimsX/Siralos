//! Anthropic provider adapter — Host-observed, bounded, replay-recordable (Stage 8, decision 67 C3, 68 §3).
//!
//! Mirrors `openai.rs`: the `ModelProvider` seam stays synchronous and
//! Host-observed via `siralos_core::determinism::Clock` and
//! `siralos_core::identity` digests for `determinism-replay`.

use crate::provider::credential::HostCredential;
use siralos_core::provider::{
    CancellationSignal, ModelProvider, ModelRequest, ProviderEvent,
};

/// Anthropic provider — Host-constructed, credential redacted, no network in
/// this slice (stub).
#[derive(Debug)]
pub struct AnthropicProvider {
    /// Redacted credential for anthropic.
    #[allow(dead_code)]
    credential: HostCredential,
    /// Model identifier (bounded, validated at `ProfileRecord` boundary).
    model: String,
}

impl AnthropicProvider {
    /// Create a new `AnthropicProvider` with a redacted `HostCredential` and a
    /// bounded `model` id.
    pub fn new(credential: HostCredential, model: String) -> Self {
        Self { credential, model }
    }
}

impl ModelProvider for AnthropicProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "anthropic"
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        if cancellation.is_cancelled() {
            return Box::new(std::iter::once(ProviderEvent::Cancelled {
                message: "Host cancelled the turn before provider start".to_owned(),
            }));
        }
        let model = self.model.clone();
        let credential = self.credential.as_bytes().to_vec();
        let request = request.clone();
        let _ = (model, credential, request, cancellation);
        Box::new(std::iter::once(ProviderEvent::Failed(
            "anthropic provider not yet implemented — use deterministic-fake for replay".to_owned(),
        )))
    }
}

impl std::fmt::Display for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AnthropicProvider([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::{AnthropicProvider, HostCredential};
    use siralos_core::provider::{CancellationToken, ModelProvider, ModelRequest};

    #[test]
    fn anthropic_id_is_stable() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = AnthropicProvider::new(cred, "claude-3-5-sonnet".to_owned());
        assert_eq!(provider.id(), "anthropic");
    }

    #[test]
    fn anthropic_stream_is_stub_without_network() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = AnthropicProvider::new(cred, "claude-3-5-sonnet".to_owned());
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
