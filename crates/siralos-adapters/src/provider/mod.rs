//! Provider adapters (Stage 3R R7.1).
//!
//! This module owns the concrete provider side of the R7.1 contract:
//! the deterministic fake provider (identity 'deterministic-fake',
//! deterministic echo, 16-code-point chunking, and the generic
//! workspace list/read/search scenarios) and the strict bounded-turn
//! collector used by planner/reviewer-style call sites. Both build on
//! the provider-neutral contracts and the shared bounded accounting
//! core in 'siralos-core::provider'.

pub mod anthropic;
pub mod credential;
pub mod deterministic_fake;
pub mod generic;
pub mod openai;
pub mod registry;
pub mod strict_turn;

#[cfg(test)]
mod tests;

pub use credential::HostCredential;
pub use deterministic_fake::{
    DETERMINISTIC_FAKE_PROVIDER_ID, DeterministicFakeProvider,
};
pub use registry::{
    HostProvider, ProviderKind, UnknownProvider, provider_kind_from_str,
};
pub use strict_turn::{
    BoundedModelToolCall, BoundedModelTurnLimits, BoundedModelTurnOutcome,
    collect_bounded_model_turn,
};

/// Hooks for determinism replay recording of provider HTTP responses.
///
/// Holds an optional clock for `observed_at_ms` and an optional recorder for
/// the response identity. The struct is `pub(crate)` and intentionally keeps
/// providers' derived `Debug` intact via a manual `Debug` impl.
#[derive(Default)]
pub(crate) struct ReplayHooks {
    /// Clock for `observed_at_ms` when recording.
    pub clock: Option<std::rc::Rc<dyn siralos_core::determinism::Clock>>,
    /// Recorder for the response identity.
    pub recorder:
        Option<std::rc::Rc<dyn siralos_core::determinism::ReplayRecorder>>,
}

impl std::fmt::Debug for ReplayHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.clock.is_some() || self.recorder.is_some() {
            f.write_str("ReplayHooks(present)")
        } else {
            f.write_str("ReplayHooks(absent)")
        }
    }
}

/// Compute the `sha256` hex of the sanitized bounded body text.
pub(crate) fn response_body_sha256(text: &str) -> String {
    siralos_core::identity::sha256_hex(text.as_bytes())
}

/// Record one provider HTTP outcome for replay.
///
/// `body_text` must be the sanitized bounded text (never the credential).
/// When a recorder is present and recording, the response identity is recorded
/// and `last_replay` is set to `Recorded { digest }`; on digest failure it is
/// set to `Unavailable { reason: "response identity digest failed" }`.
/// Otherwise `last_replay` is set to `Unavailable { reason: "live call not recorded" }`.
pub(crate) fn record_outcome(
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
    provider_id: &str,
    model: &str,
    status: Option<u16>,
    body_text: &str,
) {
    let body_sha256 = response_body_sha256(body_text);
    let body_bytes = body_text.len() as u64;
    let observed_at_ms = hooks.clock.as_ref().map(|c| c.now_ms());
    if hooks.recorder.as_ref().is_some_and(|r| r.is_recording()) {
        let identity = siralos_core::determinism::ProviderResponseIdentity {
            provider_id: provider_id.to_owned(),
            model: model.to_owned(),
            status,
            body_sha256,
            body_bytes,
            observed_at_ms,
        };
        if let Some(recorder) = hooks.recorder.as_ref() {
            recorder.record_provider_response(&identity);
        }
        let digest =
            siralos_core::determinism::compute_provider_response_identity_digest(
                &identity,
            );
        match digest {
            Ok(digest) => {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Recorded {
                        digest,
                    };
            }
            Err(_) => {
                *last_replay.borrow_mut() =
                    siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                        reason: "response identity digest failed".to_owned(),
                    };
            }
        }
    } else {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "live call not recorded".to_owned(),
            };
    }
}

/// Maximum provider response body bytes accepted before truncation.
pub(crate) const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Read an HTTP response body bounded at READ time: at most
/// `MAX_RESPONSE_BYTES + 1` bytes are buffered (via `io::Read::take`), so a
/// hostile endpoint cannot exhaust memory through an unbounded body. The
/// returned text is lossily UTF-8, stripped of control characters (newlines
/// and tabs kept), and marked `...[truncated]` when the bound was hit. The
/// `Err` payload is the raw I/O error for the caller to prefix.
pub(crate) fn bounded_body_text(
    response: reqwest::blocking::Response,
) -> Result<String, String> {
    use std::io::Read;
    let mut limited = response.take((MAX_RESPONSE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    let truncated = bytes.len() > MAX_RESPONSE_BYTES;
    bytes.truncate(MAX_RESPONSE_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    text.retain(|c| !c.is_control() || c == '\n' || c == '\t');
    if truncated {
        text.push_str("...[truncated]");
    }
    Ok(text)
}
