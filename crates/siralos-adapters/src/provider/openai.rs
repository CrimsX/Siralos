//! OpenAI provider adapter — Host-observed, bounded, replay-recordable (Stage 8, decision 67 C3, 68 §3).
//!
//! The `ModelProvider` seam stays synchronous (`Iterator<Item = ProviderEvent>`)
//! and Host-observed via `siralos_core::determinism::Clock` and
//! `siralos_core::identity` digests for `determinism-replay`. Response
//! recording is implemented via the determinism ports with typed availability.
//! No hidden unbounded retry — the `tool-loop` budget is the only retry.
//!
//! The adapter performs a bounded `reqwest::blocking` POST to
//! `https://api.openai.com/v1/chat/completions` with `Authorization: Bearer`
//! and the `ModelRequest` JSON body (messages/tools/system), 10s connect /
//! 60s read timeouts, and `CancellationSignal` checks before and after the
//! blocking call. Responses are bounded to 1 MiB and sanitized before
//! embedding in `ProviderEvent::Failed` diagnostics. Records never contain the
//! credential or raw body text (only its `sha256`).

use crate::provider::credential::HostCredential;
use crate::provider::{
    CANCELLED_BEFORE_HTTP_CALL, CANCELLED_BEFORE_HTTP_SEND,
    CANCELLED_BEFORE_PROVIDER_START, NO_PROVIDER_RESPONSE_OBSERVED,
    ReplayHooks, record_outcome,
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

/// OpenAI provider — Host-constructed, credential redacted, bounded
/// real-HTTP adapter.
///
/// The `model` is a shared live cell: a session-level `/model` switch
/// replaces it in place, and the NEXT `stream()` clones the cell at call
/// time, so the switched id flows into the request body without
/// re-composing provider/endpoint/credential.
#[derive(Debug)]
pub struct OpenAiProvider {
    /// Redacted credential for openai.
    credential: HostCredential,
    /// Model identifier (bounded, validated at `ProfileRecord` boundary).
    model: Rc<RefCell<String>>,
    /// Replay hooks for determinism recording.
    hooks: ReplayHooks,
    /// Last replay availability, set on each terminal outcome.
    last_replay: RefCell<ProviderReplayAvailability>,
}

impl OpenAiProvider {
    /// Create a new `OpenAiProvider` with a redacted `HostCredential` and a
    /// bounded `model` id. The credential is held in memory only for the
    /// `ModelProvider` call and never written to `siralos.toml`/`siralos.lock`.
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
                message: CANCELLED_BEFORE_PROVIDER_START.to_owned(),
            }));
        }
        let model = self.model.borrow().clone();
        let credential =
            String::from_utf8_lossy(self.credential.as_bytes()).to_string();
        let request = request.clone();
        let events = Self::call_openai(
            &model,
            CHAT_BASE_URL,
            &credential,
            &request,
            cancellation,
            &self.hooks,
            &self.last_replay,
        );
        Box::new(events.into_iter())
    }
}

/// The OpenAI chat-completions base URL.
///
/// Production is this client's only supplier of a base URL: `stream` passes
/// this constant, and [`chat_completions_url`] appends the path, so the URL the
/// wire sees is unchanged. The parameter exists so an offline probe can point
/// the real call path at a loopback fixture server.
const CHAT_BASE_URL: &str = "https://api.openai.com/v1";

/// The chat POST URL for `base_url`; trailing slashes are tolerated so a seam
/// caller cannot produce a doubled separator.
fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

impl OpenAiProvider {
    fn call_openai(
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
                    "openai client build failed: {err}"
                ))];
                record_outcome(hooks, last_replay, "openai", model, None, "");
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
        let mut body =
            serde_json::json!({"model": model, "messages": messages});
        if !tools_json.is_empty() {
            body["tools"] = Value::Array(tools_json);
        }
        if cancellation.is_cancelled() {
            return vec![ProviderEvent::Cancelled {
                message: CANCELLED_BEFORE_HTTP_SEND.to_owned(),
            }];
        }
        let pipeline = crate::provider::run_chat_pipeline(
            "openai",
            model,
            client
                .post(chat_completions_url(base_url))
                .header("Authorization", format!("Bearer {credential}"))
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
        events.push(ProviderEvent::Event(ModelEvent::Completed));
        record_outcome(
            hooks,
            last_replay,
            "openai",
            model,
            Some(status.as_u16()),
            &text,
        );
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
    use super::{
        CHAT_BASE_URL, HostCredential, OpenAiProvider, chat_completions_url,
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

    /// Drive the real `call_openai` path at `base_url` with recorder hooks.
    fn drive_with(
        base_url: &str,
        cancellation: CancellationSignal<'_>,
        hooks: &ReplayHooks,
    ) -> Vec<ProviderEvent> {
        OpenAiProvider::call_openai(
            "gpt-4o",
            base_url,
            "test-cred",
            &probe_request(),
            cancellation,
            hooks,
            &probe_replay(),
        )
    }

    /// Drive the real `call_openai` path at `base_url`.
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
    fn production_chat_url_is_the_historical_endpoint() {
        // The seam takes a base URL; production supplies the constant, and the
        // composed URL must be the one this client has always posted to. A
        // wrong constant or a wrong path fails here.
        assert_eq!(
            chat_completions_url(CHAT_BASE_URL),
            "https://api.openai.com/v1/chat/completions"
        );
        // Trailing slashes cannot produce a doubled separator.
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn probe_connect_refused_is_one_bounded_failed_event() {
        // Loopback port 1 refuses immediately: hermetic, no live network.
        let events =
            drive("http://127.0.0.1:1", CancellationToken::new().signal());
        let message = failed_message(&events);
        assert!(message.starts_with("openai request failed: "), "{message}");
        // Each client carries its own prefix for the shared transport failure;
        // recorded here so a future unification cannot quietly drop one.
        for other in
            ["anthropic request failed: ", "probe-vendor request failed: "]
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
            body: r#"{"choices":[{"message":{"content":"hello"}}]}"#
                .to_owned(),
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
        // a bare loopback base yields `/chat/completions`; production's base
        // carries the `/v1` and composes the historical URL.
        assert_eq!(recorded.request_line, "POST /chat/completions HTTP/1.1");
        assert_eq!(recorded.header("authorization"), Some("Bearer test-cred"));
        let body: serde_json::Value =
            serde_json::from_str(&recorded.body).expect("json request body");
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(events.iter().any(|event| matches!(
            event,
            ProviderEvent::Event(ModelEvent::TextDelta { text }) if text == "hello"
        )));
    }

    #[test]
    fn probe_records_the_success_event_sequence() {
        let server = serve(Fixture {
            status: 200,
            body: r#"{"choices":[{"message":{"content":"hello","tool_calls":[{"id":"c1","type":"function","function":{"name":"workspace_read","arguments":"{}"}}]}}]}"#
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
                    message.starts_with(&format!("openai error {status} ")),
                    "{message}"
                );
                assert!(
                    message.contains(&format!("{status} {}", reason(status))),
                    "{message}"
                );
                // The whole body never reaches the event: the snippet is 512
                // characters, so a 10 KB body cannot inflate the diagnostic.
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
    fn generic_provider_named_openai_fails_closed_on_an_unreachable_endpoint()
    {
        // Host-observed, bounded, no live network in `cargo test` — the
        // `openai` endpoint is not hit; the test verifies the `Failed`
        // path via the `GenericProvider` with an unreachable loopback
        // endpoint, which is hermetic and fast.
        let cred = HostCredential::from_bytes_for_test(b"sk-test".to_vec());
        let provider = crate::provider::generic::GenericProvider::new(
            "openai".to_owned(),
            "gpt-4o".to_owned(),
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
        let ok_body = r#"{"choices":[{"message":{"content":"hi"}}]}"#;
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
            message.starts_with("openai response JSON parse failed: "),
            "{message}"
        );
        assert!(
            !message.contains("openai error"),
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
        let server = serve_truncated(4096, "{\"choices\":[{\"message\"");
        let events =
            drive(&server.base_url, CancellationToken::new().signal());
        let _ = server.recorded();
        let message = failed_message(&events);
        assert!(
            message.starts_with("openai response read failed: "),
            "{message}"
        );
    }
}
