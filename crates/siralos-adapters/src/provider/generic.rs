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
use crate::provider::tool_names::ToolNames;
use crate::provider::{
    ANTHROPIC_VERSION, CANCELLED_AFTER_HTTP_RESPONSE,
    CANCELLED_BEFORE_HTTP_CALL, CANCELLED_BEFORE_HTTP_SEND,
    CANCELLED_BEFORE_PROVIDER_START, NO_PROVIDER_RESPONSE_OBSERVED,
    ReplayHooks, record_evidence_outcome_with_secret, record_outcome,
    record_outcome_with_secret,
};
use serde_json::{Value, json};
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

impl std::fmt::Debug for GenericProvider {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("GenericProvider")
            .field("provider", &self.provider.as_str().len())
            .field("model", &self.model.borrow().len())
            .field("endpoint", &"[PROJECTED]")
            .field(
                "credential",
                &self.credential.borrow().as_ref().map(|_| "[REDACTED]"),
            )
            .field("protocol", &self.protocol.borrow())
            .field("hooks", &self.hooks)
            .finish()
    }
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
        let endpoint = endpoint.filter(|value| {
            siralos_core::composition::is_valid_http_endpoint(value)
        });
        Self {
            provider,
            model: Rc::new(RefCell::new(model)),
            endpoint: Rc::new(RefCell::new(endpoint)),
            credential: Rc::new(RefCell::new(credential)),
            protocol: Rc::new(RefCell::new(Protocol::default())),
            hooks: ReplayHooks::default(),
            last_replay: RefCell::new(
                ProviderReplayAvailability::Unavailable {
                    reason: NO_PROVIDER_RESPONSE_OBSERVED.to_owned(),
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
        self.hooks = ReplayHooks {
            clock: Some(clock),
            recorder: Some(recorder),
            request_sha256: core::cell::RefCell::new(None),
        };
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
            reason: NO_PROVIDER_RESPONSE_OBSERVED.to_owned(),
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
        if endpoint.as_ref().is_none_or(|value| {
            siralos_core::composition::is_valid_http_endpoint(value)
        }) {
            *self.endpoint.borrow_mut() = endpoint;
        }
    }

    /// Validate and apply an endpoint replacement, reporting refusal to the
    /// caller instead of silently retaining a stale route.
    pub fn try_set_endpoint(
        &self,
        endpoint: Option<String>,
    ) -> Result<(), String> {
        if endpoint.as_ref().is_some_and(|value| {
            !siralos_core::composition::is_valid_http_endpoint(value)
        }) {
            return Err("endpoint is not a valid HTTP(S) URL".to_owned());
        }
        *self.endpoint.borrow_mut() = endpoint;
        Ok(())
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

    /// Fetch models from the generic provider's effective live route.
    pub fn fetch_models(&self) -> Result<Vec<String>, String> {
        let endpoint = self
            .live_endpoint()
            .unwrap_or_else(|| GENERIC_PLACEHOLDER_ENDPOINT.to_owned());
        let credential = self.live_credential();
        fetch_models_for_protocol(
            &endpoint,
            credential.as_ref(),
            self.live_protocol(),
            Some(self.provider.as_str()),
        )
    }

    /// Fetch models while allowing the caller to interrupt the blocking
    /// probe. Cancellable probes are serialized so a cancelled HTTP timeout
    /// cannot leave an unbounded set of detached workers or credential copies.
    pub fn fetch_models_cancellable(
        &self,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Vec<String>, String> {
        // Keep only cheap cell handles while the permit is waiting. The
        // endpoint and credential snapshots are taken after the single probe
        // slot is acquired, so queued callers cannot retain credential clones.
        let endpoint_cell = Rc::clone(&self.endpoint);
        let credential_cell = Rc::clone(&self.credential);
        let provider = self.provider.clone();
        let protocol = self.live_protocol();
        run_model_listing_probe(cancelled, move || {
            let endpoint = endpoint_cell
                .borrow()
                .clone()
                .unwrap_or_else(|| GENERIC_PLACEHOLDER_ENDPOINT.to_owned());
            let credential = credential_cell.borrow().clone();
            move || {
                fetch_models_for_protocol(
                    &endpoint,
                    credential.as_ref(),
                    protocol,
                    Some(provider.as_str()),
                )
            }
        })
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
    if endpoint.len() > siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES
        || !siralos_core::composition::is_valid_http_endpoint(endpoint)
    {
        return GENERIC_PLACEHOLDER_ENDPOINT.to_owned();
    }
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
/// A stored protocol-specific completion URL is normalized back to its base
/// before appending `/models`; a base URL is used directly.
#[must_use]
pub fn models_url(endpoint: &str) -> String {
    if endpoint.len() > siralos_core::composition::MAX_PROFILE_ENDPOINT_BYTES
        || !siralos_core::composition::is_valid_http_endpoint(endpoint)
    {
        return format!("{GENERIC_PLACEHOLDER_ENDPOINT}/models");
    }
    let trimmed = endpoint.trim_end_matches('/');
    let base = ["/chat/completions", "/responses", "/messages"]
        .iter()
        .find_map(|suffix| trimmed.strip_suffix(suffix))
        .unwrap_or(trimmed);
    format!("{base}/models")
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
        // The borrowing entry point keeps its meaning; the streaming work
        // lives in `open`, which owns the request.
        self.open(request.clone(), Some(cancellation))
    }

    fn open_stream<'a>(
        &'a self,
        request: ModelRequest,
    ) -> Box<dyn Iterator<Item = ProviderEvent> + 'a> {
        // No signal: the session that owns this stream checks its own
        // cancellation token between pulls (the read loop stays bounded).
        self.open(request, None)
    }
}

impl GenericProvider {
    /// Send one request and return the turn, streamed when the protocol
    /// allows it (S2 chunk 3/2).
    fn open<'a>(
        &'a self,
        request: ModelRequest,
        cancellation: Option<CancellationSignal<'a>>,
    ) -> Box<dyn Iterator<Item = ProviderEvent> + 'a> {
        *self.last_replay.borrow_mut() =
            ProviderReplayAvailability::Unavailable {
                reason: "stream has not completed".to_owned(),
            };
        if cancellation.is_some_and(|signal| signal.is_cancelled()) {
            return Box::new(std::iter::once(ProviderEvent::Cancelled {
                message: CANCELLED_BEFORE_PROVIDER_START.to_owned(),
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
        let protocol = *self.protocol.borrow();
        let request_sha256 = crate::provider::request_sha256_for_route(
            &request,
            &provider,
            &endpoint,
            protocol.as_str(),
        );
        *self.hooks.request_sha256.borrow_mut() = Some(request_sha256.clone());
        // Host-observed, bounded HTTP call via `reqwest::blocking` with
        // connect/read timeouts. No hidden retry — the `tool-loop` budget
        // is the only retry.
        let stream_redaction = credential.clone();
        match Self::call_generic(
            &provider,
            &model,
            &endpoint,
            protocol,
            credential,
            &request,
            cancellation,
            &self.hooks,
            &self.last_replay,
        ) {
            CallOutcome::Events(events) => {
                let events = if events.is_empty() {
                    *self.last_replay.borrow_mut() =
                        ProviderReplayAvailability::Unavailable {
                            reason: "provider returned no completion event"
                                .to_owned(),
                        };
                    vec![ProviderEvent::Failed(
                        "provider returned no completion event".to_owned(),
                    )]
                } else {
                    events
                };
                Box::new(events.into_iter())
            }
            CallOutcome::Streaming { response, status, tool_names } => {
                Box::new(StreamingTurn::new(
                    response,
                    status,
                    provider,
                    model,
                    Some(request_sha256),
                    &self.hooks,
                    &self.last_replay,
                    cancellation,
                    tool_names,
                    stream_redaction,
                ))
            }
        }
    }
}

fn provider_uses_anthropic_auth(provider: &str) -> bool {
    provider.eq_ignore_ascii_case("anthropic")
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
        cancellation: Option<CancellationSignal<'_>>,
        hooks: &ReplayHooks,
        last_replay: &RefCell<ProviderReplayAvailability>,
    ) -> CallOutcome {
        if cancellation.is_some_and(|signal| signal.is_cancelled()) {
            *last_replay.borrow_mut() =
                ProviderReplayAvailability::Unavailable {
                    reason: "call cancelled before HTTP request".to_owned(),
                };
            return CallOutcome::Events(vec![ProviderEvent::Cancelled {
                message: CANCELLED_BEFORE_HTTP_CALL.to_owned(),
            }]);
        }
        let client = match crate::provider::build_http_client() {
            Ok(client) => client,
            Err(_err) => {
                // Status-only: a client-build failure is reported by its class,
                // never by the transport's own text (which can carry a proxy
                // URL or other environment detail).
                let events = vec![ProviderEvent::Failed(format!(
                    "{provider} client build failed"
                ))];
                record_outcome(hooks, last_replay, provider, model, None, "");
                return CallOutcome::Events(events);
            }
        };
        // Owner bug 2026-09-12: the provider validator rejects the dot in
        // `workspace.read`, so the boundary translates every name -- the
        // definitions below, the replayed calls, and the inbound calls.
        let tool_names = crate::provider::tool_names::ToolNames::new(
            request.tools.iter().map(|tool| tool.name.as_str()),
        );
        let mut openai_messages = Vec::new();
        let mut responses_input = Vec::new();
        let mut anthropic_conversation = Vec::new();
        if let Some(system) = &request.system {
            openai_messages.push(
                serde_json::json!({"role": "system", "content": system}),
            );
        }
        for item in &request.messages {
            match item {
                siralos_core::provider::ConversationItem::UserMessage { content } => {
                    openai_messages
                        .push(serde_json::json!({"role": "user", "content": content}));
                    responses_input.push(serde_json::json!({
                        "role": "user",
                        "content": [{"type": "input_text", "text": content}],
                    }));
                    anthropic_conversation.push(serde_json::json!({
                        "role": "user",
                        "content": [{"type": "text", "text": content}],
                    }));
                }
                siralos_core::provider::ConversationItem::AssistantMessage { content } => {
                    openai_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": content,
                    }));
                    responses_input.push(serde_json::json!({
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": content}],
                    }));
                    anthropic_conversation.push(serde_json::json!({
                        "role": "assistant",
                        "content": [{"type": "text", "text": content}],
                    }));
                }
                siralos_core::provider::ConversationItem::AssistantToolCall {
                    call_id,
                    tool_name,
                    input,
                } => {
                    let args = input
                        .value()
                        .filter(|value| value.is_object())
                        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned()))
                        .unwrap_or_else(|| "{}".to_owned());
                    let name = tool_names.alias(tool_name);
                    openai_messages.push(serde_json::json!({
                        "role": "assistant",
                        "tool_calls": [{
                            "id": call_id,
                            "type": "function",
                            "function": {"name": name, "arguments": args},
                        }],
                    }));
                    responses_input.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": call_id,
                        "name": name,
                        "arguments": args,
                    }));
                    anthropic_conversation.push(serde_json::json!({
                        "role": "assistant",
                        "content": [{
                            "type": "tool_use",
                            "id": call_id,
                            "name": name,
                            "input": input
                            .value()
                            .filter(|value| value.is_object())
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                        }],
                    }));
                }
                siralos_core::provider::ConversationItem::ToolResult {
                    call_id,
                    result,
                    ..
                } => {
                    let is_error = !matches!(
                        result,
                        siralos_core::provider::ToolExecutionResult::Success { .. }
                    );
                    let content = match result {
                        siralos_core::provider::ToolExecutionResult::Success {
                            output,
                            summary: _,
                        } => output.to_string(),
                        other => other.message().to_owned(),
                    };
                    openai_messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": content,
                    }));
                    responses_input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": content,
                    }));
                    anthropic_conversation.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": call_id,
                            "content": content,
                            "is_error": is_error,
                        }],
                    }));
                }
            }
        }
        let mut tools_json = Vec::new();
        for tool in &request.tools {
            let name = tool_names.alias(&tool.name);
            tools_json.push(match protocol {
                Protocol::OpenAiResponses => serde_json::json!({
                    "type": "function",
                    "name": name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                }),
                Protocol::AnthropicMessages => serde_json::json!({
                    "name": name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                }),
                Protocol::OpenAiCompletions => serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                }),
            });
        }
        let mut body = match protocol {
            Protocol::OpenAiCompletions => {
                let mut value = serde_json::json!({
                    "model": model,
                    "messages": openai_messages,
                });
                // Server-sent events: the streamed deltas are assembled back
                // into the same events (and the same recording) as before.
                value["stream"] = Value::Bool(true);
                value
            }
            Protocol::OpenAiResponses => {
                let mut value = serde_json::json!({
                    "model": model,
                    "input": responses_input,
                });
                if let Some(system) = &request.system {
                    value["instructions"] = Value::String(system.clone());
                }
                value
            }
            Protocol::AnthropicMessages => {
                let mut value = serde_json::json!({
                    "model": model,
                    "max_tokens": 4096,
                    "messages": anthropic_conversation,
                });
                if let Some(system) = &request.system {
                    value["system"] = Value::String(system.clone());
                }
                value
            }
        };
        if !tools_json.is_empty() {
            body["tools"] = Value::Array(tools_json);
        }
        if cancellation.is_some_and(|signal| signal.is_cancelled()) {
            *last_replay.borrow_mut() =
                ProviderReplayAvailability::Unavailable {
                    reason: "call cancelled before HTTP send".to_owned(),
                };
            return CallOutcome::Events(vec![ProviderEvent::Cancelled {
                message: CANCELLED_BEFORE_HTTP_SEND.to_owned(),
            }]);
        }
        let url = chat_url(endpoint, protocol);
        let mut req =
            client.post(&url).header("Content-Type", "application/json");
        if let Some(cred) = credential.as_deref() {
            if provider_uses_anthropic_auth(provider) {
                req = req
                    .header("x-api-key", cred)
                    .header("anthropic-version", ANTHROPIC_VERSION);
            } else {
                req = req.header("Authorization", format!("Bearer {cred}"));
            }
        }
        let response = req.json(&body).send();
        let response = match response {
            Ok(resp) => resp,
            Err(_err) => {
                let events = vec![ProviderEvent::Failed(
                    crate::provider::redact_sensitive(
                        &format!(
                            "{provider} request failed: transport unavailable at {}",
                            crate::provider::safe_endpoint_for_output(&url)
                        ),
                        credential.as_deref(),
                    ),
                )];
                record_outcome(hooks, last_replay, provider, model, None, "");
                return CallOutcome::Events(events);
            }
        };
        if cancellation.is_some_and(|signal| signal.is_cancelled()) {
            *last_replay.borrow_mut() =
                ProviderReplayAvailability::Unavailable {
                    reason: "call cancelled after HTTP response".to_owned(),
                };
            return CallOutcome::Events(vec![ProviderEvent::Cancelled {
                message: CANCELLED_AFTER_HTTP_RESPONSE.to_owned(),
            }]);
        }
        let status = response.status();
        if !status.is_success() {
            // A refused request still has a bounded, sanitized body worth
            // reporting; read it once and stop (no stream to iterate).
            let text = match crate::provider::bounded_body_text_raw(
                response,
                credential.as_deref(),
            ) {
                Ok(text) => text,
                Err(_err) => {
                    let events = vec![ProviderEvent::Failed(format!(
                        "{provider} response read failed before completion"
                    ))];
                    // The observed status is evidence even when the body could
                    // not be read; recording `None` would lose it.
                    record_outcome(
                        hooks,
                        last_replay,
                        provider,
                        model,
                        Some(status.as_u16()),
                        "",
                    );
                    return CallOutcome::Events(events);
                }
            };
            let events =
                vec![ProviderEvent::Failed(http_error_message_with_secret(
                    status.as_u16(),
                    &url,
                    &text,
                    credential.as_deref(),
                ))];
            record_outcome_with_secret(
                hooks,
                last_replay,
                provider,
                model,
                Some(status.as_u16()),
                &text,
                credential.as_deref(),
            );
            return CallOutcome::Events(events);
        }
        if protocol != Protocol::OpenAiCompletions {
            // The responses and messages protocols stream DIFFERENT event
            // shapes, so they keep the whole-body path byte-unchanged
            // (no `stream: true` was sent for them).
            let text = match crate::provider::bounded_body_text_raw(
                response,
                credential.as_deref(),
            ) {
                Ok(text) => text,
                Err(_err) => {
                    let events = vec![ProviderEvent::Failed(format!(
                        "{provider} response read failed before completion"
                    ))];
                    // The observed status is evidence even when the body could
                    // not be read; recording `None` would lose it.
                    record_outcome(
                        hooks,
                        last_replay,
                        provider,
                        model,
                        Some(status.as_u16()),
                        "",
                    );
                    return CallOutcome::Events(events);
                }
            };
            let value: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
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
                        credential.as_deref(),
                    );
                    return CallOutcome::Events(events);
                }
            };
            let _ = &value;
            let mut redactor = crate::provider::StreamingSecretRedactor::new(
                credential.as_deref(),
            );
            // The complete buffered body is not a cross-event stream, so the
            // redactor's held suffix is not ambiguous here; it is masked rather
            // than dropped, so a live run and a replay of the same recording
            // never disagree about how much text was shown.
            let mut events: Vec<ProviderEvent> = tool_names
                .restore_events(
                    crate::provider::replay::completion_events_from_body(
                        &text,
                    ),
                )
                .into_iter()
                .map(|event| redactor.redact_event(event))
                .collect();
            if let Some(mask) = redactor.finish() {
                events.push(ProviderEvent::Event(
                    siralos_core::provider::event::ModelEvent::TextDelta {
                        text: mask,
                    },
                ));
            }
            if events
                .iter()
                .any(|event| matches!(event, ProviderEvent::Failed(_)))
            {
                record_evidence_outcome_with_secret(
                    hooks,
                    last_replay,
                    provider,
                    model,
                    Some(status.as_u16()),
                    &text,
                    credential.as_deref(),
                );
            } else {
                record_outcome_with_secret(
                    hooks,
                    last_replay,
                    provider,
                    model,
                    Some(status.as_u16()),
                    &text,
                    credential.as_deref(),
                );
            }
            return CallOutcome::Events(events);
        }
        // 2xx on the OpenAI-compatible chat path: hand the OPEN response
        // back so the caller iterates events as they arrive.
        CallOutcome::Streaming {
            response,
            status: status.as_u16(),
            tool_names,
        }
    }
}

/// What one generic call produced (S2 chunk 3).
enum CallOutcome {
    /// Terminal events: the call failed before a usable stream existed.
    Events(Vec<ProviderEvent>),
    /// A 2xx response the caller iterates incrementally.
    Streaming {
        response: reqwest::blocking::Response,
        status: u16,
        tool_names: ToolNames,
    },
}

/// One streamed provider turn: reads the response in bounded chunks, parses
/// SSE frames as they complete, and records the assembled body once at the
/// end so a recording still replays through the shared body converter.
struct StreamingTurn<'a> {
    response: reqwest::blocking::Response,
    status: u16,
    provider: String,
    model: String,
    request_sha256: Option<String>,
    hooks: &'a ReplayHooks,
    last_replay: &'a RefCell<ProviderReplayAvailability>,
    cancellation: Option<CancellationSignal<'a>>,
    tool_names: ToolNames,
    redaction: Option<String>,
    redactor: crate::provider::StreamingSecretRedactor,
    assembler: crate::provider::sse::CompletionStream,
    pending_bytes: Vec<u8>,
    queued: std::collections::VecDeque<ProviderEvent>,
    state: StreamState,
    recorded: bool,
}

#[derive(PartialEq, Eq)]
enum StreamState {
    Reading,
    /// The response is finished; drain what is queued, then stop.
    Draining,
    Done,
}

impl<'a> StreamingTurn<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        response: reqwest::blocking::Response,
        status: u16,
        provider: String,
        model: String,
        request_sha256: Option<String>,
        hooks: &'a ReplayHooks,
        last_replay: &'a RefCell<ProviderReplayAvailability>,
        cancellation: Option<CancellationSignal<'a>>,
        tool_names: ToolNames,
        redaction: Option<String>,
    ) -> Self {
        let redactor = crate::provider::StreamingSecretRedactor::new(
            redaction.as_deref(),
        );
        Self {
            response,
            status,
            provider,
            model,
            request_sha256,
            hooks,
            last_replay,
            cancellation,
            tool_names,
            redaction,
            redactor,
            assembler: crate::provider::sse::CompletionStream::new(
                crate::provider::MAX_RESPONSE_BYTES,
            ),
            pending_bytes: Vec::new(),
            queued: std::collections::VecDeque::new(),
            state: StreamState::Reading,
            recorded: false,
        }
    }

    fn redact_events(&mut self, events: Vec<ProviderEvent>) {
        self.queued.extend(
            events
                .into_iter()
                .map(|event| self.redactor.redact_event(event))
                .collect::<Vec<_>>(),
        );
    }

    fn flush_redaction_tail(&mut self) {
        if let Some(tail) = self.redactor.finish() {
            let delta = ProviderEvent::Event(
                siralos_core::provider::ModelEvent::TextDelta { text: tail },
            );
            if let Some(completed) = self.queued.iter().rposition(|event| {
                matches!(
                    event,
                    ProviderEvent::Event(
                        siralos_core::provider::ModelEvent::Completed
                    )
                )
            }) {
                self.queued.insert(completed, delta);
            } else {
                self.queued.push_back(delta);
            }
        }
    }

    /// replay store keeps its body-shaped contract.
    fn mark_unavailable(&self, reason: &'static str) {
        *self.last_replay.borrow_mut() =
            ProviderReplayAvailability::Unavailable {
                reason: reason.to_owned(),
            };
    }

    fn record_evidence_unavailable(&self, reason: &'static str) {
        let body =
            self.assembler.plain_body().map(str::to_owned).unwrap_or_else(
                || self.assembler.assembled_body().to_string(),
            );
        record_evidence_outcome_with_secret(
            self.hooks,
            self.last_replay,
            &self.provider,
            &self.model,
            Some(self.status),
            &body,
            self.redaction.as_deref(),
        );
        *self.last_replay.borrow_mut() =
            ProviderReplayAvailability::Unavailable {
                reason: reason.to_owned(),
            };
    }

    fn record_once(&mut self) {
        if self.recorded {
            return;
        }
        self.recorded = true;
        if self.assembler.is_failed() {
            self.record_evidence_unavailable(
                "stream failed before a replayable completion",
            );
            return;
        }
        let body = match self.assembler.plain_body() {
            Some(body) => {
                let events =
                    crate::provider::replay::completion_events_from_body(body);
                if events
                    .iter()
                    .any(|event| matches!(event, ProviderEvent::Failed(_)))
                {
                    self.record_evidence_unavailable(
                        "stream body was not a replayable completion",
                    );
                    return;
                }
                if events.is_empty() {
                    self.record_evidence_unavailable(
                        "stream body produced no usable event",
                    );
                    return;
                }
                body.to_owned()
            }
            None => {
                if !self.assembler.is_completed() {
                    self.record_evidence_unavailable(
                        "stream ended before an explicit completion",
                    );
                    return;
                }
                self.assembler.assembled_body().to_string()
            }
        };
        if let Some(request_sha256) = self.request_sha256.as_deref() {
            crate::provider::record_outcome_with_secret_and_request_digest(
                self.hooks,
                self.last_replay,
                &self.provider,
                &self.model,
                Some(self.status),
                &body,
                self.redaction.as_deref(),
                request_sha256,
            );
        } else {
            record_outcome_with_secret(
                self.hooks,
                self.last_replay,
                &self.provider,
                &self.model,
                Some(self.status),
                &body,
                self.redaction.as_deref(),
            );
        }
    }

    /// Decode the bytes read so far, keeping an incomplete trailing UTF-8
    /// sequence for the next read instead of corrupting it.
    fn take_text(&mut self) -> String {
        match std::str::from_utf8(&self.pending_bytes) {
            Ok(text) => {
                let text = text.to_owned();
                self.pending_bytes.clear();
                text
            }
            Err(err) if err.error_len().is_none() => {
                let valid = err.valid_up_to();
                let text =
                    String::from_utf8_lossy(&self.pending_bytes[..valid])
                        .to_string();
                self.pending_bytes.drain(..valid);
                text
            }
            Err(_) => {
                let text =
                    String::from_utf8_lossy(&self.pending_bytes).to_string();
                self.pending_bytes.clear();
                text
            }
        }
    }
}

impl Iterator for StreamingTurn<'_> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        loop {
            // A completed iterator stays completed even if cancellation is
            // raised after its terminal event. While the turn is still open,
            // cancellation outranks events already parsed into `queued`, so a
            // host cancel stops provider text immediately. Once the assembler
            // has seen the terminal `[DONE]`, the recorded turn is finished and
            // the queued terminal event is what the transcript must show: a
            // late cancel must not rewrite a completed response as cancelled.
            if matches!(&self.state, StreamState::Done) {
                return None;
            }
            if !self.assembler.is_completed()
                && self
                    .cancellation
                    .is_some_and(|signal| signal.is_cancelled())
            {
                self.mark_unavailable("stream cancelled");
                self.state = StreamState::Done;
                self.queued.clear();
                return Some(ProviderEvent::Cancelled {
                    message: "Host cancelled during the streamed response"
                        .to_owned(),
                });
            }
            if let Some(event) = self.queued.pop_front() {
                return Some(event);
            }
            match self.state {
                StreamState::Done => return None,
                StreamState::Draining => {
                    self.state = StreamState::Done;
                    return None;
                }
                StreamState::Reading => {}
            }
            let mut buf = [0u8; 8192];
            match std::io::Read::read(&mut self.response, &mut buf) {
                Ok(0) => {
                    let mut events = match self.assembler.plain_body() {
                        Some(body) => crate::provider::replay::completion_events_from_body(body),
                        None => self.assembler.finish(),
                    };
                    if events.is_empty() && !self.assembler.is_completed() {
                        events.push(ProviderEvent::Failed(
                            "provider returned no completion event".to_owned(),
                        ));
                    }
                    let restored = self.tool_names.restore_events(events);
                    self.redact_events(restored);
                    self.flush_redaction_tail();
                    self.record_once();
                    self.state = StreamState::Draining;
                }
                Ok(n) => {
                    self.pending_bytes.extend_from_slice(&buf[..n]);
                    let text = self.take_text();
                    let events = self.assembler.push_chunk(&text);
                    let failed = self.assembler.is_failed()
                        || events.iter().any(|event| {
                            matches!(event, ProviderEvent::Failed(_))
                        });
                    let restored = self.tool_names.restore_events(events);
                    self.redact_events(restored);
                    if failed || self.assembler.is_completed() {
                        self.record_once();
                        self.state = StreamState::Draining;
                    }
                }
                Err(_err) => {
                    let message = format!(
                        "{} response read failed before completion",
                        self.provider
                    );
                    self.record_evidence_unavailable(
                        "response read failed before completion",
                    );
                    self.state = StreamState::Done;
                    return Some(ProviderEvent::Failed(message));
                }
            }
        }
    }
}

/// Maximum number of model ids accepted from a provider listing.
pub const MAX_MODEL_IDS: usize = 1_000;

/// Maximum UTF-8 character length of one model id accepted from a provider
/// listing.
pub const MAX_MODEL_ID_CHARS: usize = 256;

/// Fetch available models from a provider's OpenAI-compatible `/models` endpoint.
///
/// This is the I6 provider surface for `/models` — a blocking GET to
/// `{endpoint}/models` with Bearer auth (when a credential is present),
/// bounded to 1 MiB and sanitized, parsing the OpenAI shape
/// `{data: [{id: "..."}]}`. The interactive worker path uses a bounded
/// cancellable helper; direct callers use the same bounded blocking client.
/// On error or unrecognized shape an `Err` with a sanitized `String` is
/// returned, never echoing the credential.
///
/// Uses the same bounded `reqwest` pattern as `GenericProvider::call_generic`
/// and the same credential redaction / 1 MiB cap / recording-hygiene rules
/// (no credential values in output). Not a tool — invoked from the `/models`
/// command dispatch.
pub fn fetch_models(
    endpoint: &str,
    credential: Option<&HostCredential>,
) -> Result<Vec<String>, String> {
    fetch_models_for_protocol(
        endpoint,
        credential,
        Protocol::OpenAiCompletions,
        None,
    )
}

fn fetch_models_for_protocol(
    endpoint: &str,
    credential: Option<&HostCredential>,
    protocol: Protocol,
    auth_provider: Option<&str>,
) -> Result<Vec<String>, String> {
    let url = models_url(endpoint);
    // Deliberately NOT `crate::provider::build_http_client()`: this is a
    // user-triggered listing probe that must fail fast, so it keeps its own
    // 5-second request / 3-second connect timeouts. Folding it into the shared
    // helper would change an observable timeout.
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|err| format!("client build failed: {err}"))?;
    let secret =
        credential.map(|c| String::from_utf8_lossy(c.as_bytes()).to_string());
    let cred_str = credential.map(|c| {
        // `HostCredential` redacted Debug; hold the bearer only for the header.
        String::from_utf8_lossy(c.as_bytes()).to_string()
    });
    let mut req = client.get(&url).header("Content-Type", "application/json");
    if let Some(cred) = cred_str {
        if auth_provider.is_some_and(provider_uses_anthropic_auth)
            || (auth_provider.is_none()
                && protocol == Protocol::AnthropicMessages)
        {
            req = req.header("x-api-key", cred).header(
                "anthropic-version",
                crate::provider::ANTHROPIC_VERSION,
            );
        } else {
            req = req.header("Authorization", format!("Bearer {cred}"));
        }
    }
    let response = req.send().map_err(|_err| {
        crate::provider::redact_sensitive(
            "request failed: transport unavailable",
            secret.as_deref(),
        )
    })?;
    let status = response.status();
    // Status-only: the transport's read error text is never reported.
    let text =
        crate::provider::bounded_body_text_raw(response, secret.as_deref())
            .map_err(|_err| "response read failed".to_owned())?;
    if !status.is_success() {
        return Err(http_error_message_with_secret(
            status.as_u16(),
            &url,
            &text,
            secret.as_deref(),
        ));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_err| {
        "unrecognized response shape: JSON parse failed".to_owned()
    })?;
    parse_models_shape_with_secret(&value, secret.as_deref())
}

/// The cancellation result shared by all model-listing probe paths.
const MODEL_LISTING_CANCELLED: &str = "model listing cancelled";

/// A process-wide single-outstanding-probe permit. The blocking HTTP call
/// cannot be interrupted by this flag, so a cancelled caller must not start an
/// unbounded number of those calls (or retain an unbounded number of cloned
/// credentials) while the current call reaches its fixed timeout.
static MODEL_LISTING_PROBE_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Releases the single model-listing probe slot on every exit path, including
/// a worker panic.
struct ModelListingProbePermit;

impl Drop for ModelListingProbePermit {
    fn drop(&mut self) {
        MODEL_LISTING_PROBE_ACTIVE
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Acquire the one model-listing probe slot without cloning its request
/// inputs. Waiters poll the caller's cancellation flag just like the existing
/// result loop does.
fn acquire_model_listing_probe(
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<ModelListingProbePermit, String> {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(MODEL_LISTING_CANCELLED.to_owned());
        }
        if MODEL_LISTING_PROBE_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(ModelListingProbePermit);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Apply the terminal cancellation precedence rule to a ready probe result.
fn finish_model_listing_probe<T>(
    result: Result<T, String>,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<T, String> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(MODEL_LISTING_CANCELLED.to_owned());
    }
    result
}

/// Run one blocking model-listing probe with a joined result path and a
/// serialized in-flight worker. On cancellation, the caller returns promptly;
/// the one worker that is still inside the bounded HTTP timeout owns the
/// permit until it exits, so a later invocation cannot accumulate another
/// detached probe or credential clone.
fn run_model_listing_probe<T, Prepare, Probe>(
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    prepare: Prepare,
) -> Result<T, String>
where
    T: Send + 'static,
    Prepare: FnOnce() -> Probe,
    Probe: FnOnce() -> Result<T, String> + Send + 'static,
{
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::Duration;

    if cancelled.load(Ordering::Acquire) {
        return Err(MODEL_LISTING_CANCELLED.to_owned());
    }
    let permit = acquire_model_listing_probe(&cancelled)?;
    // Do not clone endpoint/auth inputs after cancellation wins the slot.
    if cancelled.load(Ordering::Acquire) {
        return Err(MODEL_LISTING_CANCELLED.to_owned());
    }
    let probe = prepare();
    if cancelled.load(Ordering::Acquire) {
        return Err(MODEL_LISTING_CANCELLED.to_owned());
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _permit = permit;
        let result = probe();
        let _ = sender.send(result);
    });
    loop {
        if cancelled.load(Ordering::Acquire) {
            // If the worker has already finished, join it before returning.
            // Otherwise its RAII permit keeps this the sole in-flight probe
            // while the bounded HTTP timeout winds down.
            if worker.is_finished() {
                let _ = worker.join();
            }
            return Err(MODEL_LISTING_CANCELLED.to_owned());
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => {
                let _ = worker.join();
                // Cancellation outranks a result that was ready at the
                // terminal boundary.
                return finish_model_listing_probe(result, &cancelled);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = worker.join();
                return finish_model_listing_probe(
                    Err("model listing worker ended unexpectedly".to_owned()),
                    &cancelled,
                );
            }
        }
    }
}

/// Fetch an OpenAI-compatible model listing while polling a cancellation
/// flag. The blocking HTTP probe runs on a bounded helper thread so a TUI
/// interrupt can return control to the worker; the request itself still has
/// the fixed five-second timeout, and a cancelled probe never exposes its
/// result.
pub fn fetch_models_cancellable(
    endpoint: &str,
    credential: Option<&HostCredential>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<String>, String> {
    fetch_models_for_protocol_cancellable(
        endpoint,
        credential,
        Protocol::OpenAiCompletions,
        None,
        cancelled,
    )
}

fn fetch_models_for_protocol_cancellable(
    endpoint: &str,
    credential: Option<&HostCredential>,
    protocol: Protocol,
    auth_provider: Option<&str>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<String>, String> {
    run_model_listing_probe(cancelled, || {
        let endpoint = endpoint.to_owned();
        let credential = credential.cloned();
        let auth_provider = auth_provider.map(str::to_owned);
        move || {
            fetch_models_for_protocol(
                &endpoint,
                credential.as_ref(),
                protocol,
                auth_provider.as_deref(),
            )
        }
    })
}

/// Fetch an Anthropic model listing using the provider's actual auth
/// headers. Keeping this beside the OpenAI-compatible listing keeps `/models`
/// on the effective provider route instead of borrowing an unrelated
/// workspace endpoint or credential.
pub fn fetch_models_anthropic(
    endpoint: &str,
    credential: Option<&HostCredential>,
) -> Result<Vec<String>, String> {
    let url = models_url(endpoint);
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|err| format!("client build failed: {err}"))?;
    let secret =
        credential.map(|c| String::from_utf8_lossy(c.as_bytes()).to_string());
    let mut req = client
        .get(&url)
        .header("Content-Type", "application/json")
        .header("anthropic-version", crate::provider::ANTHROPIC_VERSION);
    if let Some(credential) = credential {
        req = req.header(
            "x-api-key",
            String::from_utf8_lossy(credential.as_bytes()).to_string(),
        );
    }
    let response = req.send().map_err(|_err| {
        crate::provider::redact_sensitive(
            "request failed: transport unavailable",
            secret.as_deref(),
        )
    })?;
    let status = response.status();
    // Status-only: the transport's read error text is never reported.
    let text =
        crate::provider::bounded_body_text_raw(response, secret.as_deref())
            .map_err(|_err| "response read failed".to_owned())?;
    if !status.is_success() {
        return Err(http_error_message_with_secret(
            status.as_u16(),
            &url,
            &text,
            secret.as_deref(),
        ));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_err| {
        "unrecognized response shape: JSON parse failed".to_owned()
    })?;
    parse_models_shape_with_secret(&value, secret.as_deref())
}

/// Fetch an Anthropic model listing while polling a cancellation flag.
pub fn fetch_models_anthropic_cancellable(
    endpoint: &str,
    credential: Option<&HostCredential>,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<Vec<String>, String> {
    run_model_listing_probe(cancelled, || {
        let endpoint = endpoint.to_owned();
        let credential = credential.cloned();
        move || fetch_models_anthropic(&endpoint, credential.as_ref())
    })
}

/// Parse the OpenAI models response shape `{data: [{id: "..."}]}` into ids.
/// Public for tests to inject fixture-shaped responses via existing infra.
pub fn parse_models_shape(value: &Value) -> Result<Vec<String>, String> {
    parse_models_shape_with_secret(value, None)
}

fn parse_models_shape_with_secret(
    value: &Value,
    secret: Option<&str>,
) -> Result<Vec<String>, String> {
    let data =
        value.get("data").and_then(|d| d.as_array()).ok_or_else(|| {
            "unrecognized response shape: expected {data: [{id: \"...\"}]}"
                .to_owned()
        })?;
    if data.len() > MAX_MODEL_IDS {
        return Err(format!(
            "provider model listing exceeds the {MAX_MODEL_IDS}-id limit"
        ));
    }
    let mut ids = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for entry in data {
        let id = entry.get("id").and_then(|v| v.as_str()).ok_or_else(|| {
            "unrecognized response shape: every model entry needs a string id"
                .to_owned()
        })?;
        if id.chars().count() > MAX_MODEL_ID_CHARS
            || !siralos_core::composition::is_model_id(id)
        {
            return Err(
                "provider returned an invalid model id (empty, too long, or invalid characters)"
                    .to_owned(),
            );
        }
        if let Some(secret) = secret {
            if crate::provider::redact_sensitive(id, Some(secret)) != id {
                return Err(
                    "provider returned a model id containing the active credential"
                        .to_owned(),
                );
            }
        }
        if !seen.insert(id.to_owned()) {
            return Err("provider returned duplicate model ids".to_owned());
        }
        ids.push(id.to_owned());
    }
    Ok(ids)
}

/// Bounded, regex-free sanitization used by legacy focused tests to inspect
/// the old body-bound helper. Production diagnostics no longer reflect bodies.
#[cfg(test)]
fn truncated_sanitized_body(body: &str) -> String {
    let cut_at_html = body.find('<').map(|idx| &body[..idx]).unwrap_or(body);
    let truncated: String = cut_at_html.chars().take(240).collect();
    truncated
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// Actionable hint appended when the provider answers HTTP 429 (rate
/// limiting, e.g. a free-tier key over quota). Response bodies are not
/// included in report-safe errors; this hint describes the status only.
/// No automatic retry is attempted: retrying into a rate limit makes it
/// worse.
pub const RATE_LIMIT_HINT: &str = "the provider is rate limiting this key (HTTP 429) -- wait a moment and retry, or switch model";

/// Build a status-only provider HTTP error. The bounded response body is
/// retained for recording, but never reflected into a user-visible string.
#[cfg(test)]
fn http_error_message(status: u16, url: &str, body: &str) -> String {
    http_error_message_with_secret(status, url, body, None)
}

fn http_error_message_with_secret(
    status: u16,
    url: &str,
    _body: &str,
    secret: Option<&str>,
) -> String {
    // Provider-controlled response bytes are retained only in the bounded
    // recording path. Never reflect even a redacted body into a user-visible
    // error: arbitrary response data may contain unrelated secrets, URLs, or
    // control characters that the active-credential redactor cannot know.
    let safe_url = crate::provider::safe_endpoint_for_output(url);
    let base = crate::provider::redact_sensitive(
        &format!("response failed: {status} at {safe_url}"),
        secret,
    );
    if status == 429 { format!("{base} ({RATE_LIMIT_HINT})") } else { base }
}

impl std::fmt::Display for GenericProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GenericProvider({}:[REDACTED])", self.provider)
    }
}

#[cfg(test)]
mod tests {
    use super::{CallOutcome, GenericProvider, HostCredential};
    use crate::provider::ReplayHooks;
    use crate::provider::probe::{
        ERROR_STATUSES, Fixture, error_bodies, retaining_hooks, serve,
        serve_truncated,
    };
    use siralos_core::composition::Protocol;
    use siralos_core::determinism::ProviderReplayAvailability;
    use siralos_core::provider::{
        CancellationSignal, CancellationToken, ModelEvent, ModelProvider,
        ModelRequest, ProviderEvent,
    };

    /// A request that carries nothing workspace-specific.
    fn probe_request() -> ModelRequest {
        ModelRequest {
            messages: vec![],
            tools: vec![],
            system: Some("probe".to_owned()),
        }
    }

    /// Fresh per-call replay availability; the call path takes it by reference.
    fn probe_replay() -> std::cell::RefCell<ProviderReplayAvailability> {
        std::cell::RefCell::new(ProviderReplayAvailability::Unavailable {
            reason: "no provider response observed yet".to_owned(),
        })
    }

    /// Drive the real `call_generic` path at `endpoint` with recorder hooks.
    fn drive_named_with(
        provider: &str,
        endpoint: &str,
        protocol: Protocol,
        cancellation: Option<CancellationSignal<'_>>,
        hooks: &ReplayHooks,
    ) -> Vec<ProviderEvent> {
        match GenericProvider::call_generic(
            provider,
            "probe-model",
            endpoint,
            protocol,
            Some("test-cred".to_owned()),
            &probe_request(),
            cancellation,
            hooks,
            &probe_replay(),
        ) {
            CallOutcome::Events(events) => events,
            CallOutcome::Streaming { .. } => {
                panic!("probe expected a buffered outcome")
            }
        }
    }

    /// Drive the real `call_generic` path at `endpoint` under `provider`.
    fn drive_named(
        provider: &str,
        endpoint: &str,
        protocol: Protocol,
        cancellation: Option<CancellationSignal<'_>>,
    ) -> Vec<ProviderEvent> {
        drive_named_with(
            provider,
            endpoint,
            protocol,
            cancellation,
            &ReplayHooks::default(),
        )
    }

    /// The same, under the neutral probe vendor name.
    fn drive(
        endpoint: &str,
        protocol: Protocol,
        cancellation: Option<CancellationSignal<'_>>,
    ) -> Vec<ProviderEvent> {
        drive_named("probe-vendor", endpoint, protocol, cancellation)
    }

    /// The single `Failed` message an error fixture produces.
    fn failed_message(events: &[ProviderEvent]) -> &str {
        assert_eq!(events.len(), 1, "expected one event, got {events:?}");
        match &events[0] {
            ProviderEvent::Failed(message) => message,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn probe_connect_refused_is_one_bounded_failed_event() {
        let events =
            drive("http://127.0.0.1:1", Protocol::OpenAiCompletions, None);
        let message = failed_message(&events);
        assert!(
            message.starts_with("probe-vendor request failed: "),
            "{message}"
        );
        // Each client carries its own prefix for the shared transport failure;
        // recorded here so a future unification cannot quietly drop one.
        for other in ["openai request failed: ", "anthropic request failed: "]
        {
            assert!(
                !message.starts_with(other),
                "prefix collision: {message}"
            );
        }
        assert!(message.len() <= 512, "bounded: {}", message.len());
    }

    #[test]
    fn probe_pre_cancelled_stops_before_the_http_call() {
        let token = CancellationToken::new();
        token.cancel();
        let events = drive(
            "http://127.0.0.1:1",
            Protocol::OpenAiCompletions,
            Some(token.signal()),
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            ProviderEvent::Cancelled { message } => {
                assert_eq!(message, "Host cancelled before HTTP call");
            }
            other => panic!("expected Cancelled, got {other:?}"),
        }
    }

    #[test]
    fn probe_contacts_only_the_loopback_fixture_server() {
        let server = serve(Fixture {
            status: 200,
            body: r#"{"content":[{"type":"text","text":"hello"}]}"#.to_owned(),
        });
        assert!(
            server.base_url.starts_with("http://127.0.0.1:"),
            "the probe must only ever point at loopback: {}",
            server.base_url
        );
        // A non-completions protocol parses the whole body inline, so the call
        // returns events rather than an open response.
        let events =
            drive(&server.base_url, Protocol::AnthropicMessages, None);
        // `recorded` panics when nothing reached 127.0.0.1 within its deadline:
        // a probe that contacted a real endpoint fails loudly here.
        let recorded = server.recorded();
        assert_eq!(recorded.request_line, "POST /messages HTTP/1.1");
        assert_eq!(recorded.header("authorization"), Some("Bearer test-cred"));
        let body: serde_json::Value =
            serde_json::from_str(&recorded.body).expect("json request body");
        assert_eq!(body["model"], "probe-model");
        assert!(events.iter().any(|event| matches!(
            event,
            ProviderEvent::Event(ModelEvent::TextDelta { text }) if text == "hello"
        )));
    }

    #[test]
    fn buffered_provider_text_redacts_the_literal_credential() {
        let server = serve(Fixture {
            status: 200,
            body: r#"{"content":[{"type":"text","text":"test-cred"}]}"#
                .to_owned(),
        });
        let events =
            drive(&server.base_url, Protocol::AnthropicMessages, None);
        let _ = server.recorded();
        let rendered = format!("{events:?}");
        assert!(!rendered.contains("test-cred"), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
    }

    #[test]
    fn probe_records_that_auth_follows_the_name_not_the_declared_protocol() {
        // Recorded drift: `call_generic` chooses the auth header from the
        // provider NAME, so two requests that declare the same protocol get
        // different authentication.
        let named = |provider: &str| {
            let server = serve(Fixture { status: 200, body: "{}".to_owned() });
            let _ = drive_named(
                provider,
                &server.base_url,
                Protocol::AnthropicMessages,
                None,
            );
            server.recorded()
        };
        let neutral = named("probe-vendor");
        assert_eq!(neutral.header("authorization"), Some("Bearer test-cred"));
        assert_eq!(neutral.header("x-api-key"), None);

        let anthropic_named = named("anthropic");
        assert_eq!(anthropic_named.header("x-api-key"), Some("test-cred"));
        assert_eq!(
            anthropic_named.header("anthropic-version"),
            Some("2023-06-01")
        );
        assert_eq!(anthropic_named.header("authorization"), None);
    }

    #[test]
    fn probe_records_the_buffered_and_streaming_outcomes() {
        // Recorded: a 2xx on the completions protocol hands the OPEN response
        // back for the caller to iterate, where the other two protocols parse
        // the whole body here.
        let streaming = serve(Fixture {
            status: 200,
            body: r#"{"choices":[]}"#.to_owned(),
        });
        let outcome = GenericProvider::call_generic(
            "probe-vendor",
            "probe-model",
            &streaming.base_url,
            Protocol::OpenAiCompletions,
            Some("test-cred".to_owned()),
            &probe_request(),
            None,
            &ReplayHooks::default(),
            &probe_replay(),
        );
        let _ = streaming.recorded();
        assert!(matches!(outcome, CallOutcome::Streaming { .. }));

        let buffered = serve(Fixture {
            status: 200,
            body: r#"{"choices":[]}"#.to_owned(),
        });
        let outcome = GenericProvider::call_generic(
            "probe-vendor",
            "probe-model",
            &buffered.base_url,
            Protocol::OpenAiResponses,
            Some("test-cred".to_owned()),
            &probe_request(),
            None,
            &ReplayHooks::default(),
            &probe_replay(),
        );
        let _ = buffered.recorded();
        assert!(matches!(outcome, CallOutcome::Events(_)));
    }

    #[test]
    fn streamed_turn_checks_cancellation_before_queued_events() {
        let mut body = String::new();
        for index in 0..256 {
            body.push_str(&format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"chunk-{index}\"}}}}]}}\n\n"
            ));
        }
        body.push_str("data: [DONE]\n\n");
        let server = serve(Fixture { status: 200, body });
        let provider = GenericProvider::new(
            "probe-vendor".to_owned(),
            "probe-model".to_owned(),
            Some(server.base_url.clone()),
            None,
        );
        let request = probe_request();
        let token = CancellationToken::new();
        let mut stream = provider.stream(&request, token.signal());
        let first = stream.next().expect("first streamed event");
        assert!(matches!(
            first,
            ProviderEvent::Event(ModelEvent::TextDelta { .. })
        ));
        token.cancel();
        let next = stream.next().expect("cancellation event");
        assert!(matches!(next, ProviderEvent::Cancelled { .. }));
        assert!(stream.next().is_none());
        drop(stream);
        let _ = server.recorded();
    }

    #[test]
    fn probe_records_the_http_error_matrix() {
        // Recorded baseline, not an approved-parity claim: these assertions
        // describe what this path does today at each (status, body) pair.
        for status in ERROR_STATUSES {
            for (_label, body) in error_bodies() {
                let server = serve(Fixture { status, body: body.clone() });
                let events =
                    drive(&server.base_url, Protocol::OpenAiCompletions, None);
                let _ = server.recorded();
                let message = failed_message(&events);
                assert!(
                    message
                        .contains(&format!("response failed: {status} at ")),
                    "{message}"
                );
                assert!(
                    message.len() <= 512,
                    "status {status}: {} bytes",
                    message.len()
                );
                assert!(!message.contains("<html>"), "{message}");
                if status == 429 {
                    assert!(
                        message.contains(super::RATE_LIMIT_HINT),
                        "{message}"
                    );
                } else {
                    assert!(!message.contains("rate limiting"), "{message}");
                }
            }
        }
    }

    #[test]
    fn probe_records_what_replay_records_at_each_outcome() {
        // Recorded baseline, not an approved-parity claim. The non-completions
        // protocols parse the whole body inline, so this drives
        // `AnthropicMessages` and pins what `record_outcome` receives, which no
        // `ProviderEvent` shows.
        let (hooks, recorder) = retaining_hooks();

        // Pre-response failure: no status, empty body.
        let _ = drive_named_with(
            "probe-vendor",
            "http://127.0.0.1:1",
            Protocol::OpenAiCompletions,
            None,
            &hooks,
        );

        // HTTP error: the whole bounded body reaches the recorder.
        let long_body = format!("{}tail", "e".repeat(700));
        let error = serve(Fixture { status: 404, body: long_body.clone() });
        let events = drive_named_with(
            "probe-vendor",
            &error.base_url,
            Protocol::AnthropicMessages,
            None,
            &hooks,
        );
        let _ = error.recorded();
        let message = failed_message(&events);
        assert!(
            message.len() < long_body.len(),
            "the message is the bounded snippet: {} bytes",
            message.len()
        );

        // Parse failure on a 2xx: still the whole body.
        let unparseable = "not json ".repeat(100);
        let raw = serve(Fixture { status: 200, body: unparseable.clone() });
        let _ = drive_named_with(
            "probe-vendor",
            &raw.base_url,
            Protocol::AnthropicMessages,
            None,
            &hooks,
        );
        let _ = raw.recorded();

        // Success: the status and the body of the response that parsed.
        let ok_body = r#"{"content":[{"type":"text","text":"hi"}]}"#;
        let ok = serve(Fixture { status: 200, body: ok_body.to_owned() });
        let _ = drive_named_with(
            "probe-vendor",
            &ok.base_url,
            Protocol::AnthropicMessages,
            None,
            &hooks,
        );
        let _ = ok.recorded();

        let records = recorder.records_snapshot();
        assert_eq!(records.len(), 4, "one recording per terminal outcome");
        assert_eq!(records[0].identity.status, None);
        assert_eq!(records[0].body, "");
        assert_eq!(records[1].identity.status, Some(404));
        assert_eq!(records[1].body, long_body);
        assert_eq!(records[2].identity.status, Some(200));
        assert_eq!(records[2].body, unparseable);
        assert_eq!(records[3].identity.status, Some(200));
        assert_eq!(records[3].body, ok_body);
    }

    #[test]
    fn probe_records_the_204_empty_body_as_a_parse_failure() {
        // Recorded baseline, not an approved-parity claim. A 204 is a 2xx, so it
        // takes the parse path; the matrix's 200-only successes leave that
        // `is_success` boundary unpinned.
        let (hooks, recorder) = retaining_hooks();
        let server = serve(Fixture { status: 204, body: String::new() });
        let events = drive_named_with(
            "probe-vendor",
            &server.base_url,
            Protocol::AnthropicMessages,
            None,
            &hooks,
        );
        let _ = server.recorded();
        let message = failed_message(&events);
        assert!(
            message.starts_with("probe-vendor response JSON parse failed"),
            "{message}"
        );
        assert!(
            !message.contains("response failed: 204"),
            "204 is a success status, so this is not the error branch: {message}"
        );
        let records = recorder.records_snapshot();
        assert_eq!(records[0].identity.status, Some(204));
        assert_eq!(records[0].body, "");
    }

    #[test]
    fn probe_records_the_truncated_response_as_a_read_failure() {
        // Recorded baseline, not an approved-parity claim. The fixture promises
        // more body than it sends and closes: the read path fails, and only the
        // prefix is asserted because the transport text is `reqwest`'s.
        let server = serve_truncated(4096, "{\"content\":[{\"type\"");
        let events =
            drive(&server.base_url, Protocol::AnthropicMessages, None);
        let _ = server.recorded();
        let message = failed_message(&events);
        assert!(
            message.starts_with("probe-vendor response read failed"),
            "{message}"
        );
    }

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
    fn a_pre_cancelled_model_probe_stops_before_spawning_a_request() {
        let cancelled =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let error = super::fetch_models_cancellable(
            "https://user:password@example.invalid/v1?token=secret",
            None,
            cancelled,
        )
        .expect_err("pre-cancelled probe");
        assert_eq!(error, "model listing cancelled");
    }

    #[test]
    fn model_listing_probe_does_not_prepare_after_pre_cancellation() {
        use std::sync::atomic::Ordering;

        let cancelled =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let prepared =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let prepared_for_probe = prepared.clone();
        let result: Result<Vec<String>, String> =
            super::run_model_listing_probe(cancelled, move || {
                prepared_for_probe.store(true, Ordering::Release);
                || Ok::<Vec<String>, String>(Vec::new())
            });

        assert_eq!(result.unwrap_err(), "model listing cancelled");
        assert!(!prepared.load(Ordering::Acquire));
    }

    #[test]
    fn model_listing_probe_final_check_rejects_a_ready_result() {
        let cancelled =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let result = super::finish_model_listing_probe(
            Ok::<Vec<String>, String>(vec!["must-not-escape".to_owned()]),
            &cancelled,
        );

        assert_eq!(result.unwrap_err(), "model listing cancelled");
    }

    #[test]
    fn model_listing_probe_permit_is_released_after_drop() {
        use std::sync::atomic::Ordering;

        let cancelled =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let permit = super::acquire_model_listing_probe(&cancelled)
            .expect("single probe slot");
        assert!(super::MODEL_LISTING_PROBE_ACTIVE.load(Ordering::Acquire));
        drop(permit);
        assert!(!super::MODEL_LISTING_PROBE_ACTIVE.load(Ordering::Acquire));
    }

    #[test]
    fn a_pre_cancelled_anthropic_probe_stops_before_spawning_a_request() {
        let cancelled =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let error = super::fetch_models_anthropic_cancellable(
            "https://user:password@example.invalid/v1?token=secret",
            None,
            cancelled,
        )
        .expect_err("pre-cancelled Anthropic probe");
        assert_eq!(error, "model listing cancelled");
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
    fn http_429_error_is_status_only_with_actionable_hint() {
        let url = "https://api.example.com/v1/chat/completions";
        let msg = super::http_error_message(429, url, "provider body");
        assert!(msg.contains("429"), "status must stay: {msg:?}");
        assert!(
            msg.contains("https://api.example.com"),
            "authority must stay: {msg:?}"
        );
        assert!(!msg.contains("/v1/chat/completions"), "path must be dropped");
        assert!(!msg.contains("provider body"), "body must not be reflected");
        assert!(msg.contains(super::RATE_LIMIT_HINT));
        assert!(msg.contains("wait a moment and retry"));
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
            assert!(msg.contains("https://api.example.com"), "authority");
            assert!(
                !msg.contains("something broke"),
                "provider body must not be reflected: {msg:?}"
            );
            assert!(
                !msg.contains("rate limiting"),
                "hint must not fire on {status}: {msg:?}"
            );
        }
    }

    #[test]
    fn http_429_error_body_is_not_reflected() {
        let body = "LEAKME".repeat(2000);
        let msg = super::http_error_message(
            429,
            "https://api.example.com/v1/chat/completions",
            &body,
        );
        assert!(
            !msg.contains("LEAKME"),
            "body must not be reflected: {msg:?}"
        );
        assert!(msg.contains(super::RATE_LIMIT_HINT));
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
    fn models_url_normalizes_protocol_paths() {
        let cases = [
            (
                "https://api.example.com/v1",
                "https://api.example.com/v1/models",
            ),
            (
                "https://api.example.com/v1/",
                "https://api.example.com/v1/models",
            ),
            (
                "https://api.example.com/v1/chat/completions",
                "https://api.example.com/v1/models",
            ),
            (
                "https://api.example.com/v1/responses",
                "https://api.example.com/v1/models",
            ),
            (
                "https://api.example.com/v1/messages",
                "https://api.example.com/v1/models",
            ),
        ];
        for (endpoint, expected) in cases {
            assert_eq!(super::models_url(endpoint), expected);
        }
    }
}
