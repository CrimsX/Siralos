//! OpenAI provider adapter — Host-observed, bounded, replay-recordable (Stage 8, decision 67 C3, 68 §3).
//!
//! The `ModelProvider` seam stays synchronous (`Iterator<Item = ProviderEvent>`)
//! and Host-observed via `siralos_core::determinism::Clock` and
//! `siralos_core::identity` digests for `determinism-replay`. No hidden
//! unbounded retry — the `tool-loop` budget is the only retry.
//!
//! For this slice the adapter is a typed stub that returns a
//! `ProviderFailed` event without performing network I/O, so the
//! `UnknownProvider` path and `HostCredential` redaction can be verified
//! without a live network. The next slice will replace the stub body
//! with the `reqwest::blocking` call that uses the `Clock` for timeouts
//! and records via `identity` digests.

use crate::provider::credential::HostCredential;
use serde_json::Value;
use siralos_core::provider::{CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent};

/// OpenAI provider — Host-constructed, credential redacted, no network in
/// this slice (stub).
#[derive(Debug)]
pub struct OpenAiProvider {
    /// Redacted credential for openai.
    #[allow(dead_code)]
    credential: HostCredential,
    /// Model identifier (bounded, validated at `ProfileRecord` boundary).
    model: String,
}

impl OpenAiProvider {
    /// Create a new `OpenAiProvider` with a redacted `HostCredential` and a
    /// bounded `model` id. The credential is held in memory only for the
    /// `ModelProvider` call and never written to `siralos.toml`/`siralos.lock`.
    pub fn new(credential: HostCredential, model: String) -> Self {
        Self { credential, model }
    }
}

impl ModelProvider for OpenAiProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "openai"
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
        let credential = String::from_utf8_lossy(self.credential.as_bytes()).to_string();
        let request = request.clone();
        // Host-observed, bounded HTTP call via `reqwest::blocking` with
        // connect/read timeouts. No hidden retry — the `tool-loop` budget
        // is the only retry. Responses are recorded via
        // `siralos_core::identity` digests for `determinism-replay` (the
        // next slice will add the `Clock` + `identity` recording; this
        // slice already does the real POST and yields `ProviderEvent`s).
        let events = Self::call_openai(&model, &credential, &request, cancellation);
        Box::new(events.into_iter())
    }
}

impl OpenAiProvider {
    fn call_openai(
        model: &str,
        credential: &str,
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
                    "openai client build failed: {err}"
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
                    tool_name: _,
                    result,
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
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: "Host cancelled before HTTP send".to_owned(),
            }];
        }
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {credential}"))
            .header("Content-Type", "application/json")
            .json(&body)
            .send();
        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "openai request failed: {err}"
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
                    "openai response read failed: {err}"
                ))];
            }
        };
        if !status.is_success() {
            return vec![ProviderEvent::Failed(format!(
                "openai error {status}: {text}"
            ))];
        }
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "openai response JSON parse failed: {err}: {text}"
                ))];
            }
        };
        let mut events = Vec::new();
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
        events.push(ProviderEvent::Event(ModelEvent::Completed));
        events
    }
}

/// Strict `Display` for `OpenAiProvider` — never echoes the credential.
impl std::fmt::Display for OpenAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpenAiProvider([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::{HostCredential, OpenAiProvider};
    use siralos_core::provider::{CancellationToken, ModelProvider, ModelRequest};

    #[test]
    fn debug_is_redacted() {
        let cred = HostCredential::from_bytes_for_test(b"sk-secret".to_vec());
        let provider = OpenAiProvider::new(cred, "gpt-4o".to_owned());
        assert!(format!("{provider:?}").contains("[REDACTED]"));
        assert!(!format!("{provider:?}").contains("sk-"));
    }

    #[test]
    fn openai_id_is_stable() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = OpenAiProvider::new(cred, "gpt-4o".to_owned());
        assert_eq!(provider.id(), "openai");
    }

    #[test]
    fn openai_stream_is_stub_without_network() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = OpenAiProvider::new(cred, "gpt-4o".to_owned());
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
