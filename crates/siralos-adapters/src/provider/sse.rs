//! Server-sent-event assembly for streamed chat completions (S2 chunk 3).
//!
//! The generic provider used to read the whole response body and convert it
//! once, so a turn produced every event at the end -- which is why the UI
//! could not repaint while the model was working. This module turns the SSE
//! stream back into the SAME events, incrementally, and assembles the
//! equivalent non-streamed body so RECORDINGS stay replayable through
//! [`crate::provider::replay::completion_events_from_body`].
//!
//! Pure by construction: `push_chunk` takes raw text and returns the events
//! that text completed. No I/O, no clock, no provider specifics beyond the
//! documented OpenAI-compatible delta shape.

use serde_json::{Value, json};
use siralos_core::provider::{ModelEvent, ProviderEvent, ToolCallInput};

/// One accumulated tool call (OpenAI streams them by index, in pieces).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// Incremental assembler for one streamed completion.
#[derive(Debug)]
pub struct CompletionStream {
    /// Bytes accepted so far, against `limit`.
    bytes: usize,
    limit: usize,
    /// Partial trailing line (SSE frames arrive split across reads).
    pending: String,
    event_id: String,
    model: String,
    content: String,
    /// Reasoning text the route volunteered (S3 renders it; nothing is
    /// emitted for it yet).
    reasoning: String,
    calls: Vec<PartialCall>,
    finish_reason: Option<String>,
    usage: Option<Value>,
    completed: bool,
    /// A raw (non-SSE) body that arrived instead of a stream.
    plain_body: Option<String>,
}

impl CompletionStream {
    /// An assembler bounded to `limit` response bytes.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: 0,
            limit,
            pending: String::new(),
            event_id: String::new(),
            model: String::new(),
            content: String::new(),
            reasoning: String::new(),
            calls: Vec::new(),
            finish_reason: None,
            usage: None,
            completed: false,
            plain_body: None,
        }
    }

    /// True once the assembler saw `[DONE]` or a completion marker.
    #[must_use]
    pub fn is_completed(&self) -> bool {
        self.completed
    }

    /// The raw body when the endpoint answered with plain JSON instead of a
    /// stream (some gateways ignore `stream: true`).
    #[must_use]
    pub fn plain_body(&self) -> Option<&str> {
        self.plain_body.as_deref()
    }

    /// Bytes accepted so far.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The reasoning text the route volunteered, if any.
    #[must_use]
    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }

    /// Feed one raw response chunk; returns the events it completed.
    pub fn push_chunk(&mut self, chunk: &str) -> Vec<ProviderEvent> {
        if self.bytes.saturating_add(chunk.len()) > self.limit {
            self.bytes = self.limit.saturating_add(1);
            return vec![ProviderEvent::Failed(format!(
                "{chunk_len} more bytes would exceed the {limit}-byte response bound",
                chunk_len = chunk.len(),
                limit = self.limit,
            ))];
        }
        self.bytes += chunk.len();
        // A plain JSON body (no `data:` framing) is handed back verbatim.
        if self.pending.is_empty() && self.plain_body.is_none() {
            let head = chunk.trim_start();
            if head.starts_with('{') {
                let mut body = self.plain_body.take().unwrap_or_default();
                body.push_str(chunk);
                self.plain_body = Some(body);
                return Vec::new();
            }
        }
        if self.plain_body.is_some() {
            let mut body = self.plain_body.take().unwrap_or_default();
            body.push_str(chunk);
            self.plain_body = Some(body);
            return Vec::new();
        }
        let mut events = Vec::new();
        self.pending.push_str(chunk);
        while let Some(pos) = self.pending.find('\n') {
            let line = self.pending[..pos].trim_end_matches('\r').to_owned();
            self.pending.drain(..=pos);
            self.consume_line(&line, &mut events);
        }
        events
    }

    /// Flush the last line when the stream ends without a trailing newline.
    pub fn finish(&mut self) -> Vec<ProviderEvent> {
        let mut events = Vec::new();
        let tail = std::mem::take(&mut self.pending);
        let tail = tail.trim();
        if !tail.is_empty() {
            self.consume_line(tail, &mut events);
        }
        if !self.completed {
            events.extend(self.commit());
        }
        events
    }

    fn consume_line(&mut self, line: &str, events: &mut Vec<ProviderEvent>) {
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            return;
        }
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data == "[DONE]" {
            events.extend(self.commit());
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            // A malformed frame is skipped rather than failing the turn: the
            // non-streamed path would not have seen it at all.
            return;
        };
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            if self.event_id.is_empty() {
                self.event_id = id.to_owned();
            }
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            if self.model.is_empty() {
                self.model = model.to_owned();
            }
        }
        if let Some(usage) = value.get("usage") {
            if !usage.is_null() {
                self.usage = Some(usage.clone());
            }
        }
        let Some(choices) = value.get("choices").and_then(Value::as_array)
        else {
            return;
        };
        for choice in choices {
            if let Some(reason) =
                choice.get("finish_reason").and_then(Value::as_str)
            {
                self.finish_reason = Some(reason.to_owned());
            }
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            if let Some(text) = delta.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    self.content.push_str(text);
                    events.push(ProviderEvent::Event(ModelEvent::TextDelta {
                        text: text.to_owned(),
                    }));
                }
            }
            for field in ["reasoning", "reasoning_content"] {
                if let Some(text) = delta.get(field).and_then(Value::as_str) {
                    self.reasoning.push_str(text);
                }
            }
            if let Some(calls) =
                delta.get("tool_calls").and_then(Value::as_array)
            {
                for call in calls {
                    let index =
                        call.get("index").and_then(Value::as_u64).unwrap_or(0)
                            as usize;
                    while self.calls.len() <= index {
                        self.calls.push(PartialCall::default());
                    }
                    let slot = &mut self.calls[index];
                    if let Some(id) = call.get("id").and_then(Value::as_str)
                        && !id.is_empty()
                        && slot.id.is_empty()
                    {
                        slot.id = id.to_owned();
                    }
                    if let Some(function) = call.get("function") {
                        if let Some(name) =
                            function.get("name").and_then(Value::as_str)
                            && !name.is_empty()
                            && slot.name.is_empty()
                        {
                            slot.name = name.to_owned();
                        }
                        if let Some(args) =
                            function.get("arguments").and_then(Value::as_str)
                        {
                            slot.arguments.push_str(args);
                        }
                    }
                }
            }
        }
    }

    /// Emit the tool calls and the completion, exactly once.
    fn commit(&mut self) -> Vec<ProviderEvent> {
        if self.completed {
            return Vec::new();
        }
        self.completed = true;
        let mut events = Vec::new();
        for (index, call) in self.calls.iter().enumerate() {
            if call.name.is_empty() {
                continue;
            }
            let call_id = if call.id.is_empty() {
                format!("call-{index}")
            } else {
                call.id.clone()
            };
            let input_val = serde_json::from_str::<Value>(&call.arguments)
                .unwrap_or_else(|_| Value::String(call.arguments.clone()));
            events.push(ProviderEvent::Event(ModelEvent::ToolCall {
                call_id,
                tool_name: call.name.clone(),
                input: ToolCallInput::from_value(input_val),
            }));
        }
        events.push(ProviderEvent::Event(ModelEvent::Completed));
        events
    }

    /// The non-streamed body equivalent to what was streamed, so a recording
    /// replays through the SAME converter the live path used to call.
    #[must_use]
    pub fn assembled_body(&self) -> Value {
        let tool_calls: Vec<Value> = self
            .calls
            .iter()
            .enumerate()
            .filter(|(_, call)| !call.name.is_empty())
            .map(|(index, call)| {
                let call_id = if call.id.is_empty() {
                    format!("call-{index}")
                } else {
                    call.id.clone()
                };
                json!({
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": call.arguments,
                    },
                })
            })
            .collect();
        let mut message = json!({
            "role": "assistant",
            "content": if self.content.is_empty() {
                Value::Null
            } else {
                Value::String(self.content.clone())
            },
        });
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }
        let mut body = json!({
            "id": self.event_id,
            "model": self.model,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": self.finish_reason.clone().unwrap_or_else(|| "stop".to_owned()),
            }],
        });
        if let Some(usage) = &self.usage {
            body["usage"] = usage.clone();
        }
        body
    }
}

#[cfg(test)]
mod tests {
    use super::CompletionStream;
    use siralos_core::provider::{ModelEvent, ProviderEvent};

    fn texts(events: &[ProviderEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|event| match event {
                ProviderEvent::Event(ModelEvent::TextDelta { text }) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn deltas_split_across_reads_are_reassembled() {
        let mut stream = CompletionStream::new(4096);
        // The SAME frame arrives in three reads, mid-JSON.
        assert!(
            stream
                .push_chunk("data: {\"choices\":[{\"delta\":{\"cont")
                .is_empty(),
            "a half frame yields nothing yet"
        );
        // The frame completes: its text is emitted NOW, not at the end.
        let first = stream.push_chunk("ent\":\"Hel\"}}]}\n");
        assert_eq!(texts(&first), vec!["Hel".to_owned()]);
        let events = stream.push_chunk(
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n",
        );
        assert_eq!(texts(&events), vec!["lo".to_owned()]);
        let done = stream.push_chunk("data: [DONE]\n");
        assert!(stream.is_completed());
        assert!(matches!(
            done.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn tool_call_arguments_accumulate_across_frames() {
        let mut stream = CompletionStream::new(4096);
        stream.push_chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-a\",\"function\":{\"name\":\"workspace_read\",\"arguments\":\"{\\\"pa\"}}]}}]}\n");
        stream.push_chunk("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n");
        let events = stream.push_chunk("data: [DONE]\n");
        let call = events
            .iter()
            .find_map(|event| match event {
                ProviderEvent::Event(ModelEvent::ToolCall {
                    call_id,
                    tool_name,
                    input,
                }) => {
                    Some((call_id.clone(), tool_name.clone(), input.clone()))
                }
                _ => None,
            })
            .expect("one tool call");
        assert_eq!(call.0, "call-a");
        assert_eq!(call.1, "workspace_read");
        assert_eq!(call.2.value(), &serde_json::json!({"path": "a"}));
    }

    #[test]
    fn assembled_body_replays_through_the_shared_body_converter() {
        // The recording contract: a streamed turn records a body, and that
        // body must convert to the same events the live stream produced.
        let mut stream = CompletionStream::new(4096);
        let mut live = Vec::new();
        live.extend(stream.push_chunk("data: {\"id\":\"gen-1\",\"model\":\"example/model-a\",\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n"));
        live.extend(stream.push_chunk("data: {\"choices\":[{\"delta\":{\"content\":\" there\"},\"finish_reason\":\"stop\"}]}\n"));
        live.extend(stream.push_chunk("data: [DONE]\n"));
        let body = stream.assembled_body().to_string();
        let replayed =
            crate::provider::replay::completion_events_from_body(&body);
        // The live stream emits per delta; a recorded body merges them into
        // one message. The turn's assistant text is the same either way.
        assert_eq!(texts(&live), vec!["Hi".to_owned(), " there".to_owned()]);
        assert_eq!(texts(&live).join(""), texts(&replayed).join(""));
        assert!(matches!(
            replayed.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn plain_json_body_is_handed_back_instead_of_parsed_as_sse() {
        let mut stream = CompletionStream::new(4096);
        assert!(
            stream
                .push_chunk(
                    "{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}"
                )
                .is_empty()
        );
        assert_eq!(
            stream.plain_body().map(str::trim),
            Some("{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}")
        );
        assert!(!stream.is_completed());
    }

    #[test]
    fn reasoning_is_captured_without_being_emitted_yet() {
        let mut stream = CompletionStream::new(4096);
        let events = stream.push_chunk("data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"weighing options\"}}]}\n");
        assert!(events.is_empty(), "reasoning is not a turn event yet");
        assert_eq!(stream.reasoning(), "weighing options");
    }

    #[test]
    fn the_response_bound_is_enforced_incrementally() {
        let mut stream = CompletionStream::new(32);
        stream.push_chunk("data: {\"choices\":[]}\n");
        let events = stream.push_chunk(&"x".repeat(64));
        assert!(matches!(events.first(), Some(ProviderEvent::Failed(_))));
    }
}
