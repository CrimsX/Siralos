//! Provider registry — bounded `provider` string → `ModelProvider` (Stage 8, decision 67 C1, 68 §2).
//!
//! The registry maps the `provider` field from `ProfileRecord` (validated at
//! `profile_config.rs` and `composition.rs`) to a concrete `ModelProvider`.
//! Unknown provider strings are a typed `UnknownProvider` refusal before any
//! network call, with a diagnostic that never echoes the credential.

use crate::provider::credential::HostCredential;
use siralos_core::provider::{ModelProvider, ModelRequest, ProviderEvent};

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
/// For this slice the `openai`/`anthropic` variants are typed stubs that
/// return a `ProviderFailed` event without performing network I/O, so the
/// `UnknownProvider` path and the `HostCredential` redaction can be
/// verified without a live network. The next slice will replace the stubs
/// with the `reqwest`/`hyper` adapters that use the `Clock` and record
/// via `identity` digests for `determinism-replay`.
pub enum HostProvider {
    /// Deterministic fake provider (no credential, echo).
    Fake(crate::provider::deterministic_fake::DeterministicFakeProvider),
    /// OpenAI provider stub (credential redacted, no network in this slice).
    OpenAi {
        /// Redacted credential for openai.
        #[allow(dead_code)]
        credential: HostCredential,
    },
    /// Anthropic provider stub (credential redacted, no network in this slice).
    Anthropic {
        /// Redacted credential for anthropic.
        #[allow(dead_code)]
        credential: HostCredential,
    },
}

impl HostProvider {
    /// Construct a `HostProvider` from a `ProviderKind` and an optional
    /// `HostCredential`. `DeterministicFake` requires no credential;
    /// `OpenAi`/`Anthropic` require `Some(credential)`.
    pub fn from_kind(
        kind: ProviderKind,
        credential: Option<HostCredential>,
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
                Ok(Self::OpenAi { credential })
            }
            ProviderKind::Anthropic => {
                let credential = credential
                    .ok_or_else(|| "anthropic provider requires a credential".to_owned())?;
                Ok(Self::Anthropic { credential })
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
            Self::OpenAi { .. } => "openai",
            Self::Anthropic { .. } => "anthropic",
        }
    }

    fn stream<'a>(
        &'a self,
        request: &'a siralos_core::provider::ModelRequest,
        cancellation: siralos_core::provider::CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        match self {
            Self::Fake(provider) => Box::new(provider.stream(request, cancellation)),
            Self::OpenAi { .. } | Self::Anthropic { .. } => {
                // Stub: Host-observed, bounded, no network I/O in this slice.
                // The next slice will replace this with the `determinism` clock
                // and `identity` digest recording for `determinism-replay`.
                let _ = (request, cancellation);
                Box::new(std::iter::once(ProviderEvent::Failed(
                    "provider not yet implemented — use deterministic-fake for replay".to_owned(),
                )))
            }
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
