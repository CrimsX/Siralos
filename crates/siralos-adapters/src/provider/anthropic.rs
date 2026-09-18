//! Anthropic provider adapter — Host-observed, bounded, replay-recordable (Stage 8, decision 67 C3, 68 §3).
//!
//! Mirrors `openai.rs`: the `ModelProvider` seam stays synchronous and
//! Host-observed, with a bounded `reqwest::blocking` POST to
//! `https://api.anthropic.com/v1/messages` (`x-api-key` + `anthropic-version`),
//! 10s connect / 60s read, `CancellationSignal` checks, 1 MiB bound and
//! sanitized diagnostics. Response recording is implemented via the determinism
//! ports with typed availability. Records never contain the credential or raw
//! body text (only its `sha256`).

use crate::provider::credential::HostCredential;
use crate::provider::{
    ANTHROPIC_VERSION, CANCELLED_BEFORE_HTTP_CALL, CANCELLED_BEFORE_HTTP_SEND,
    NO_PROVIDER_RESPONSE_OBSERVED, ReplayHooks, record_outcome,
};
use serde_json::Value;
use siralos_core::determinism::{
    Clock, ProviderReplayAvailability, ReplayRecorder,
};
use siralos_core::provider::{
    CancellationSignal, ModelEvent, ModelProvider, ModelRequest, ProviderEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Anthropic provider — Host-constructed, credential redacted, bounded
/// real-HTTP adapter.
///
/// The `model` is a shared live cell: a session-level `/model` switch
/// replaces it in place, and the NEXT `stream()` clones the cell at call
/// time, so the switched id flows into the request body without
/// re-composing provider/endpoint/credential.
#[derive(Debug)]
pub struct AnthropicProvider {
    /// Redacted credential for anthropic.
    credential: HostCredential,
    /// Model identifier (bounded, validated at `ProfileRecord` boundary).
    model: Rc<RefCell<String>>,
    /// Replay hooks for determinism recording.
    hooks: ReplayHooks,
    /// Last replay availability, set on each terminal outcome.
    last_replay: RefCell<ProviderReplayAvailability>,
}

impl AnthropicProvider {
    /// Create a new `AnthropicProvider` with a redacted `HostCredential` and a
    /// bounded `model` id.
    pub fn new(credential: HostCredential, model: String) -> Self {
        Self {
            credential,
            model: Rc::new(RefCell::new(model)),
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
        self.hooks =
            ReplayHooks { clock: Some(clock), recorder: Some(recorder) };
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
        let model = self.model.borrow().clone();
        let credential =
            String::from_utf8_lossy(self.credential.as_bytes()).to_string();
        let request = request.clone();
        let events = Self::call_anthropic(
            &model,
            MESSAGES_BASE_URL,
            &credential,
            &request,
            cancellation,
            &self.hooks,
            &self.last_replay,
        );
        Box::new(events.into_iter())
    }
}

/// The Anthropic messages base URL.
///
/// Production is this client's only supplier of a base URL: `stream` passes
/// this constant, and [`messages_url`] appends the path, so the URL the wire
/// sees is unchanged. The parameter exists so an offline probe can point the
/// real call path at a loopback fixture server.
const MESSAGES_BASE_URL: &str = "https://api.anthropic.com/v1";

/// The messages POST URL for `base_url`; trailing slashes are tolerated so a
/// seam caller cannot produce a doubled separator.
fn messages_url(base_url: &str) -> String {
    format!("{}/messages", base_url.trim_end_matches('/'))
}

impl AnthropicProvider {
    fn call_anthropic(
        model: &str,
        base_url: &str,
        credential: &str,
        request: &ModelRequest,
        cancellation: CancellationSignal<'_>,
        hooks: &ReplayHooks,
        last_replay: &RefCell<ProviderReplayAvailability>,
    ) -> Vec<ProviderEvent> {
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: CANCELLED_BEFORE_HTTP_CALL.to_owned(),
            }];
        }
        let client = match crate::provider::build_http_client() {
            Ok(client) => client,
            Err(err) => {
                let events = vec![ProviderEvent::Failed(format!(
                    "anthropic client build failed: {err}"
                ))];
                record_outcome(
                    hooks,
                    last_replay,
                    "anthropic",
                    model,
                    None,
                    "",
                );
                return events;
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
                message: CANCELLED_BEFORE_HTTP_SEND.to_owned(),
            }];
        }
        let pipeline = crate::provider::run_chat_pipeline(
            "anthropic",
            model,
            client
                .post(messages_url(base_url))
                .header("x-api-key", credential)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("Content-Type", "application/json")
                .json(&body),
            cancellation,
            hooks,
            last_replay,
        );
        let (status, text, value) = match pipeline {
            crate::provider::ChatPipelineOutcome::Events(events) => {
                return events;
            }
            crate::provider::ChatPipelineOutcome::Parsed {
                status,
                text,
                value,
            } => (status, text, value),
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
        record_outcome(
            hooks,
            last_replay,
            "anthropic",
            model,
            Some(status.as_u16()),
            &text,
        );
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
    use super::{
        AnthropicProvider, HostCredential, MESSAGES_BASE_URL, messages_url,
    };
    use crate::provider::ReplayHooks;
    use crate::provider::probe::{
        ERROR_STATUSES, Fixture, error_bodies, reason, retaining_hooks, serve,
        serve_truncated,
    };
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

    /// Drive the real `call_anthropic` path at `base_url` with recorder hooks.
    fn drive_with(
        base_url: &str,
        cancellation: CancellationSignal<'_>,
        hooks: &ReplayHooks,
    ) -> Vec<ProviderEvent> {
        AnthropicProvider::call_anthropic(
            "claude-3-5-sonnet",
            base_url,
            "test-cred",
            &probe_request(),
            cancellation,
            hooks,
            &probe_replay(),
        )
    }

    /// Drive the real `call_anthropic` path at `base_url`.
    fn drive(
        base_url: &str,
        cancellation: CancellationSignal<'_>,
    ) -> Vec<ProviderEvent> {
        drive_with(base_url, cancellation, &ReplayHooks::default())
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
    fn production_messages_url_is_the_historical_endpoint() {
        // The seam takes a base URL; production supplies the constant, and the
        // composed URL must be the one this client has always posted to. A
        // wrong constant or a wrong path fails here.
        assert_eq!(
            messages_url(MESSAGES_BASE_URL),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            messages_url("https://api.anthropic.com/v1/"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn probe_connect_refused_is_one_bounded_failed_event() {
        let events =
            drive("http://127.0.0.1:1", CancellationToken::new().signal());
        let message = failed_message(&events);
        assert!(
            message.starts_with("anthropic request failed: "),
            "{message}"
        );
        // Each client carries its own prefix for the shared transport failure;
        // recorded here so a future unification cannot quietly drop one.
        for other in
            ["openai request failed: ", "probe-vendor request failed: "]
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
        let events = drive("http://127.0.0.1:1", token.signal());
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
        let events =
            drive(&server.base_url, CancellationToken::new().signal());
        // `recorded` panics when nothing reached 127.0.0.1 within its deadline:
        // a probe that contacted a real endpoint fails loudly here.
        let recorded = server.recorded();
        // Recorded: the seam appends the path to whatever base it is given, so
        // a bare loopback base yields `/messages`; production's base carries
        // the `/v1` and composes the historical URL.
        assert_eq!(recorded.request_line, "POST /messages HTTP/1.1");
        assert_eq!(recorded.header("x-api-key"), Some("test-cred"));
        // Recorded: this client hardcodes the dated API version.
        assert_eq!(recorded.header("anthropic-version"), Some("2023-06-01"));
        let body: serde_json::Value =
            serde_json::from_str(&recorded.body).expect("json request body");
        assert_eq!(body["model"], "claude-3-5-sonnet");
        assert!(events.iter().any(|event| matches!(
            event,
            ProviderEvent::Event(ModelEvent::TextDelta { text }) if text == "hello"
        )));
    }

    #[test]
    fn probe_records_the_success_event_sequence() {
        let server = serve(Fixture {
            status: 200,
            body: r#"{"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"t1","name":"workspace_read","input":{}}]}"#
                .to_owned(),
        });
        let events =
            drive(&server.base_url, CancellationToken::new().signal());
        let _ = server.recorded();
        // Recorded baseline, not an approved-parity claim: this is the sequence
        // this client produced today for this body.
        assert!(events.iter().any(|event| matches!(
            event,
            ProviderEvent::Event(ModelEvent::ToolCall { .. })
        )));
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Event(ModelEvent::Completed))
        ));
    }

    #[test]
    fn probe_records_the_text_field_of_a_tool_use_block_only_when_it_is_first()
    {
        // Recorded baseline, not an approved-parity claim. Text on an ordinary
        // block is collected wherever it sits, but a `text` field carried on a
        // `tool_use` block survives only when that block is first: the first
        // block is read without checking its type, while the later-block pass
        // takes `tool_use` in preference to `text`.
        let calls = |body: &str| {
            let server = serve(Fixture { status: 200, body: body.to_owned() });
            let events =
                drive(&server.base_url, CancellationToken::new().signal());
            let _ = server.recorded();
            events
        };
        let first = calls(
            r#"{"content":[{"type":"tool_use","id":"t1","name":"n","input":{},"text":"first-block-text"}]}"#,
        );
        assert!(first.iter().any(|event| matches!(
            event,
            ProviderEvent::Event(ModelEvent::TextDelta { text })
                if text == "first-block-text"
        )));

        let later = calls(
            r#"{"content":[{"type":"text","text":"leading"},{"type":"tool_use","id":"t1","name":"n","input":{},"text":"later-block-text"}]}"#,
        );
        assert!(!later.iter().any(|event| matches!(
            event,
            ProviderEvent::Event(ModelEvent::TextDelta { text })
                if text == "later-block-text"
        )));

        // Non-first blocks are otherwise fully collected: three text blocks all
        // arrive, so "text is first-block-only" would be the wrong reading.
        let texts = calls(
            r#"{"content":[{"type":"text","text":"one"},{"type":"text","text":"two"},{"type":"text","text":"three"}]}"#,
        );
        for expected in ["one", "two", "three"] {
            assert!(texts.iter().any(|event| matches!(
                event,
                ProviderEvent::Event(ModelEvent::TextDelta { text })
                    if text == expected
            )));
        }
    }

    #[test]
    fn probe_records_the_http_error_matrix() {
        // Recorded baseline, not an approved-parity claim: these assertions
        // describe what this client does today at each (status, body) pair.
        for status in ERROR_STATUSES {
            for (label, body) in error_bodies() {
                let server = serve(Fixture { status, body: body.clone() });
                let events =
                    drive(&server.base_url, CancellationToken::new().signal());
                let _ = server.recorded();
                let message = failed_message(&events);
                // Recorded: the message embeds `reqwest`'s full status line
                // (`400 Bad Request`), not the bare numeric code, which is what
                // the generic path prints.
                assert!(
                    message.starts_with(&format!("anthropic error {status} ")),
                    "{message}"
                );
                assert!(
                    message.contains(&format!("{status} {}", reason(status))),
                    "{message}"
                );
                assert!(
                    message.len() <= 512 + 64,
                    "status {status}: {} bytes",
                    message.len()
                );
                if label.contains("html") {
                    // Recorded: this client keeps the raw HTML snippet, however
                    // large the body is — the 512-character cut lands after the
                    // markup in both HTML fixtures.
                    assert!(message.contains("<html>"), "{message}");
                }
                // Recorded: this client appends no rate-limit hint, unlike the
                // generic path's `http_error_message`.
                assert!(!message.contains("rate limiting"), "{message}");
            }
        }
    }

    #[test]
    fn probe_records_what_replay_records_at_each_outcome() {
        // Recorded baseline, not an approved-parity claim. `record_outcome`'s
        // arguments are invisible in every `ProviderEvent`, so they are pinned
        // here: a later extraction could hand the recorder the 512-character
        // message snippet instead of the whole bounded body, and every visible
        // assertion would still pass.
        let (hooks, recorder) = retaining_hooks();

        // Pre-response failure: no status, empty body.
        let _ = drive_with(
            "http://127.0.0.1:1",
            CancellationToken::new().signal(),
            &hooks,
        );

        // HTTP error: the whole bounded body reaches the recorder, while the
        // event message stays at the 512-character snippet.
        let long_body = format!("{}tail", "e".repeat(700));
        let error = serve(Fixture { status: 404, body: long_body.clone() });
        let events = drive_with(
            &error.base_url,
            CancellationToken::new().signal(),
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
        let _ = drive_with(
            &raw.base_url,
            CancellationToken::new().signal(),
            &hooks,
        );
        let _ = raw.recorded();

        // Success: the status and the body of the response that parsed.
        let ok_body = r#"{"content":[{"type":"text","text":"hi"}]}"#;
        let ok = serve(Fixture { status: 200, body: ok_body.to_owned() });
        let _ = drive_with(
            &ok.base_url,
            CancellationToken::new().signal(),
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
        let events = drive_with(
            &server.base_url,
            CancellationToken::new().signal(),
            &hooks,
        );
        let _ = server.recorded();
        let message = failed_message(&events);
        assert!(
            message.starts_with("anthropic response JSON parse failed: "),
            "{message}"
        );
        assert!(
            !message.contains("anthropic error"),
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
            drive(&server.base_url, CancellationToken::new().signal());
        let _ = server.recorded();
        let message = failed_message(&events);
        assert!(
            message.starts_with("anthropic response read failed: "),
            "{message}"
        );
    }

    #[test]
    fn anthropic_id_is_stable() {
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider =
            AnthropicProvider::new(cred, "claude-3-5-sonnet".to_owned());
        assert_eq!(provider.id(), "anthropic");
    }

    #[test]
    fn generic_provider_named_anthropic_fails_closed_on_an_unreachable_endpoint()
     {
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
