//! Provider registry — bounded `provider` string → `ModelProvider` (Stage 8, decision 67 C1, 68 §2).
//!
//! The registry maps the `provider` field from `ProfileRecord` (validated at
//! `profile_config.rs` and `composition.rs`) to a concrete `ModelProvider`.
//! Unknown provider strings are a typed `UnknownProvider` refusal before any
//! network call, with a diagnostic that never echoes the credential.

use crate::provider::credential::HostCredential;
use siralos_core::provider::{ModelProvider, ProviderEvent};

/// The three provider kinds the Host can construct. `DeterministicFake` is
/// the only one that does not require a `HostCredential`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Deterministic fake provider (echo, no credential).
    DeterministicFake,
    /// OpenAI provider (requires credential).
    OpenAi,
    /// Anthropic provider (requires credential).
    Anthropic,
}

/// Typed refusal for an unregistered provider id. The diagnostic never
/// echoes the credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProvider {
    /// The unregistered provider id (bounded, validated at the boundary).
    pub provider_id: String,
}

impl UnknownProvider {
    /// Deterministic diagnostic that never echoes the credential.
    pub fn diagnostic(&self) -> String {
        format!(
            "unknown provider \"{}\" — configure it or remove the setting",
            self.provider_id
        )
    }
}

impl std::fmt::Display for UnknownProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.diagnostic())
    }
}

impl std::error::Error for UnknownProvider {}

/// Map a bounded `provider` string to a `ProviderKind`, or typed refusal.
pub fn provider_kind_from_str(s: &str) -> Result<ProviderKind, UnknownProvider> {
    match s {
        "deterministic-fake" => Ok(ProviderKind::DeterministicFake),
        "openai" => Ok(ProviderKind::OpenAi),
        "anthropic" => Ok(ProviderKind::Anthropic),
        other => Err(UnknownProvider {
            provider_id: other.to_owned(),
        }),
    }
}

/// A Host-constructed provider that is `ModelProvider` over the
/// `deterministic-fake` echo, or over the `openai`/`anthropic` HTTP
/// adapters (which are Host-observed, bounded, and replay-recordable via
/// `siralos_core::determinism` and `siralos_core::identity`).
///
/// For this slice the `openai`/`anthropic` variants delegate to the typed
/// `OpenAiProvider`/`AnthropicProvider` stubs that return a `ProviderFailed`
/// event without performing network I/O, so the `UnknownProvider` path and
/// the `HostCredential` redaction can be verified without a live network.
/// The next slice will replace the stub bodies with the `reqwest::blocking`
/// call that uses the `Clock` for timeouts and records via `identity`
/// digests for `determinism-replay`.
pub enum HostProvider {
    /// Deterministic fake provider (no credential, echo).
    Fake(crate::provider::deterministic_fake::DeterministicFakeProvider),
    /// OpenAI provider (credential redacted, Host-observed stub in this slice).
    OpenAi(crate::provider::openai::OpenAiProvider),
    /// Anthropic provider (credential redacted, Host-observed stub in this slice).
    Anthropic(crate::provider::anthropic::AnthropicProvider),
}

impl HostProvider {
    /// Construct a `HostProvider` from a `ProviderKind`, an optional
    /// `HostCredential`, and an optional `model` id. `DeterministicFake`
    /// requires no credential; `OpenAi`/`Anthropic` require `Some(credential)`.
    /// When `model` is `None`, the provider's default model is used.
    pub fn from_kind(
        kind: ProviderKind,
        credential: Option<HostCredential>,
    ) -> Result<Self, String> {
        Self::from_kind_with_model(kind, credential, None)
    }

    /// Construct a `HostProvider` with an explicit `model` id.
    pub fn from_kind_with_model(
        kind: ProviderKind,
        credential: Option<HostCredential>,
        model: Option<String>,
    ) -> Result<Self, String> {
        match kind {
            ProviderKind::DeterministicFake => {
                Ok(Self::Fake(
                    crate::provider::deterministic_fake::DeterministicFakeProvider::new(),
                ))
            }
            ProviderKind::OpenAi => {
                let credential = credential
                    .ok_or_else(|| "openai provider requires a credential".to_owned())?;
                let model = model.unwrap_or_else(|| "gpt-4o".to_owned());
                Ok(Self::OpenAi(crate::provider::openai::OpenAiProvider::new(
                    credential, model,
                )))
            }
            ProviderKind::Anthropic => {
                let credential = credential
                    .ok_or_else(|| "anthropic provider requires a credential".to_owned())?;
                let model =
                    model.unwrap_or_else(|| "claude-3-5-sonnet".to_owned());
                Ok(Self::Anthropic(
                    crate::provider::anthropic::AnthropicProvider::new(credential, model),
                ))
            }
        }
    }
}

impl ModelProvider for HostProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        match self {
            Self::Fake(provider) => provider.id(),
            Self::OpenAi(provider) => provider.id(),
            Self::Anthropic(provider) => provider.id(),
        }
    }

    fn stream<'a>(
        &'a self,
        request: &'a siralos_core::provider::ModelRequest,
        cancellation: siralos_core::provider::CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        match self {
            Self::Fake(provider) => Box::new(provider.stream(request, cancellation)),
            Self::OpenAi(provider) => Box::new(provider.stream(request, cancellation)),
            Self::Anthropic(provider) => Box::new(provider.stream(request, cancellation)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderKind, UnknownProvider, provider_kind_from_str};

    #[test]
    fn known_providers_map() {
        assert_eq!(
            provider_kind_from_str("deterministic-fake").unwrap(),
            ProviderKind::DeterministicFake
        );
        assert_eq!(
            provider_kind_from_str("openai").unwrap(),
            ProviderKind::OpenAi
        );
        assert_eq!(
            provider_kind_from_str("anthropic").unwrap(),
            ProviderKind::Anthropic
        );
    }

    #[test]
    fn unknown_provider_is_typed_refusal_without_credential_echo() {
        let err = provider_kind_from_str("reviewer").unwrap_err();
        assert_eq!(err.provider_id, "reviewer");
        assert!(err.diagnostic().contains("unknown provider"));
        assert!(!err.diagnostic().contains("sk-"));
    }

    #[test]
    fn host_provider_requires_credential_for_openai() {
        let kind = ProviderKind::OpenAi;
        assert!(super::HostProvider::from_kind(kind, None).is_err());
    }
}
