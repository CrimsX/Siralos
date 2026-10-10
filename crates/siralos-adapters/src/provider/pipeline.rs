//! Shared HTTP chat pipeline for the model-endpoint clients.
//!
//! Owns the bounded response-body read and the send, read and error-
//! mapping region the openai, anthropic and generic `call_*` paths run
//! once each request is built.

use super::constants::{CANCELLED_AFTER_HTTP_RESPONSE, MAX_RESPONSE_BYTES};
use super::record::{ReplayHooks, record_outcome, record_outcome_with_secret};
use super::redaction::{redact_sensitive, safe_endpoint_for_output};

pub(super) fn bounded_body_text_from_bytes(
    mut bytes: Vec<u8>,
    secret: Option<&str>,
) -> String {
    let truncated = bytes.len() > MAX_RESPONSE_BYTES;
    bytes.truncate(MAX_RESPONSE_BYTES);
    if truncated {
        if let Some(secret) = secret.filter(|value| !value.is_empty()) {
            let max_encoded_len = secret
                .encode_utf16()
                .count()
                .saturating_mul(6)
                .max(secret.len().saturating_mul(3));
            let overlap = max_encoded_len.saturating_sub(1).min(bytes.len());
            bytes.truncate(bytes.len().saturating_sub(overlap));
        }
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("...[truncated]");
    }
    redact_sensitive(&text, secret)
}

/// Read an HTTP response body bounded at read time while retaining its bytes
/// for exact credential redaction. Control characters are not discarded here;
/// callers must redact the raw bounded text before copying any part of it to a
/// diagnostic or sanitized output surface.
pub(crate) fn bounded_body_text_raw(
    response: reqwest::blocking::Response,
    secret: Option<&str>,
) -> Result<String, String> {
    use std::io::Read;
    let mut limited = response.take((MAX_RESPONSE_BYTES + 1) as u64);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    Ok(bounded_body_text_from_bytes(bytes, secret))
}

/// Build the HTTP client the three model-endpoint clients share.
///
/// The openai, anthropic and generic `call_*` paths configure `reqwest`
/// identically — a 60-second request timeout and a 10-second connect timeout —
/// and differ only in the error prefix each puts on a build failure, which its
/// caller supplies. The model-listing probe in `generic` deliberately uses
/// tighter timeouts and does not call this helper.
///
/// # Errors
///
/// Returns the `reqwest` build error unchanged; callers prefix it.
pub(crate) fn build_http_client()
-> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
}

/// What one shared chat-pipeline call produced.
pub(crate) enum ChatPipelineOutcome {
    /// Terminal: the failure is already recorded, and the caller returns these.
    Events(Vec<siralos_core::provider::ProviderEvent>),
    /// The response parsed. The caller performs its own shape extraction.
    Parsed {
        /// The status the body arrived with.
        status: reqwest::StatusCode,
        /// The bounded, sanitized body text, exactly as recorded for replay.
        text: String,
        /// The parsed JSON body.
        value: serde_json::Value,
    },
}

/// Run the region the two chat clients share once a request is ready to send.
///
/// The caller owns everything up to and including building the request — URL,
/// headers and body differ per client — and its own post-parse extraction. This
/// owns the send and its failure mapping, the post-response cancellation check,
/// the bounded read and its failure, the non-success mapping, and the JSON parse
/// and its failure, recording each terminal outcome through [`record_outcome`].
///
/// The two terminal error paths do not reflect provider-controlled response
/// bodies. Their bounded bodies are still passed to the replay recorder for
/// exact, credential-redacted persistence; only the user/event message carries
/// provider/status classification.
// The shared boundary intentionally keeps the request builder, secret redaction,
// cancellation, replay hooks, and state cell as distinct arguments rather than
// hiding them in a broad context object.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_chat_pipeline(
    provider: &str,
    model: &str,
    url: &str,
    request: reqwest::blocking::RequestBuilder,
    secret: Option<&str>,
    cancellation: siralos_core::provider::CancellationSignal<'_>,
    hooks: &ReplayHooks,
    last_replay: &core::cell::RefCell<
        siralos_core::determinism::ProviderReplayAvailability,
    >,
) -> ChatPipelineOutcome {
    use siralos_core::provider::ProviderEvent;
    // Do not echo reqwest's transport error: it can contain the complete
    // destination URL (including query/userinfo). Report only the bounded,
    // userinfo/query-stripped URL supplied by the caller.
    let safe_url = safe_endpoint_for_output(url);
    let response = match request.send() {
        Ok(response) => response,
        Err(_err) => {
            let events = vec![ProviderEvent::Failed(redact_sensitive(
                &format!(
                    "{provider} request failed: transport unavailable at {safe_url}"
                ),
                secret,
            ))];
            record_outcome(hooks, last_replay, provider, model, None, "");
            return ChatPipelineOutcome::Events(events);
        }
    };
    if cancellation.is_cancelled() {
        *last_replay.borrow_mut() =
            siralos_core::determinism::ProviderReplayAvailability::Unavailable {
                reason: "call cancelled".to_owned(),
            };
        return ChatPipelineOutcome::Events(vec![ProviderEvent::Cancelled {
            message: CANCELLED_AFTER_HTTP_RESPONSE.to_owned(),
        }]);
    }
    let status = response.status();
    // Bound the response body at READ time (at most 1 MiB is buffered)
    // and sanitize untrusted data before embedding it in the
    // Host-visible diagnostic.
    let text = if status.as_u16() == 204 {
        String::new()
    } else {
        match bounded_body_text_raw(response, secret) {
            Ok(text) => text,
            Err(_err) => {
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} response read failed"
                ))];
                record_outcome(
                    hooks,
                    last_replay,
                    provider,
                    model,
                    Some(status.as_u16()),
                    "",
                );
                return ChatPipelineOutcome::Events(events);
            }
        }
    };
    if !status.is_success() {
        // Do not reflect any provider-controlled body in a user/event string.
        // The bounded, credential-redacted body remains available only to the
        // replay recorder through the typed hook below.
        let events =
            vec![ProviderEvent::Failed(format!("{provider} error {status}"))];
        record_outcome_with_secret(
            hooks,
            last_replay,
            provider,
            model,
            Some(status.as_u16()),
            &text,
            secret,
        );
        return ChatPipelineOutcome::Events(events);
    }
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_err) => {
            let events = vec![ProviderEvent::Failed(format!(
                "{provider} response JSON parse failed"
            ))];
            record_outcome_with_secret(
                hooks,
                last_replay,
                provider,
                model,
                Some(status.as_u16()),
                &text,
                secret,
            );
            return ChatPipelineOutcome::Events(events);
        }
    };
    ChatPipelineOutcome::Parsed { status, text, value }
}
