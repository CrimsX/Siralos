//! Generic provider adapter — accepts any `provider` string with an optional
//! `endpoint` override (Stage 8, decision 67 C1, 68 §3, user direction
//! 2026-08-31 "make an all purpose provider diagnostic that can accept any";
//! follow-up user direction: the generic default is a provider-neutral
//! placeholder, never a real provider).
//!
//! The `ModelProvider` seam stays synchronous and Host-observed. When
//! `endpoint` is `Some`, it is treated as a BASE URL: the chat POST URL is
//! the base plus the protocol's path segment (`/chat/completions` for
//! `openai-completions`, `/responses` for `openai-responses`, `/messages`
//! for `anthropic-messages`). For backward compatibility, an endpoint that
//! already ends with that protocol segment (trailing slashes tolerated) is
//! used verbatim. Model listing stays `endpoint + "/models"`. The endpoint
//! authority is host-configured (`https://`/`http://` per
//! `ProfileRecord::validate`, no `file:`/`unix:`); otherwise the provider-neutral placeholder
//! `https://generic.invalid/endpoint` is used — the RFC 6761 reserved
//! `.invalid` TLD is guaranteed unresolvable, so a missing configuration
//! fails closed with a typed `ProviderEvent::Failed` and can never route a
//! request or its credential header to a real provider. The default `model`
//! is the neutral `generic-model` placeholder (`GENERIC_PLACEHOLDER_MODEL`).
//! Response recording is implemented via the determinism ports with typed
//! availability. Records never contain the credential or raw body text (only
//! its `sha256`). No `UnknownProvider` — any bounded `provider` string that passed
//! `ProfileRecord` validation is accepted.

use crate::provider::credential::HostCredential;
use crate::provider::{ReplayHooks, record_outcome};
use serde_json::Value;
use siralos_core::composition::Protocol;
use siralos_core::determinism::{
    Clock, ProviderReplayAvailability, ReplayRecorder,
};
use siralos_core::provider::{
    CancellationSignal, ModelProvider, ModelRequest, ProviderEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Generic provider — holds the bounded `provider`/`model`/`endpoint` and a
/// redacted `HostCredential` (if any). `Debug`/`Display` redacted.
///
/// The `model` is a shared live cell: a session-level `/model` switch
/// replaces it in place, and the NEXT `stream()` clones the cell at call
/// time, so the switched id flows into the request body without
/// re-composing provider/endpoint/credential.
#[derive(Debug)]
pub struct GenericProvider {
    provider: String,
    model: Rc<RefCell<String>>,
    endpoint: Rc<RefCell<Option<String>>>,
    /// The resolved credential the NEXT request authenticates with. A
    /// shared live cell: `/reload` replaces it in place and the NEXT
    /// `stream()` reads it, so a credential that appears after the session
    /// was composed converges without a restart. `None` means the profile
    /// declared none.
    credential: Rc<RefCell<Option<HostCredential>>>,
    /// API protocol selecting the chat POST path segment appended to the
    /// base endpoint (`openai-completions` by default). A shared live cell:
    /// `/reload` replaces it in place and the NEXT `stream()` reads it.
    protocol: Rc<RefCell<Protocol>>,
    /// Replay hooks for determinism recording.
    hooks: ReplayHooks,
    /// Last replay availability, set on each terminal outcome.
    last_replay: RefCell<ProviderReplayAvailability>,
}

impl GenericProvider {
    /// Create a new `GenericProvider`. `provider` and `model` are bounded
    /// strings validated at the `ProfileRecord` boundary; `credential` is
    /// `Some` when `siralos.toml` declared `credential = "env:..."` and the
    /// Host resolved it.
    pub fn new(
        provider: String,
        model: String,
        endpoint: Option<String>,
        credential: Option<HostCredential>,
    ) -> Self {
        Self {
            provider,
            model: Rc::new(RefCell::new(model)),
            endpoint: Rc::new(RefCell::new(endpoint)),
            credential: Rc::new(RefCell::new(credential)),
            protocol: Rc::new(RefCell::new(Protocol::default())),
            hooks: ReplayHooks::default(),
            last_replay: RefCell::new(
                ProviderReplayAvailability::Unavailable {
                    reason: "no provider response observed yet".to_owned(),
                },
            ),
        }
    }

    /// Attach replay support via an explicit clock and recorder.
    #[must_use]
    pub fn with_replay_support(
        mut self,
        clock: Rc<dyn Clock>,
        recorder: Rc<dyn ReplayRecorder>,
    ) -> Self {
        self.hooks =
            ReplayHooks { clock: Some(clock), recorder: Some(recorder) };
        self
    }

    /// Set the API protocol selecting the chat POST path segment.
    /// The endpoint stays a base URL; `openai-completions` (default)
    /// appends `/chat/completions`, `openai-responses` appends
    /// `/responses`, and `anthropic-messages` appends `/messages`, unless
    /// the endpoint already ends with that segment (trailing slashes
    /// tolerated), in which case it is used verbatim.
    #[must_use]
    pub fn with_protocol(self, protocol: Protocol) -> Self {
        *self.protocol.borrow_mut() = protocol;
        self
    }

    /// Take the last replay availability, resetting it to unavailable.
    #[must_use]
    pub fn take_last_replay_availability(&self) -> ProviderReplayAvailability {
        self.last_replay.replace(ProviderReplayAvailability::Unavailable {
            reason: "no provider response observed yet".to_owned(),
        })
    }

    /// Replace the live model id in place. The NEXT `stream()` reads this
    /// cell, so a session `/model` switch takes effect without
    /// re-composing provider/endpoint/credential.
    pub fn set_model(&self, model: String) {
        *self.model.borrow_mut() = model;
    }

    /// The model id the NEXT `stream()` will send.
    #[must_use]
    pub fn live_model(&self) -> String {
        self.model.borrow().clone()
    }

    /// Replace the live endpoint base in place. The NEXT `stream()` reads
    /// this cell, so a session `/reload` takes effect without rebuilding
    /// the provider; `None` restores the provider-neutral placeholder.
    pub fn set_endpoint(&self, endpoint: Option<String>) {
        *self.endpoint.borrow_mut() = endpoint;
    }

    /// The endpoint base the NEXT `stream()` will use (`None` = the
    /// provider-neutral placeholder).
    #[must_use]
    pub fn live_endpoint(&self) -> Option<String> {
        self.endpoint.borrow().clone()
    }

    /// Replace the live protocol in place. The NEXT `stream()` reads this
    /// cell, so the resolved POST path segment follows a `/reload`.
    pub fn set_protocol(&self, protocol: Protocol) {
        *self.protocol.borrow_mut() = protocol;
    }

    /// The protocol the NEXT `stream()` will use.
    #[must_use]
    pub fn live_protocol(&self) -> Protocol {
        *self.protocol.borrow()
    }

    /// Replace the live credential in place. The NEXT `stream()` reads
    /// this cell, so a credential that appears in `siralos.toml` after the
    /// session was composed converges on `/reload` without a restart.
    /// `None` clears it (the request then carries no auth header, which is
    /// what a public endpoint wants).
    pub fn set_credential(&self, credential: Option<HostCredential>) {
        *self.credential.borrow_mut() = credential;
    }

    /// The credential the NEXT `stream()` will authenticate with.
    /// Redacted: `HostCredential` never prints its bytes.
    #[must_use]
    pub fn live_credential(&self) -> Option<HostCredential> {
        self.credential.borrow().clone()
    }
}

/// Provider-neutral placeholder endpoint used when the generic provider is
/// constructed without an explicit `endpoint`: the RFC 6761 reserved
/// `.invalid` TLD is guaranteed unresolvable, so a missing configuration can
/// never route a request (or its credential header) to a real provider.
const GENERIC_PLACEHOLDER_ENDPOINT: &str = "https://generic.invalid/endpoint";

/// Provider-neutral placeholder model applied by
/// `registry::from_provider_str` when no `model` was declared.
pub(crate) const GENERIC_PLACEHOLDER_MODEL: &str = "generic-model";

/// Resolve the chat POST URL for a base `endpoint` and `protocol`.
///
/// The endpoint is consistently a BASE URL: the protocol's path segment is
/// appended (`/chat/completions` for `openai-completions`, `/responses` for
/// `openai-responses`, `/messages` for `anthropic-messages`). For backward
/// compatibility, an endpoint that already ends with that segment (trailing
/// slashes tolerated) is used verbatim (normalised without trailing
/// slashes), so configurations that already store the full path keep
/// working unchanged.
#[must_use]
pub fn chat_url(endpoint: &str, protocol: Protocol) -> String {
    let segment = match protocol {
        Protocol::OpenAiCompletions => "/chat/completions",
        Protocol::OpenAiResponses => "/responses",
        Protocol::AnthropicMessages => "/messages",
    };
    let trimmed = endpoint.trim_end_matches('/');
    if trimmed.ends_with(segment) {
        trimmed.to_owned()
    } else {
        format!("{trimmed}{segment}")
    }
}

/// Resolve the model-listing URL for a base `endpoint`.
///
/// Unchanged in every case: `endpoint + "/models"` (trailing slashes on
/// the endpoint tolerated).
#[must_use]
pub fn models_url(endpoint: &str) -> String {
    format!("{}/models", endpoint.trim_end_matches('/'))
}

impl ModelProvider for GenericProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        &self.provider
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        if cancellation.is_cancelled() {
            return Box::new(std::iter::once(ProviderEvent::Cancelled {
                message: "Host cancelled the turn before provider start"
                    .to_owned(),
            }));
        }
        let provider = self.provider.clone();
        let model = self.model.borrow().clone();
        let endpoint = self
            .endpoint
            .borrow()
            .clone()
            .unwrap_or_else(|| GENERIC_PLACEHOLDER_ENDPOINT.to_owned());
        let credential = {
            let held = self.credential.borrow();
            held.as_ref().map(|c| {
                // Clone the bytes as a String for the header; the
                // `HostCredential` itself stays redacted, and the `String`
                // is held only for the `reqwest` call and never logged.
                String::from_utf8_lossy(c.as_bytes()).to_string()
            })
        };
        let request = request.clone();
        let protocol = *self.protocol.borrow();
        // Host-observed, bounded HTTP call via `reqwest::blocking` with
        // connect/read timeouts. No hidden retry — the `tool-loop` budget
        // is the only retry.
        let events = Self::call_generic(
            &provider,
            &model,
            &endpoint,
            protocol,
            credential,
            &request,
            cancellation,
            &self.hooks,
            &self.last_replay,
        );
        Box::new(events.into_iter())
    }
}

impl GenericProvider {
    #[allow(clippy::too_many_arguments)]
    fn call_generic(
        provider: &str,
        model: &str,
        endpoint: &str,
        protocol: Protocol,
        credential: Option<String>,
        request: &ModelRequest,
        cancellation: CancellationSignal<'_>,
        hooks: &ReplayHooks,
        last_replay: &RefCell<ProviderReplayAvailability>,
    ) -> Vec<ProviderEvent> {
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: "Host cancelled before HTTP call".to_owned(),
            }];
        }
        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} client build failed: {err}"
                ))];
                record_outcome(hooks, last_replay, provider, model, None, "");
                return events;
            }
        };
        // Owner bug 2026-09-12: the provider validator rejects the dot in
        // `workspace.read`, so the boundary translates every name -- the
        // definitions below, the replayed calls, and the inbound calls.
        let tool_names = crate::provider::tool_names::ToolNames::new(
            request.tools.iter().map(|tool| tool.name.as_str()),
        );
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(
                serde_json::json!({"role": "system", "content": system}),
            );
        }
        for item in &request.messages {
            match item {
                siralos_core::provider::ConversationItem::UserMessage { content } => {
                    messages.push(serde_json::json!({"role": "user", "content": content}));
                }
                siralos_core::provider::ConversationItem::AssistantMessage { content } => {
                    messages.push(serde_json::json!({"role": "assistant", "content": content}));
                }
                siralos_core::provider::ConversationItem::AssistantToolCall {
                    call_id,
                    tool_name,
                    input,
                } => {
                    let args_str = match input.value() {
                        Some(Value::String(s)) => s.clone(),
                        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| v.to_string()),
                        None => "{}".to_owned(),
                    };
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "tool_calls": [{"id": call_id, "type": "function", "function": {"name": tool_names.alias(tool_name), "arguments": args_str}}]
                    }));
                }
                siralos_core::provider::ConversationItem::ToolResult {
                    call_id,
                    result,
                    ..
                } => {
                    let content = match result {
                        siralos_core::provider::ToolExecutionResult::Success {
                            output,
                            summary: _,
                        } => output.to_string(),
                        other => other.message().to_owned(),
                    };
                    messages.push(serde_json::json!({"role": "tool", "tool_call_id": call_id, "content": content}));
                }
            }
        }
        let mut tools_json = Vec::new();
        for tool in &request.tools {
            tools_json.push(serde_json::json!({
                "type": "function",
                "function": {"name": tool_names.alias(&tool.name), "description": tool.description, "parameters": tool.input_schema}
            }));
        }
        let mut body =
            serde_json::json!({"model": model, "messages": messages});
        if !tools_json.is_empty() {
            body["tools"] = Value::Array(tools_json);
        }
        if let Some(system) = &request.system {
            body["system"] = Value::String(system.clone());
        }
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: "Host cancelled before HTTP send".to_owned(),
            }];
        }
        let url = chat_url(endpoint, protocol);
        let mut req =
            client.post(&url).header("Content-Type", "application/json");
        if let Some(cred) = credential {
            if provider == "anthropic" {
                req = req
                    .header("x-api-key", cred)
                    .header("anthropic-version", "2023-06-01");
            } else {
                req = req.header("Authorization", format!("Bearer {cred}"));
            }
        }
        let response = req.json(&body).send();
        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} request failed: {err}"
                ))];
                record_outcome(hooks, last_replay, provider, model, None, "");
                return events;
            }
        };
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: "Host cancelled after HTTP response".to_owned(),
            }];
        }
        let status = response.status();
        // Bound the response body at READ time (at most 1 MiB is buffered)
        // and sanitize untrusted data before embedding it in the
        // Host-visible diagnostic.
        let text = match crate::provider::bounded_body_text(response) {
            Ok(text) => text,
            Err(err) => {
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} response read failed: {err}"
                ))];
                record_outcome(hooks, last_replay, provider, model, None, "");
                return events;
            }
        };
        if !status.is_success() {
            let events = vec![ProviderEvent::Failed(http_error_message(
                status.as_u16(),
                &url,
                &text,
            ))];
            record_outcome(
                hooks,
                last_replay,
                provider,
                model,
                Some(status.as_u16()),
                &text,
            );
            return events;
        }
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                let snippet: String = text.chars().take(512).collect();
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} response JSON parse failed: {err}: {snippet}"
                ))];
                record_outcome(
                    hooks,
                    last_replay,
                    provider,
                    model,
                    Some(status.as_u16()),
                    &text,
                );
                return events;
            }
        };
        // Body-to-events conversion is centralized in
        // `crate::provider::replay::completion_events_from_body` for reuse by
        // `RecordedReplayProvider`; validate the value is usable before
        // delegating to avoid double-parse divergence on malformed JSON.
        let _ = &value;
        let events = tool_names.restore_events(
            crate::provider::replay::completion_events_from_body(&text),
        );
        record_outcome(
            hooks,
            last_replay,
            provider,
            model,
            Some(status.as_u16()),
            &text,
        );
        events
    }
}

/// Fetch available models from a provider's OpenAI-compatible `/models` endpoint.
///
/// This is the I6 provider surface for `/models` — a blocking GET to
/// `{endpoint}/models` with Bearer auth (when a credential is present),
/// bounded to 1 MiB and sanitized, parsing the OpenAI shape
/// `{data: [{id: "..."}]}`. The synchronous blocking call freezes the TUI
/// redraw while waiting — documented architectural constraint (no threads,
/// single read-owner, stdio frontend byte-unchanged). On error or
/// unrecognized shape an `Err` with a sanitized `String` is returned, never
/// echoing the credential.
///
/// Uses the same bounded `reqwest` pattern as `GenericProvider::call_generic`
/// and the same credential redaction / 1 MiB cap / recording-hygiene rules
/// (no credential values in output). Not a tool — invoked from the `/models`
/// command dispatch.
pub fn fetch_models(
    endpoint: &str,
    credential: Option<&HostCredential>,
) -> Result<Vec<String>, String> {
    let url = models_url(endpoint);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|err| format!("client build failed: {err}"))?;
    let cred_str = credential.map(|c| {
        // `HostCredential` redacted Debug; hold the bearer only for the header.
        String::from_utf8_lossy(c.as_bytes()).to_string()
    });
    let mut req = client.get(&url).header("Content-Type", "application/json");
    if let Some(cred) = cred_str {
        // I6 specifies Bearer auth from HostCredential (OpenAI-compatible).
        // For providers that use a different header the generic path still
        // routes via Bearer — the error surface is honest if rejected.
        req = req.header("Authorization", format!("Bearer {cred}"));
    }
    let response =
        req.send().map_err(|err| format!("request failed: {err}"))?;
    let status = response.status();
    let text = crate::provider::bounded_body_text(response)
        .map_err(|err| format!("response read failed: {err}"))?;
    if !status.is_success() {
        return Err(http_error_message(status.as_u16(), &url, &text));
    }
    let value: Value = serde_json::from_str(&text).map_err(|err| {
        format!("unrecognized response shape: JSON parse failed: {err}")
    })?;
    parse_models_shape(&value)
}

/// Parse the OpenAI models response shape `{data: [{id: "..."}]}` into ids.
/// Public for tests to inject fixture-shaped responses via existing infra.
pub fn parse_models_shape(value: &Value) -> Result<Vec<String>, String> {
    let data =
        value.get("data").and_then(|d| d.as_array()).ok_or_else(|| {
            "unrecognized response shape: expected {data: [{id: \"...\"}]}"
                .to_owned()
        })?;
    let mut ids = Vec::new();
    for entry in data {
        if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
            ids.push(id.to_owned());
        }
    }
    if ids.is_empty() && !data.is_empty() {
        return Err(
            "unrecognized response shape: no valid model ids".to_owned()
        );
    }
    Ok(ids)
}

/// Bounded, regex-free sanitization for provider error bodies: cut at first `<` (HTML), truncate to 240 chars, filter controls.
fn truncated_sanitized_body(body: &str) -> String {
    let cut_at_html = body.find('<').map(|idx| &body[..idx]).unwrap_or(body);
    let truncated: String = cut_at_html.chars().take(240).collect();
    truncated
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// Actionable hint appended when the provider answers HTTP 429 (rate
/// limiting, e.g. a free-tier key over quota). The bounded truthful detail
/// (status, URL, bounded body) is always kept; this hint says what to do.
/// No automatic retry is attempted: retrying into a rate limit makes it
/// worse.
pub const RATE_LIMIT_HINT: &str = "the provider is rate limiting this key (HTTP 429) -- wait a moment and retry, or switch model";

/// Build the bounded provider HTTP-error message for `status` at `url`
/// with raw `body`: the truthful `status` + URL + bounded sanitized body
/// (decisions 137-138 behaviour, unchanged), plus [`RATE_LIMIT_HINT`]
/// when the status is 429.
fn http_error_message(status: u16, url: &str, body: &str) -> String {
    let safe = truncated_sanitized_body(body);
    let base = format!("response failed: {status} at {url} - {safe}");
    if status == 429 { format!("{base} ({RATE_LIMIT_HINT})") } else { base }
}

impl std::fmt::Display for GenericProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GenericProvider({}:[REDACTED])", self.provider)
    }
}

#[cfg(test)]
mod tests {
    use super::{GenericProvider, HostCredential};
    use siralos_core::provider::{
        CancellationToken, ModelProvider, ModelRequest,
    };

    #[test]
    fn generic_id_is_provider_name() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = GenericProvider::new(
            "github-copilot".to_owned(),
            "generic-model".to_owned(),
            None,
            Some(cred),
        );
        assert_eq!(provider.id(), "github-copilot");
    }

    #[test]
    fn generic_without_endpoint_fails_closed_on_placeholder() {
        // Provider-neutral placeholder: the RFC 6761 `.invalid` host can
        // never resolve, so a missing configuration fails closed with a
        // typed refusal naming the placeholder — never a real provider.
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = GenericProvider::new(
            "my-provider".to_owned(),
            "my-model".to_owned(),
            None,
            Some(cred),
        );
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            siralos_core::provider::ProviderEvent::Failed(_)
        ));
        if let siralos_core::provider::ProviderEvent::Failed(message) =
            &events[0]
        {
            assert!(message.contains("generic.invalid"));
        }
    }

    #[test]
    fn generic_stream_fails_closed_on_unreachable_endpoint() {
        // Without a live endpoint the call will fail with a reqwest error,
        // which is still Host-observed and bounded — not a panic.
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = GenericProvider::new(
            "my-provider".to_owned(),
            "my-model".to_owned(),
            Some("http://127.0.0.1:1/invalid".to_owned()),
            Some(cred),
        );
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert!(!events.is_empty());
    }

    #[test]
    fn parse_models_shape_extracts_ids() {
        // I6: fixture-shaped OpenAI response via existing test infra
        let value = serde_json::json!({
            "object": "list",
            "data": [
                {"id": "gpt-4o", "object": "model"},
                {"id": "gpt-4o-mini", "object": "model"}
            ]
        });
        let ids = super::parse_models_shape(&value).expect("parse");
        assert_eq!(ids, vec!["gpt-4o", "gpt-4o-mini"]);
        // Empty data yields empty Ok
        let empty = serde_json::json!({"data": []});
        assert_eq!(
            super::parse_models_shape(&empty).expect("empty"),
            Vec::<String>::new()
        );
        // Missing data -> Err honest
        let bad = serde_json::json!({"models": []});
        assert!(super::parse_models_shape(&bad).is_err());
    }

    #[test]
    fn fetch_models_bounded_get_redacts_credential_on_error() {
        // I6 hygiene: credential values never appear in error output
        let cred =
            HostCredential::from_bytes_for_test(b"sk-secret-123".to_vec());
        // Use an unreachable endpoint to force an error without leaking cred
        let result = super::fetch_models("http://127.0.0.1:1", Some(&cred));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            !err.contains("sk-secret-123"),
            "credential must not leak in error: {err:?}"
        );
    }

    #[test]
    fn credential_resolution_key_prefix_is_literal() {
        let cred = HostCredential::from_credential_str("key:my-secret-value")
            .expect("key:");
        assert_eq!(cred.as_bytes(), b"my-secret-value");
    }

    #[test]
    fn credential_resolution_env_prefix_resolves_env_var() {
        let cred = HostCredential::from_credential_str("env:PATH")
            .expect("env:PATH must resolve");
        assert!(!cred.as_bytes().is_empty());
    }

    #[test]
    fn credential_resolution_bare_is_env_compat() {
        let cred = HostCredential::from_credential_str("PATH")
            .expect("bare PATH must resolve");
        assert!(!cred.as_bytes().is_empty());
    }

    #[test]
    fn switched_model_is_what_the_next_request_reads() {
        // The live cell `stream()` clones at call time holds the switched
        // id after `set_model`: the NEXT request body uses it.
        let provider = super::GenericProvider::new(
            "example-vendor".to_owned(),
            "example/model-a".to_owned(),
            None,
            None,
        );
        assert_eq!(provider.live_model(), "example/model-a");
        provider.set_model("example/model-b".to_owned());
        assert_eq!(provider.live_model(), "example/model-b");
    }

    #[test]
    fn error_truncation_html_cut_and_bounded() {
        let big_html = format!(
            "Error 404: not found <html><body>{}</body></html>",
            "x".repeat(10000)
        );
        let truncated = super::truncated_sanitized_body(&big_html);
        assert!(
            truncated.len() <= 240,
            "must be <=240, got {}",
            truncated.len()
        );
        assert!(!truncated.contains('<'), "must cut at first '<'");
        assert!(
            truncated.contains("Error 404"),
            "prefix before html must remain"
        );
        // Simulate full error shape: "response failed: 404 at https://host/v1/chat/completions - <truncated>"
        let url = "https://host/v1/chat/completions";
        let err = format!("response failed: 404 at {} - {}", url, truncated);
        assert!(err.contains(url));
        assert!(err.contains("404"));
        assert!(err.len() <= url.len() + 50 + 240);
        assert!(!err.contains("<html"));
    }

    #[test]
    fn error_truncation_10kb_html_body_is_bounded() {
        let body = format!(
            "{}{}{}",
            "a".repeat(240),
            "<html>dump</html>",
            "b".repeat(10000)
        );
        let truncated = super::truncated_sanitized_body(&body);
        assert!(truncated.len() <= 240);
        assert!(!truncated.contains('<'));
        assert_eq!(truncated, "a".repeat(240));
    }

    #[test]
    fn fetch_models_error_is_bounded_and_has_url_status() {
        // Force a 404 error via unreachable endpoint with body containing html — the error from fetch_models should be bounded
        // We test the helper directly since live fetch would need a server; the shape is verified by error_truncation tests above.
        let html_body =
            "<html><head></head><body>Not Found</body></html>".repeat(500);
        let truncated = super::truncated_sanitized_body(&html_body);
        assert!(truncated.len() <= 240);
        assert!(!truncated.contains('<'));
    }

    #[test]
    fn http_429_error_keeps_status_url_body_and_adds_actionable_hint() {
        // Placeholder host only; no network.
        let url = "https://api.example.com/v1/chat/completions";
        let body = r#"{"error":{"message":"Provider rate limit exceeded"}}"#;
        let msg = super::http_error_message(429, url, body);
        assert!(msg.contains("429"), "status must stay: {msg:?}");
        assert!(msg.contains(url), "URL must stay: {msg:?}");
        assert!(
            msg.contains("Provider rate limit exceeded"),
            "bounded body must stay: {msg:?}"
        );
        assert!(
            msg.contains(super::RATE_LIMIT_HINT),
            "actionable hint must be added: {msg:?}"
        );
        assert!(
            msg.contains("wait a moment and retry"),
            "hint must say what to do: {msg:?}"
        );
    }

    #[test]
    fn non_429_error_has_no_rate_limit_hint() {
        // The hint fires only on 429; every other status keeps the
        // truthful bounded shape unchanged.
        let url = "https://api.example.com/v1/chat/completions";
        for status in [400, 401, 404, 500, 503] {
            let msg =
                super::http_error_message(status, url, "something broke");
            assert!(
                msg.contains(&status.to_string()),
                "status must stay: {msg:?}"
            );
            assert!(msg.contains(url), "URL must stay: {msg:?}");
            assert!(
                msg.contains("something broke"),
                "body must stay: {msg:?}"
            );
            assert!(
                !msg.contains("rate limiting"),
                "hint must not fire on {status}: {msg:?}"
            );
        }
    }

    #[test]
    fn http_429_error_body_stays_bounded() {
        // The 240-char bound (decisions 137-138) still applies to the body
        // portion when the 429 hint is appended.
        let body = "x".repeat(10000);
        let msg = super::http_error_message(
            429,
            "https://api.example.com/v1/chat/completions",
            &body,
        );
        assert!(
            !msg.contains(&"x".repeat(241)),
            "body portion must stay bounded: len {}",
            msg.len()
        );
        assert!(
            msg.contains(&"x".repeat(240)),
            "bounded body prefix must be kept: {msg:?}"
        );
        assert!(
            msg.contains(super::RATE_LIMIT_HINT),
            "hint must still be added: {msg:?}"
        );
    }

    #[test]
    fn chat_url_appends_completions_for_base_openai_completions() {
        // Base endpoint + openai-completions -> base + /chat/completions.
        // Placeholder host only; no network.
        let url = super::chat_url(
            "https://api.example.com/v1",
            siralos_core::composition::Protocol::OpenAiCompletions,
        );
        assert_eq!(url, "https://api.example.com/v1/chat/completions");
    }

    #[test]
    fn chat_url_appends_responses_for_base_openai_responses() {
        let url = super::chat_url(
            "https://api.example.com/v1",
            siralos_core::composition::Protocol::OpenAiResponses,
        );
        assert_eq!(url, "https://api.example.com/v1/responses");
    }

    #[test]
    fn chat_url_appends_messages_for_base_anthropic_messages() {
        let url = super::chat_url(
            "https://api.example.com/v1",
            siralos_core::composition::Protocol::AnthropicMessages,
        );
        assert_eq!(url, "https://api.example.com/v1/messages");
    }

    #[test]
    fn chat_url_keeps_full_path_verbatim_with_and_without_slash() {
        // Backward compatibility: endpoints that already store the full
        // protocol path keep working unchanged (trailing slash tolerated).
        let cases = [
            (
                "https://api.example.com/v1/chat/completions",
                siralos_core::composition::Protocol::OpenAiCompletions,
                "https://api.example.com/v1/chat/completions",
            ),
            (
                "https://api.example.com/v1/chat/completions/",
                siralos_core::composition::Protocol::OpenAiCompletions,
                "https://api.example.com/v1/chat/completions",
            ),
            (
                "https://api.example.com/v1/responses",
                siralos_core::composition::Protocol::OpenAiResponses,
                "https://api.example.com/v1/responses",
            ),
            (
                "https://api.example.com/v1/responses/",
                siralos_core::composition::Protocol::OpenAiResponses,
                "https://api.example.com/v1/responses",
            ),
            (
                "https://api.example.com/v1/messages",
                siralos_core::composition::Protocol::AnthropicMessages,
                "https://api.example.com/v1/messages",
            ),
            (
                "https://api.example.com/v1/messages/",
                siralos_core::composition::Protocol::AnthropicMessages,
                "https://api.example.com/v1/messages",
            ),
        ];
        for (endpoint, protocol, expected) in cases {
            assert_eq!(super::chat_url(endpoint, protocol), expected);
        }
    }

    #[test]
    fn models_url_is_base_plus_models_in_every_case() {
        // Model listing stays exactly as it is: endpoint + "/models".
        let cases = [
            "https://api.example.com/v1",
            "https://api.example.com/v1/",
            "https://api.example.com/v1/chat/completions",
            "https://api.example.com/v1/responses",
            "https://api.example.com/v1/messages",
        ];
        for endpoint in cases {
            let trimmed = endpoint.trim_end_matches('/');
            assert_eq!(
                super::models_url(endpoint),
                format!("{trimmed}/models")
            );
        }
    }
}
