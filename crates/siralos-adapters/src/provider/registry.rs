//! Provider registry — bounded `provider` string → `ModelProvider` (Stage 8, decision 67 C1, 68 §2, all-purpose per user direction 2026-08-31).
//!
//! The registry maps the `provider` field from `ProfileRecord` (validated at
//! `profile_config.rs` and `composition.rs`) to a concrete `ModelProvider`.
//! Three provider kinds are typed (`deterministic-fake`, `openai`,
//! `anthropic`) via `ProviderKind`; any other bounded provider string is
//! accepted via `GenericProvider` (all-purpose, `endpoint` override, no
//! `UnknownProvider` for valid strings). The `UnknownProvider` diagnostic
//! path is retained only for callers that opt into strict matching via
//! `provider_kind_from_str`.

use crate::provider::credential::HostCredential;
use siralos_core::determinism::{
    Clock, ProviderReplayAvailability, ReplayRecorder,
};
use siralos_core::provider::{ModelProvider, ProviderEvent};
use std::rc::Rc;

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
/// Any syntactically valid `provider` string that passed `ProfileRecord`
/// validation is accepted — unknown names become `Generic`, not a refusal.
/// `UnknownProvider` is retained only for the explicit `provider_kind_from_str`
/// diagnostic path used by `HostProvider::from_kind` when the caller opts
/// into strict matching.
pub fn provider_kind_from_str(
    s: &str,
) -> Result<ProviderKind, UnknownProvider> {
    match s {
        "deterministic-fake" => Ok(ProviderKind::DeterministicFake),
        "openai" => Ok(ProviderKind::OpenAi),
        "anthropic" => Ok(ProviderKind::Anthropic),
        other => Err(UnknownProvider { provider_id: other.to_owned() }),
    }
}

/// Strict diagnostic path — kept for callers that want a typed refusal for
/// an unregistered provider. The generic `HostProvider` path below accepts
/// any provider string via `GenericProvider`.
pub fn is_known_provider(s: &str) -> bool {
    matches!(s, "deterministic-fake" | "openai" | "anthropic")
}

/// A Host-constructed provider that is `ModelProvider` over the
/// `deterministic-fake` echo, or over the `openai`/`anthropic`/generic HTTP
/// adapters (which are Host-observed, bounded, 1 MiB, sanitized, and
/// replay-recordable via `siralos_core::determinism` and
/// `siralos_core::identity`). The `generic` path accepts any bounded
/// provider string with an optional `endpoint` override.
pub enum HostProvider {
    /// Deterministic fake provider (no credential, echo).
    Fake(crate::provider::deterministic_fake::DeterministicFakeProvider),
    /// OpenAI provider (credential redacted, Host-observed bounded HTTP
    /// adapter).
    OpenAi(crate::provider::openai::OpenAiProvider),
    /// Anthropic provider (credential redacted, Host-observed bounded HTTP
    /// adapter).
    Anthropic(crate::provider::anthropic::AnthropicProvider),
    /// Generic provider — accepts any bounded `provider` string with an
    /// optional `endpoint` and `credential`, Host-observed and bounded.
    Generic(crate::provider::generic::GenericProvider),
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

    /// Construct a `HostProvider` from an arbitrary `provider` string
    /// (generic, all-purpose) with an optional `model`, `credential`, and
    /// `endpoint`. Any bounded `provider` string that passed
    /// `ProfileRecord` validation is accepted — no `UnknownProvider`.
    /// The generic path uses the default `openai-completions` protocol
    /// (base endpoint + `/chat/completions`).
    pub fn from_provider_str(
        provider: &str,
        model: Option<String>,
        credential: Option<HostCredential>,
        endpoint: Option<String>,
    ) -> Result<Self, String> {
        Self::from_provider_str_with_protocol(
            provider,
            model,
            credential,
            endpoint,
            siralos_core::composition::Protocol::default(),
        )
    }

    /// Construct a `HostProvider` from an arbitrary `provider` string with
    /// an explicit API `protocol` for the generic path. The endpoint stays
    /// a base URL; the protocol selects the chat POST segment, with
    /// full-path endpoints used verbatim (see `generic::chat_url`).
    pub fn from_provider_str_with_protocol(
        provider: &str,
        model: Option<String>,
        credential: Option<HostCredential>,
        endpoint: Option<String>,
        protocol: siralos_core::composition::Protocol,
    ) -> Result<Self, String> {
        if provider == "deterministic-fake" {
            return Ok(Self::Fake(
                crate::provider::deterministic_fake::DeterministicFakeProvider::new(),
            ));
        }
        // For known providers, use the typed OpenAi/Anthropic adapters;
        // for any other provider, use Generic.
        match provider_kind_from_str(provider) {
            Ok(ProviderKind::OpenAi) => Self::from_kind_with_model(
                ProviderKind::OpenAi,
                credential,
                model,
            ),
            Ok(ProviderKind::Anthropic) => Self::from_kind_with_model(
                ProviderKind::Anthropic,
                credential,
                model,
            ),
            Ok(ProviderKind::DeterministicFake) => unreachable!(),
            Err(_) => {
                let model = model.unwrap_or_else(|| {
                    crate::provider::generic::GENERIC_PLACEHOLDER_MODEL
                        .to_owned()
                });
                Ok(Self::Generic(
                    crate::provider::generic::GenericProvider::new(
                        provider.to_owned(),
                        model,
                        endpoint,
                        credential,
                    )
                    .with_protocol(protocol),
                ))
            }
        }
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

    /// Attach replay support via an explicit clock and recorder.
    ///
    /// Wires the `OpenAi`/`Anthropic`/`Generic` variants; returns `Fake`
    /// unchanged.
    #[must_use]
    pub fn with_replay_support(
        self,
        clock: Rc<dyn Clock>,
        recorder: Rc<dyn ReplayRecorder>,
    ) -> Self {
        match self {
            Self::Fake(provider) => Self::Fake(provider),
            Self::OpenAi(provider) => {
                Self::OpenAi(provider.with_replay_support(clock, recorder))
            }
            Self::Anthropic(provider) => {
                Self::Anthropic(provider.with_replay_support(clock, recorder))
            }
            Self::Generic(provider) => {
                Self::Generic(provider.with_replay_support(clock, recorder))
            }
        }
    }

    /// Replace the live model id for the NEXT provider request.
    ///
    /// Interior mutability (`&self` suffices — the session holds the
    /// provider behind a shared reference borrowed by the application).
    /// `Fake` is a model-less echo and ignores the switch; the HTTP
    /// adapters read the same cell at `stream()` time, so the switched id
    /// flows into the request body. Provider/endpoint/credential are
    /// never re-composed here (separate approved slice).
    pub fn set_live_model(&self, model: &str) {
        match self {
            Self::Fake(_) => {}
            Self::OpenAi(provider) => provider.set_model(model.to_owned()),
            Self::Anthropic(provider) => {
                provider.set_model(model.to_owned());
            }
            Self::Generic(provider) => provider.set_model(model.to_owned()),
        }
    }

    /// The model id the NEXT provider request will use, or `None` for
    /// `Fake` (model-less echo).
    #[must_use]
    pub fn live_model(&self) -> Option<String> {
        match self {
            Self::Fake(_) => None,
            Self::OpenAi(provider) => Some(provider.live_model()),
            Self::Anthropic(provider) => Some(provider.live_model()),
            Self::Generic(provider) => Some(provider.live_model()),
        }
    }

    /// Replace the live endpoint base for the NEXT provider request.
    ///
    /// Only `Generic` carries a configurable endpoint: the named adapters
    /// post to fixed URLs and ignore this. `None` restores the
    /// provider-neutral placeholder. `/reload` uses this to apply a changed
    /// `endpoint` without rebuilding the provider.
    pub fn set_live_endpoint(&self, endpoint: Option<String>) {
        if let Self::Generic(provider) = self {
            provider.set_endpoint(endpoint);
        }
    }

    /// The endpoint base the NEXT provider request will use, or `None` when
    /// the provider is not endpoint-configurable or has no endpoint set.
    #[must_use]
    pub fn live_endpoint(&self) -> Option<String> {
        match self {
            Self::Generic(provider) => provider.live_endpoint(),
            _ => None,
        }
    }

    /// Replace the live protocol for the NEXT provider request. Only
    /// `Generic` resolves its POST path segment from the protocol.
    pub fn set_live_protocol(
        &self,
        protocol: siralos_core::composition::Protocol,
    ) {
        if let Self::Generic(provider) = self {
            provider.set_protocol(protocol);
        }
    }

    /// The protocol the NEXT provider request will use, or `None` when the
    /// provider does not resolve its path from a protocol.
    #[must_use]
    pub fn live_protocol(
        &self,
    ) -> Option<siralos_core::composition::Protocol> {
        match self {
            Self::Generic(provider) => Some(provider.live_protocol()),
            _ => None,
        }
    }

    /// Take the last replay availability from the inner provider.
    ///
    /// `Fake` returns `Unavailable` with reason
    /// `"deterministic-fake records no HTTP responses (inherently deterministic echo)"`;
    /// others delegate to their inner provider.
    #[must_use]
    pub fn take_last_replay_availability(&self) -> ProviderReplayAvailability {
        match self {
            Self::Fake(_) => ProviderReplayAvailability::Unavailable {
                reason: "deterministic-fake records no HTTP responses (inherently deterministic echo)".to_owned(),
            },
            Self::OpenAi(provider) => provider.take_last_replay_availability(),
            Self::Anthropic(provider) => provider.take_last_replay_availability(),
            Self::Generic(provider) => provider.take_last_replay_availability(),
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
            Self::Generic(provider) => provider.id(),
        }
    }

    fn stream<'a>(
        &'a self,
        request: &'a siralos_core::provider::ModelRequest,
        cancellation: siralos_core::provider::CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        match self {
            Self::Fake(provider) => {
                Box::new(provider.stream(request, cancellation))
            }
            Self::OpenAi(provider) => {
                Box::new(provider.stream(request, cancellation))
            }
            Self::Anthropic(provider) => {
                Box::new(provider.stream(request, cancellation))
            }
            Self::Generic(provider) => {
                Box::new(provider.stream(request, cancellation))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderKind, provider_kind_from_str};

    #[test]
    fn live_model_switch_reaches_the_next_request() {
        // `set_live_model` replaces the id the NEXT `stream()` reads
        // (the HTTP adapters clone the same cell at call time); the
        // model-less fake reports `None` and ignores the switch.
        let provider = super::HostProvider::from_provider_str_with_protocol(
            "example-vendor",
            Some("example/model-a".to_owned()),
            None,
            Some("https://api.example.com/v1".to_owned()),
            siralos_core::composition::Protocol::OpenAiCompletions,
        )
        .expect("provider");
        assert_eq!(provider.live_model().as_deref(), Some("example/model-a"));
        provider.set_live_model("example/model-b");
        assert_eq!(provider.live_model().as_deref(), Some("example/model-b"));
        let fake = super::HostProvider::from_provider_str_with_protocol(
            "deterministic-fake",
            None,
            None,
            None,
            siralos_core::composition::Protocol::OpenAiCompletions,
        )
        .expect("fake");
        assert_eq!(fake.live_model(), None);
        fake.set_live_model("example/model-b");
        assert_eq!(fake.live_model(), None);
    }

    #[test]
    fn live_endpoint_and_protocol_switches_reach_the_next_request() {
        // `/reload` applies a changed endpoint/protocol the same way `/model`
        // applies a model: by moving the cells the NEXT `stream()` reads. The
        // resolved POST URL follows both, so a changed endpoint reroutes the
        // very next request instead of requiring a restart. The named
        // adapters post to fixed URLs and report `None`.
        let provider = super::HostProvider::from_provider_str_with_protocol(
            "example-vendor",
            Some("example/model-a".to_owned()),
            None,
            Some("https://api.example.com/v1".to_owned()),
            siralos_core::composition::Protocol::OpenAiCompletions,
        )
        .expect("provider");
        assert_eq!(
            provider.live_endpoint().as_deref(),
            Some("https://api.example.com/v1")
        );
        assert_eq!(
            provider.live_protocol(),
            Some(siralos_core::composition::Protocol::OpenAiCompletions)
        );
        provider.set_live_endpoint(Some(
            "https://other.example.com/v2".to_owned(),
        ));
        provider.set_live_protocol(
            siralos_core::composition::Protocol::OpenAiResponses,
        );
        assert_eq!(
            provider.live_endpoint().as_deref(),
            Some("https://other.example.com/v2")
        );
        assert_eq!(
            provider.live_protocol(),
            Some(siralos_core::composition::Protocol::OpenAiResponses)
        );
        // The URL the next request posts to follows both cells.
        let url = crate::provider::generic::chat_url(
            &provider.live_endpoint().expect("endpoint is configured"),
            provider.live_protocol().expect("generic resolves a protocol"),
        );
        assert_eq!(url, "https://other.example.com/v2/responses");
        // A provider without a configurable endpoint reports none, and a
        // switch against it is a no-op rather than an error.
        let fake = super::HostProvider::from_provider_str_with_protocol(
            "deterministic-fake",
            None,
            None,
            None,
            siralos_core::composition::Protocol::OpenAiCompletions,
        )
        .expect("fake");
        assert_eq!(fake.live_endpoint(), None);
        assert_eq!(fake.live_protocol(), None);
        fake.set_live_endpoint(Some(
            "https://other.example.com/v2".to_owned(),
        ));
        fake.set_live_protocol(
            siralos_core::composition::Protocol::OpenAiResponses,
        );
        assert_eq!(fake.live_endpoint(), None);
        assert_eq!(fake.live_protocol(), None);
    }

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
