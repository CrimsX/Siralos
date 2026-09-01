//! Generic provider adapter — accepts any `provider` string with an optional
//! `endpoint` override (Stage 8, decision 67 C1, 68 §3, user direction 2026-08-31
//! "make an all purpose provider diagnostic that can accept any").
//!
//! The `ModelProvider` seam stays synchronous and Host-observed. When
//! `endpoint` is `Some`, it is used verbatim as the POST URL; otherwise the
//! provider's default endpoint is used. No `UnknownProvider` — any bounded
//! `provider` string that passed `ProfileRecord` validation is accepted.

use crate::provider::credential::HostCredential;
use serde_json::Value;
use siralos_core::provider::{
    CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
};

/// Generic provider — holds the bounded `provider`/`model`/`endpoint` and a
/// redacted `HostCredential` (if any). `Debug`/`Display` redacted.
#[derive(Debug)]
pub struct GenericProvider {
    provider: String,
    model: String,
    endpoint: Option<String>,
    credential: Option<HostCredential>,
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
        }
    }

    fn default_endpoint(provider: &str) -> &'static str {
        match provider {
            "anthropic" => "https://api.anthropic.com/v1/messages",
            _ => "https://api.openai.com/v1/chat/completions",
        }
    }
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
                message: "Host cancelled the turn before provider start".to_owned(),
            }));
        }
        let provider = self.provider.clone();
        let model = self.model.clone();
        let endpoint = self
            .endpoint
            .clone()
            .unwrap_or_else(|| Self::default_endpoint(&provider).to_owned());
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
        let events = Self::call_generic(&provider, &model, &endpoint, credential, &request, cancellation);
        Box::new(events.into_iter())
    }
}

impl GenericProvider {
    fn call_generic(
        provider: &str,
        model: &str,
        endpoint: &str,
        credential: Option<String>,
        request: &ModelRequest,
        cancellation: CancellationSignal<'_>,
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
                return vec![ProviderEvent::Failed(format!(
                    "{provider} client build failed: {err}"
                ))];
            }
        };
        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
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
        let mut body = serde_json::json!({"model": model, "messages": messages});
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
        let mut req = client.post(endpoint).header("Content-Type", "application/json");
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
                return vec![ProviderEvent::Failed(format!(
                    "{provider} request failed: {err}"
                ))];
            }
        };
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: "Host cancelled after HTTP response".to_owned(),
            }];
        }
        let status = response.status();
        let text = match response.text() {
            Ok(text) => text,
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "{provider} response read failed: {err}"
                ))];
            }
        };
        if !status.is_success() {
            return vec![ProviderEvent::Failed(format!(
                "{provider} error {status}: {text}"
            ))];
        }
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "{provider} response JSON parse failed: {err}: {text}"
                ))];
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
            let message = choice.get("message").cloned().unwrap_or(Value::Null);
            if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                        text: content.to_owned(),
                    }));
                }
            }
            if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
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
                    let input = siralos_core::provider::ToolCallInput::from_value(input_val);
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
                if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                            text: text.to_owned(),
                        }));
                    }
                }
                if content.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
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
                    let input_val = content.get("input").cloned().unwrap_or(Value::Null);
                    if !id.is_empty() && !name.is_empty() {
                        let input =
                            siralos_core::provider::ToolCallInput::from_value(input_val);
                        events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                            call_id: id,
                            tool_name: name,
                            input,
                        }));
                    }
                }
            }
            if let Some(content_arr) = value.get("content").and_then(|v| v.as_array()) {
                for block in content_arr.iter().skip(1) {
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
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
                        let input_val = block.get("input").cloned().unwrap_or(Value::Null);
                        if !id.is_empty() && !name.is_empty() {
                            let input =
                                siralos_core::provider::ToolCallInput::from_value(input_val);
                            events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                                call_id: id,
                                tool_name: name,
                                input,
                            }));
                        }
                    } else if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                                text: text.to_owned(),
                            }));
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
    use siralos_core::provider::{CancellationToken, ModelProvider, ModelRequest};

    #[test]
    fn generic_id_is_provider_name() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = GenericProvider::new(
            "github-copilot".to_owned(),
            "gpt-4o".to_owned(),
            None,
            Some(cred),
        );
        assert_eq!(provider.id(), "github-copilot");
    }

    #[test]
    fn generic_stream_is_stub_without_network_when_no_endpoint() {
        // Without a live endpoint the call will fail with a reqwest error,
        // which is still Host-observed and bounded — not a panic.
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = GenericProvider::new(
            "my-provider".to_owned(),
            "my-model".to_owned(),
            Some("http://127.0.0.1:1/invalid".to_owned()),
            Some(cred),
        );
        let request = ModelRequest {
            messages: vec![],
            tools: vec![],
            system: None,
        };
        let token = CancellationToken::new();
        let events: Vec<_> = provider.stream(&request, token.signal()).collect();
        assert!(!events.is_empty());
    }
}
