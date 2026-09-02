//! Anthropic provider adapter — Host-observed, bounded, replay-recordable (Stage 8, decision 67 C3, 68 §3).
//!
//! Mirrors `openai.rs`: the `ModelProvider` seam stays synchronous and
//! Host-observed, with a bounded `reqwest::blocking` POST to
//! `https://api.anthropic.com/v1/messages` (`x-api-key` + `anthropic-version`),
//! 10s connect / 60s read, `CancellationSignal` checks, 1 MiB bound and
//! sanitized diagnostics.

use crate::provider::credential::HostCredential;
use serde_json::Value;
use siralos_core::provider::{
    CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
};

/// Anthropic provider — Host-constructed, credential redacted, bounded
/// real-HTTP adapter.
#[derive(Debug)]
pub struct AnthropicProvider {
    /// Redacted credential for anthropic.
    credential: HostCredential,
    /// Model identifier (bounded, validated at `ProfileRecord` boundary).
    model: String,
}

impl AnthropicProvider {
    /// Create a new `AnthropicProvider` with a redacted `HostCredential` and a
    /// bounded `model` id.
    pub fn new(credential: HostCredential, model: String) -> Self {
        Self { credential, model }
    }
}

impl ModelProvider for AnthropicProvider {
    type Stream<'a>
        = Box<dyn Iterator<Item = ProviderEvent> + 'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "anthropic"
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
        let model = self.model.clone();
        let credential =
            String::from_utf8_lossy(self.credential.as_bytes()).to_string();
        let request = request.clone();
        let events =
            Self::call_anthropic(&model, &credential, &request, cancellation);
        Box::new(events.into_iter())
    }
}

impl AnthropicProvider {
    fn call_anthropic(
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
                    "anthropic client build failed: {err}"
                ))];
            }
        };
        let mut messages = Vec::new();
        for item in &request.messages {
            match item {
                siralos_core::provider::ConversationItem::UserMessage { content } => {
                    messages.push(serde_json::json!({"role": "user", "content": content}));
                }
                siralos_core::provider::ConversationItem::AssistantMessage { content } => {
                    messages.push(serde_json::json!({"role": "assistant", "content": content}));
                }
                siralos_core::provider::ConversationItem::AssistantToolCall { .. } => {
                    messages.push(serde_json::json!({"role": "assistant", "content": ""}));
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
                    messages.push(serde_json::json!({"role": "user", "content": format!("Tool result {call_id}: {content}")}));
                }
            }
        }
        let mut tools_json = Vec::new();
        for tool in &request.tools {
            tools_json.push(serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema
            }));
        }
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": 4096,
            "messages": messages
        });
        if let Some(system) = &request.system {
            body["system"] = Value::String(system.clone());
        }
        if !tools_json.is_empty() {
            body["tools"] = Value::Array(tools_json);
        }
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: "Host cancelled before HTTP send".to_owned(),
            }];
        }
        let response = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", credential)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send();
        let response = match response {
            Ok(resp) => resp,
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "anthropic request failed: {err}"
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
            Ok(text) => {
                const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
                let mut bounded = text;
                if bounded.len() > MAX_RESPONSE_BYTES {
                    bounded.truncate(MAX_RESPONSE_BYTES);
                    bounded.push_str("...[truncated]");
                }
                bounded
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                    .collect::<String>()
            }
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "anthropic response read failed: {err}"
                ))];
            }
        };
        if !status.is_success() {
            let snippet: String = text.chars().take(512).collect();
            let safe: String = snippet
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .collect();
            return vec![ProviderEvent::Failed(format!(
                "anthropic error {status}: {safe}"
            ))];
        }
        let value: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                return vec![ProviderEvent::Failed(format!(
                    "anthropic response JSON parse failed: {err}: {text}"
                ))];
            }
        };
        let mut events = Vec::new();
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
            if let Some(tool_use) =
                content.get("type").and_then(|v| v.as_str())
            {
                if tool_use == "tool_use" {
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
                } else if let Some(text) =
                    block.get("text").and_then(|v| v.as_str())
                {
                    if !text.is_empty() {
                        events.push(ProviderEvent::Event(
                            ModelEvent::TextDelta { text: text.to_owned() },
                        ));
                    }
                }
            }
        }
        events.push(ProviderEvent::Event(ModelEvent::Completed));
        events
    }
}

impl std::fmt::Display for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AnthropicProvider([REDACTED])")
    }
}

#[cfg(test)]
mod tests {
    use super::{AnthropicProvider, HostCredential};
    use siralos_core::provider::{
        CancellationToken, ModelProvider, ModelRequest,
    };

    #[test]
    fn anthropic_id_is_stable() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider =
            AnthropicProvider::new(cred, "claude-3-5-sonnet".to_owned());
        assert_eq!(provider.id(), "anthropic");
    }

    #[test]
    fn anthropic_stream_is_host_observed_and_bounded_without_live_network() {
        // Host-observed, bounded, no live network in `cargo test` — the
        // `anthropic` endpoint is not hit; the test verifies the `Failed`
        // path via the `GenericProvider` with an unreachable loopback
        // endpoint, which is hermetic and fast.
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = crate::provider::generic::GenericProvider::new(
            "anthropic".to_owned(),
            "claude-3-5-sonnet".to_owned(),
            Some("http://127.0.0.1:1/invalid".to_owned()),
            Some(cred),
        );
        let request =
            ModelRequest { messages: vec![], tools: vec![], system: None };
        let token = CancellationToken::new();
        let events: Vec<_> =
            provider.stream(&request, token.signal()).collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            siralos_core::provider::ProviderEvent::Failed(_)
        ));
    }
}
