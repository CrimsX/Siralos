//! Generic provider adapter — accepts any `provider` string with an optional
//! `endpoint` override (Stage 8, decision 67 C1, 68 §3, user direction
//! 2026-08-31 "make an all purpose provider diagnostic that can accept any";
//! follow-up user direction: the generic default is a provider-neutral
//! placeholder, never a real provider).
//!
//! The `ModelProvider` seam stays synchronous and Host-observed. When
//! `endpoint` is `Some`, it is used verbatim as the POST URL (host-configured
//! authority, `https://`/`http://` per `ProfileRecord::validate`, no
//! `file:`/`unix:`); otherwise the provider-neutral placeholder
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
use siralos_core::determinism::{
    Clock, ProviderReplayAvailability, ReplayRecorder,
};
use siralos_core::provider::{
    CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Generic provider — holds the bounded `provider`/`model`/`endpoint` and a
/// redacted `HostCredential` (if any). `Debug`/`Display` redacted.
#[derive(Debug)]
pub struct GenericProvider {
    provider: String,
    model: String,
    endpoint: Option<String>,
    credential: Option<HostCredential>,
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
            model,
            endpoint,
            credential,
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

    /// Take the last replay availability, resetting it to unavailable.
    #[must_use]
    pub fn take_last_replay_availability(&self) -> ProviderReplayAvailability {
        self.last_replay.replace(ProviderReplayAvailability::Unavailable {
            reason: "no provider response observed yet".to_owned(),
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
        let model = self.model.clone();
        let endpoint = self
            .endpoint
            .clone()
            .unwrap_or_else(|| GENERIC_PLACEHOLDER_ENDPOINT.to_owned());
        let credential = self.credential.as_ref().map(|c| {
            // Clone the bytes as a String for the header; the `HostCredential`
            // itself stays redacted, and the `String` is held only for the
            // `reqwest` call and never logged.
            String::from_utf8_lossy(c.as_bytes()).to_string()
        });
        let request = request.clone();
        // Host-observed, bounded HTTP call via `reqwest::blocking` with
        // connect/read timeouts. No hidden retry — the `tool-loop` budget
        // is the only retry.
        let events = Self::call_generic(
            &provider,
            &model,
            &endpoint,
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
                        "tool_calls": [{"id": call_id, "type": "function", "function": {"name": tool_name, "arguments": args_str}}]
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
                "function": {"name": tool.name, "description": tool.description, "parameters": tool.input_schema}
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
        let mut req =
            client.post(endpoint).header("Content-Type", "application/json");
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
            let snippet: String = text.chars().take(512).collect();
            let safe: String = snippet
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .collect();
            let events = vec![ProviderEvent::Failed(format!(
                "{provider} error {status}: {safe}"
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
        let mut events = Vec::new();
        // OpenAI-compatible: choices[0].message.content / tool_calls
        // Anthropic: content[0].text / tool_use
        // Try both shapes; whichever yields events is used.
        let choices = value
            .get("choices")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for choice in choices {
            let message =
                choice.get("message").cloned().unwrap_or(Value::Null);
            if let Some(content) =
                message.get("content").and_then(|v| v.as_str())
            {
                if !content.is_empty() {
                    events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                        text: content.to_owned(),
                    }));
                }
            }
            if let Some(tool_calls) =
                message.get("tool_calls").and_then(|v| v.as_array())
            {
                for call in tool_calls {
                    let id = call
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let name = call
                        .get("function")
                        .and_then(|v| v.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let args_str = call
                        .get("function")
                        .and_then(|v| v.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}");
                    let input_val = serde_json::from_str::<Value>(args_str)
                        .unwrap_or(Value::String(args_str.to_owned()));
                    if id.is_empty() || name.is_empty() {
                        continue;
                    }
                    let input =
                        siralos_core::provider::ToolCallInput::from_value(
                            input_val,
                        );
                    events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                        call_id: id,
                        tool_name: name,
                        input,
                    }));
                }
            }
        }
        if events.is_empty() {
            if let Some(content) = value
                .get("content")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
            {
                if let Some(text) =
                    content.get("text").and_then(|v| v.as_str())
                {
                    if !text.is_empty() {
                        events.push(ProviderEvent::Event(
                            ModelEvent::TextDelta { text: text.to_owned() },
                        ));
                    }
                }
                if content.get("type").and_then(|v| v.as_str())
                    == Some("tool_use")
                {
                    let id = content
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let name = content
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_owned();
                    let input_val =
                        content.get("input").cloned().unwrap_or(Value::Null);
                    if !id.is_empty() && !name.is_empty() {
                        let input =
                            siralos_core::provider::ToolCallInput::from_value(
                                input_val,
                            );
                        events.push(ProviderEvent::Event(
                            ModelEvent::ToolCall {
                                call_id: id,
                                tool_name: name,
                                input,
                            },
                        ));
                    }
                }
            }
            if let Some(content_arr) =
                value.get("content").and_then(|v| v.as_array())
            {
                for block in content_arr.iter().skip(1) {
                    if block.get("type").and_then(|v| v.as_str())
                        == Some("tool_use")
                    {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_owned();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_owned();
                        let input_val =
                            block.get("input").cloned().unwrap_or(Value::Null);
                        if !id.is_empty() && !name.is_empty() {
                            let input =
                                siralos_core::provider::ToolCallInput::from_value(input_val);
                            events.push(ProviderEvent::Event(
                                ModelEvent::ToolCall {
                                    call_id: id,
                                    tool_name: name,
                                    input,
                                },
                            ));
                        }
                    } else if let Some(text) =
                        block.get("text").and_then(|v| v.as_str())
                    {
                        if !text.is_empty() {
                            events.push(ProviderEvent::Event(
                                ModelEvent::TextDelta {
                                    text: text.to_owned(),
                                },
                            ));
                        }
                    }
                }
            }
        }
        if events.is_empty() {
            // No content — still complete the turn so the Host doesn't hang.
            events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                text: String::new(),
            }));
        }
        events.push(ProviderEvent::Event(ModelEvent::Completed));
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
}
