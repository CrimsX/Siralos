//! Provider-turn subjects: turn/tool-call/detach execution and their input validation.
use crate::harness::*;

/// Canonical provider-turn record: one canonical observation per input
/// case (turn and/or detach).
pub(in crate::harness) fn provider_turn_record(
    input: &Value,
) -> Result<Value, HarnessError> {
    let mut records = Vec::new();
    for case in scenario_array(input, "cases")? {
        let record = match (case.get("turn"), case.get("detach")) {
            (Some(turn), None) => {
                json!({ "turn": run_provider_turn_case(turn)? })
            }
            (None, Some(detach)) => {
                json!({ "detach": run_provider_detach_case(detach)? })
            }
            _ => {
                return Err(HarnessError::corpus(
                    "provider-turn case must have exactly one of turn/detach",
                ));
            }
        };
        records.push(record);
    }
    Ok(json!({ "cases": records }))
}

/// Run one provider-turn case through the production core collector.
fn run_provider_turn_case(case: &Value) -> Result<Value, HarnessError> {
    let case = materialize_value(case)?;
    let provider_spec = case.get("provider").ok_or_else(|| {
        HarnessError::corpus("provider-turn turn case requires a provider")
    })?;
    let kind = scenario_string(provider_spec, "kind")?;
    let messages =
        parse_conversation_items(scenario_array(&case, "messages")?)?;
    let tools = parse_tool_definitions(scenario_array(&case, "tools")?)?;
    // The provider request system is Host-projection-owned (R7.3); the
    // R7.1 differential input does not carry it.
    let system = None;
    let cancel_after =
        scenario_u64(&case, "cancelAfterEvents").map(|value| value as usize);
    let token = CancellationToken::new();
    let outcome = match kind.as_str() {
        "fake" => {
            let provider = DeterministicFakeProvider::new();
            collect_with_cancellation_script(
                &provider,
                &messages,
                &tools,
                system,
                &token,
                cancel_after,
            )
        }
        "scripted" => {
            let events = scenario_array(provider_spec, "events")?;
            let provider = ScriptedProvider { events: events.clone() };
            collect_with_cancellation_script(
                &provider,
                &messages,
                &tools,
                system,
                &token,
                cancel_after,
            )
        }
        _ => {
            return Err(HarnessError::corpus(
                "provider-turn provider kind must be fake or scripted",
            ));
        }
    };
    Ok(provider_turn_value(outcome))
}

/// Run one tool-result detach case through the production core boundary.
fn run_provider_detach_case(detach: &Value) -> Result<Value, HarnessError> {
    let detach = materialize_value(detach)?;
    let value = detach.get("value").ok_or_else(|| {
        HarnessError::corpus("provider-turn detach case requires a value")
    })?;
    let max_bytes =
        detach.get("maxBytes").and_then(Value::as_u64).ok_or_else(|| {
            HarnessError::corpus(
                "provider-turn detach case requires an integer maxBytes",
            )
        })? as usize;
    let actor = scenario_string(&detach, "actor")?;
    match detach_bounded_tool_result(value, max_bytes) {
        Ok((result, byte_length)) => Ok(json!({
            "ok": true,
            "result": tool_execution_result_value(&result),
            "byteLength": byte_length,
        })),
        Err(failure) => {
            Ok(json!({ "ok": false, "message": failure.message(&actor) }))
        }
    }
}

/// The canonical turn observation for one outcome.
fn provider_turn_value(outcome: TurnOutcome) -> Value {
    match outcome {
        TurnOutcome::Cancelled => json!({ "kind": "cancelled" }),
        TurnOutcome::Failed { failure } => json!({
            "kind": "failed",
            "failure": provider_failure_code(&failure),
            "message": failure.application_message(),
        }),
        TurnOutcome::Turn { assistant_text, text_deltas, tool_calls } => {
            json!({
                "kind": "turn",
                "assistantText": assistant_text,
                "textDeltas": text_deltas,
                "toolCalls": tool_calls
                    .iter()
                    .map(tool_call_value)
                    .collect::<Vec<_>>(),
            })
        }
    }
}

/// Stable failure category for a typed turn failure.
fn provider_failure_code(failure: &TurnFailure) -> &'static str {
    match failure {
        TurnFailure::LimitExceeded(LimitClass::AssistantTextBytes) => {
            "LIMIT_ASSISTANT_TEXT_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::TextEventCount) => {
            "LIMIT_TEXT_EVENT_COUNT"
        }
        TurnFailure::LimitExceeded(LimitClass::ToolCallCount) => {
            "LIMIT_TOOL_CALL_COUNT"
        }
        TurnFailure::LimitExceeded(LimitClass::CallIdBytes) => {
            "LIMIT_CALL_ID_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::ToolNameBytes) => {
            "LIMIT_TOOL_NAME_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::ToolArgumentBytes) => {
            "LIMIT_TOOL_ARGUMENT_BYTES"
        }
        TurnFailure::LimitExceeded(LimitClass::AggregateTurnBytes) => {
            "LIMIT_AGGREGATE_TURN_BYTES"
        }
        TurnFailure::EventAfterCompletion => "EVENT_AFTER_COMPLETION",
        TurnFailure::EofWithoutCompletion => "EOF_WITHOUT_COMPLETION",
        TurnFailure::Protocol(ProtocolFailure::UnknownEventType) => {
            "UNKNOWN_EVENT_TYPE"
        }
        TurnFailure::Protocol(ProtocolFailure::MalformedEvent) => {
            "MALFORMED_EVENT"
        }
        TurnFailure::Protocol(ProtocolFailure::MalformedTextEvent) => {
            "MALFORMED_TEXT_EVENT"
        }
        TurnFailure::Protocol(ProtocolFailure::MalformedToolCall) => {
            "MALFORMED_TOOL_CALL"
        }
        TurnFailure::Protocol(ProtocolFailure::InvalidToolArgumentJson) => {
            "INVALID_TOOL_ARGUMENT_JSON"
        }
        TurnFailure::ProviderFailed(_) => "PROVIDER_FAILED",
        TurnFailure::InvalidTranscript(_) => "INVALID_TRANSCRIPT",
    }
}

/// The canonical tool-call proposal observation.
fn tool_call_value(call: &TurnToolCall) -> Value {
    match call {
        TurnToolCall::Execute { call_id, tool_name, input } => json!({
            "kind": "execute",
            "callId": call_id,
            "toolName": tool_name,
            "input": input.value(),
        }),
        TurnToolCall::Invalid { call_id, tool_name, message } => json!({
            "kind": "invalid",
            "callId": call_id,
            "toolName": tool_name,
            "message": message,
        }),
    }
}

/// The canonical typed tool-result value.
pub(in crate::harness) fn tool_execution_result_value(
    result: &ToolExecutionResult,
) -> Value {
    match result {
        ToolExecutionResult::Success { output, summary } => json!({
            "status": "success",
            "output": output,
            "summary": summary,
        }),
        other => json!({
            "status": other.status_str(),
            "message": other.message(),
        }),
    }
}

/// Collect one turn, optionally wrapping the provider stream with a
/// deterministic host-scripted cancellation point.
fn collect_with_cancellation_script<P: ModelProvider>(
    provider: &P,
    messages: &[ConversationItem],
    tools: &[ToolDefinition],
    system: Option<String>,
    token: &CancellationToken,
    cancel_after: Option<usize>,
) -> TurnOutcome {
    match cancel_after {
        Some(cancel_after) => {
            // The wrapper holds the Host controller: the cancellation
            // point is Host/test-harness authority. The wrapped provider
            // receives only the read-only observation signal.
            let provider = CancelAfterProvider {
                inner: provider,
                controller: token,
                cancel_after,
            };
            collect_provider_turn(&provider, messages, tools, system, token)
        }
        None => {
            collect_provider_turn(provider, messages, tools, system, token)
        }
    }
}

/// Harness-local host cancellation wrapper: cancels the Host controller
/// after exactly 'cancel_after' events have been emitted (0 = before the
/// first event). The production collector and controller decide the
/// outcome; the wrapped provider receives only the read-only
/// 'CancellationSignal' observation view.
struct CancelAfterProvider<'a, P: ModelProvider> {
    inner: &'a P,
    controller: &'a CancellationToken,
    cancel_after: usize,
}

impl<P: ModelProvider> ModelProvider for CancelAfterProvider<'_, P> {
    type Stream<'a>
        = CancelAfterStream<'a, P::Stream<'a>>
    where
        Self: 'a;

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        CancelAfterStream {
            inner: self.inner.stream(request, cancellation),
            controller: self.controller,
            emitted: 0,
            cancel_after: self.cancel_after,
        }
    }
}

struct CancelAfterStream<'a, S> {
    inner: S,
    controller: &'a CancellationToken,
    emitted: usize,
    cancel_after: usize,
}

impl<S: Iterator<Item = ProviderEvent>> Iterator for CancelAfterStream<'_, S> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        if self.emitted == self.cancel_after {
            self.controller.cancel();
            return None;
        }
        let event = self.inner.next()?;
        self.emitted += 1;
        Some(event)
    }
}

/// Harness-local scripted provider: yields untrusted raw events that the
/// production collector validates through the real trust boundary.
struct ScriptedProvider {
    events: Vec<Value>,
}

impl ModelProvider for ScriptedProvider {
    type Stream<'a>
        = ScriptedStream<'a>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "scripted-provider"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        ScriptedStream { events: self.events.iter() }
    }
}

struct ScriptedStream<'a> {
    events: std::slice::Iter<'a, Value>,
}

impl Iterator for ScriptedStream<'_> {
    type Item = ProviderEvent;

    fn next(&mut self) -> Option<ProviderEvent> {
        self.events.next().map(|raw| ProviderEvent::Raw(raw.clone()))
    }
}

/// Materialize deterministic '$repeat' fixture markers into strings
/// (mirroring the oracle probe), recursively. A marker object
/// {"$repeat": {"character": <one scalar>, "count": N}} becomes the
/// character repeated N times.
pub(in crate::harness) fn materialize_value(
    value: &Value,
) -> Result<Value, HarnessError> {
    if let Some(object) = value.as_object() {
        if let Some(repeat) = object.get("$eventsRepeat") {
            let repeat_object = repeat.as_object().ok_or_else(|| {
                HarnessError::corpus("$eventsRepeat marker must be an object")
            })?;
            let events = repeat_object
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$eventsRepeat marker requires an events array",
                    )
                })?;
            let count = repeat_object
                .get("count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$eventsRepeat marker requires an integer count",
                    )
                })? as usize;
            if count > 4096 || events.len().saturating_mul(count) > 4096 {
                return Err(HarnessError::corpus(
                    "$eventsRepeat expands beyond the 4096-event bound",
                ));
            }
            let materialized = events
                .iter()
                .map(materialize_value)
                .collect::<Result<Vec<_>, _>>()?;
            let mut out = Vec::with_capacity(materialized.len() * count);
            for _ in 0..count {
                out.extend(materialized.iter().cloned());
            }
            return Ok(Value::Array(out));
        }
        if let Some(repeat) = object.get("$repeat") {
            let repeat_object = repeat.as_object().ok_or_else(|| {
                HarnessError::corpus("$repeat marker must be an object")
            })?;
            let character = repeat_object
                .get("character")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$repeat marker requires a string character",
                    )
                })?;
            if character.chars().count() != 1 {
                return Err(HarnessError::corpus(
                    "$repeat character must be a single Unicode scalar value",
                ));
            }
            let count = repeat_object
                .get("count")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "$repeat marker requires an integer count",
                    )
                })? as usize;
            if count > 1_048_576 {
                return Err(HarnessError::corpus(
                    "$repeat count exceeds the materialization bound",
                ));
            }
            return Ok(Value::String(character.repeat(count)));
        }
        let mut out = serde_json::Map::new();
        for (key, child) in object {
            out.insert(key.clone(), materialize_value(child)?);
        }
        return Ok(Value::Object(out));
    }
    if let Some(array) = value.as_array() {
        let entries = array
            .iter()
            .map(materialize_value)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Value::Array(entries));
    }
    Ok(value.clone())
}

/// Parse conversation items from validated fixture JSON.
pub(in crate::harness) fn parse_conversation_items(
    items: &[Value],
) -> Result<Vec<ConversationItem>, HarnessError> {
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        let item = match scenario_string(item, "type")?.as_str() {
            "user_message" => ConversationItem::UserMessage {
                content: scenario_string(item, "content")?,
            },
            "assistant_message" => ConversationItem::AssistantMessage {
                content: scenario_string(item, "content")?,
            },
            "assistant_tool_call" => ConversationItem::AssistantToolCall {
                call_id: scenario_string(item, "callId")?,
                tool_name: scenario_string(item, "toolName")?,
                input: AssistantToolCallInput::Present(
                    item.get("input").cloned().ok_or_else(|| {
                        HarnessError::corpus(
                            "assistant_tool_call item requires an input",
                        )
                    })?,
                ),
            },
            "tool_result" => ConversationItem::ToolResult {
                call_id: scenario_string(item, "callId")?,
                tool_name: scenario_string(item, "toolName")?,
                result: parse_tool_result(item.get("result").ok_or_else(
                    || {
                        HarnessError::corpus(
                            "tool_result item requires a result",
                        )
                    },
                )?)?,
            },
            other => {
                return Err(HarnessError::corpus(format!(
                    "provider-turn conversation item has an unknown type {other}"
                )));
            }
        };
        parsed.push(item);
    }
    Ok(parsed)
}

/// Parse a typed tool-result value from fixture JSON.
fn parse_tool_result(
    value: &Value,
) -> Result<ToolExecutionResult, HarnessError> {
    let object = value.as_object().ok_or_else(|| {
        HarnessError::corpus("tool result must be an object")
    })?;
    let status =
        object.get("status").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("tool result requires a string status")
        })?;
    let string_field = |key: &str| {
        object.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(
            || {
                HarnessError::corpus(format!(
                    "tool result requires a string {key}"
                ))
            },
        )
    };
    let result = match status {
        "success" => ToolExecutionResult::Success {
            output: object.get("output").cloned().ok_or_else(|| {
                HarnessError::corpus("success tool result requires an output")
            })?,
            summary: string_field("summary")?,
        },
        "invalid_input" => ToolExecutionResult::InvalidInput {
            message: string_field("message")?,
        },
        "denied" => {
            ToolExecutionResult::Denied { message: string_field("message")? }
        }
        "conflict" => {
            ToolExecutionResult::Conflict { message: string_field("message")? }
        }
        "failed" => {
            ToolExecutionResult::Failed { message: string_field("message")? }
        }
        "cancelled" => ToolExecutionResult::Cancelled {
            message: string_field("message")?,
        },
        "timed_out" => {
            ToolExecutionResult::TimedOut { message: string_field("message")? }
        }
        "output_limit" => ToolExecutionResult::OutputLimit {
            message: string_field("message")?,
        },
        "sandbox_denied" => ToolExecutionResult::SandboxDenied {
            message: string_field("message")?,
        },
        "sandbox_unavailable" => ToolExecutionResult::SandboxUnavailable {
            message: string_field("message")?,
        },
        "workspace_violation" => ToolExecutionResult::WorkspaceViolation {
            message: string_field("message")?,
        },
        "unavailable" => ToolExecutionResult::Unavailable {
            message: string_field("message")?,
        },
        other => {
            return Err(HarnessError::corpus(format!(
                "tool result has an unknown status {other}"
            )));
        }
    };
    Ok(result)
}

/// Parse tool definitions from validated fixture JSON.
fn parse_tool_definitions(
    tools: &[Value],
) -> Result<Vec<ToolDefinition>, HarnessError> {
    let mut parsed = Vec::with_capacity(tools.len());
    for tool in tools {
        parsed.push(ToolDefinition {
            name: scenario_string(tool, "name")?,
            description: scenario_string(tool, "description")?,
            input_schema: tool.get("inputSchema").cloned().ok_or_else(
                || {
                    HarnessError::corpus(
                        "tool definition requires an inputSchema",
                    )
                },
            )?,
        });
    }
    Ok(parsed)
}

/// Whether a value is a valid '$repeat' materialization marker.
pub(in crate::harness) fn is_repeat_marker(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(repeat) = object.get("$repeat") else {
        return false;
    };
    let Some(repeat_object) = repeat.as_object() else {
        return false;
    };
    match repeat_object.get("character") {
        Some(Value::String(character)) if character.chars().count() == 1 => {
            repeat_object
                .get("count")
                .and_then(Value::as_u64)
                .is_some_and(|count| count <= 1_048_576)
        }
        _ => false,
    }
}

/// Strict provider-turn input shape validation (mirrors contract.mjs).
pub(in crate::harness) fn validate_provider_turn_input(
    input: &Value,
) -> Result<(), HarnessError> {
    let input_object = input.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn input must be an object")
    })?;
    if input_object.len() != 1 || !input_object.contains_key("cases") {
        return Err(HarnessError::corpus(
            "provider-turn input must contain exactly the cases field",
        ));
    }
    let cases =
        input_object.get("cases").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("provider-turn cases must be an array"),
        )?;
    if cases.is_empty() || cases.len() > 32 {
        return Err(HarnessError::corpus(
            "provider-turn cases must contain 1-32 entries",
        ));
    }
    for case in cases {
        validate_provider_turn_case(case)?;
    }
    Ok(())
}

fn validate_provider_turn_case(case: &Value) -> Result<(), HarnessError> {
    let case_object = case.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn cases must contain objects")
    })?;
    let has_turn = case_object.contains_key("turn");
    let has_detach = case_object.contains_key("detach");
    if has_turn == has_detach {
        return Err(HarnessError::corpus(
            "provider-turn case must have exactly one of turn/detach",
        ));
    }
    if has_turn {
        validate_provider_turn_case_input(
            case_object.get("turn").ok_or_else(|| {
                HarnessError::corpus("provider-turn case turn is missing")
            })?,
        )?;
    } else {
        validate_provider_detach_case_input(
            case_object.get("detach").ok_or_else(|| {
                HarnessError::corpus("provider-turn case detach is missing")
            })?,
        )?;
    }
    Ok(())
}

fn validate_provider_turn_case_input(
    case: &Value,
) -> Result<(), HarnessError> {
    let case_object = case.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn turn case must be an object")
    })?;
    for key in case_object.keys() {
        if !matches!(
            key.as_str(),
            "provider" | "messages" | "tools" | "cancelAfterEvents"
        ) {
            return Err(HarnessError::corpus(format!(
                "provider-turn turn case has an unknown field {key}"
            )));
        }
    }
    for required in ["provider", "messages", "tools"] {
        if !case_object.contains_key(required) {
            return Err(HarnessError::corpus(format!(
                "provider-turn turn case requires the {required} field"
            )));
        }
    }
    if let Some(cancel) = case_object.get("cancelAfterEvents") {
        let value = cancel.as_u64().ok_or_else(|| {
            HarnessError::corpus(
                "provider-turn cancelAfterEvents must be an integer",
            )
        })?;
        if value > 1024 {
            return Err(HarnessError::corpus(
                "provider-turn cancelAfterEvents exceeds the bound",
            ));
        }
    }
    let provider = case_object
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            HarnessError::corpus("provider-turn provider must be an object")
        })?;
    match provider.get("kind").and_then(Value::as_str) {
        Some("fake") => {}
        Some("scripted") => {
            let events = provider
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    HarnessError::corpus(
                        "scripted provider requires an events array",
                    )
                })?;
            if events.len() > 4096 {
                return Err(HarnessError::corpus(
                    "scripted provider events exceed the bound",
                ));
            }
            // Events are untrusted raw data: any JSON value is
            // admissible and the production collector validation decides
            // malformed events.
        }
        _ => {
            return Err(HarnessError::corpus(
                "provider-turn provider kind must be fake or scripted",
            ));
        }
    }
    let messages =
        case_object.get("messages").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("provider-turn messages must be an array"),
        )?;
    if messages.len() > 128 {
        return Err(HarnessError::corpus(
            "provider-turn messages exceed the bound",
        ));
    }
    for item in messages {
        validate_provider_message(item)?;
    }
    let tools =
        case_object.get("tools").and_then(Value::as_array).ok_or_else(
            || HarnessError::corpus("provider-turn tools must be an array"),
        )?;
    if tools.len() > 128 {
        return Err(HarnessError::corpus(
            "provider-turn tools exceed the bound",
        ));
    }
    for tool in tools {
        let object = tool.as_object().ok_or_else(|| {
            HarnessError::corpus("provider-turn tools must be objects")
        })?;
        for required in ["name", "description", "inputSchema"] {
            if !object.contains_key(required) {
                return Err(HarnessError::corpus(format!(
                    "provider-turn tool requires the {required} field"
                )));
            }
        }
        if !object.get("name").is_some_and(Value::is_string)
            || !object.get("description").is_some_and(Value::is_string)
        {
            return Err(HarnessError::corpus(
                "provider-turn tool name and description must be strings",
            ));
        }
    }
    Ok(())
}

/// Validate one conversation item (strings may be '$repeat' markers).
fn validate_provider_message(item: &Value) -> Result<(), HarnessError> {
    let object = item.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn messages must contain objects")
    })?;
    let valid_string = |value: Option<&Value>| {
        value.is_some_and(|value| value.is_string() || is_repeat_marker(value))
    };
    match object.get("type").and_then(Value::as_str) {
        Some("user_message") | Some("assistant_message") => {
            if !valid_string(object.get("content")) {
                return Err(HarnessError::corpus(
                    "provider-turn message content must be a string or repeat marker",
                ));
            }
        }
        Some("assistant_tool_call") => {
            if !valid_string(object.get("callId"))
                || !valid_string(object.get("toolName"))
            {
                return Err(HarnessError::corpus(
                    "provider-turn tool call requires string callId and toolName",
                ));
            }
            if !object.contains_key("input") {
                return Err(HarnessError::corpus(
                    "provider-turn tool call requires an input",
                ));
            }
        }
        Some("tool_result") => {
            if !valid_string(object.get("callId"))
                || !valid_string(object.get("toolName"))
            {
                return Err(HarnessError::corpus(
                    "provider-turn tool result requires string callId and toolName",
                ));
            }
            if !object.get("result").is_some_and(Value::is_object) {
                return Err(HarnessError::corpus(
                    "provider-turn tool result requires a result object",
                ));
            }
        }
        other => {
            return Err(HarnessError::corpus(format!(
                "provider-turn message has an unknown type {other:?}"
            )));
        }
    }
    Ok(())
}

fn validate_provider_detach_case_input(
    detach: &Value,
) -> Result<(), HarnessError> {
    let object = detach.as_object().ok_or_else(|| {
        HarnessError::corpus("provider-turn detach case must be an object")
    })?;
    if object.len() != 3
        || !object.contains_key("value")
        || !object.contains_key("maxBytes")
        || !object.contains_key("actor")
    {
        return Err(HarnessError::corpus(
            "provider-turn detach case must contain exactly value, maxBytes, and actor",
        ));
    }
    let max_bytes =
        object.get("maxBytes").and_then(Value::as_u64).ok_or_else(|| {
            HarnessError::corpus(
                "provider-turn detach maxBytes must be an integer",
            )
        })?;
    if max_bytes == 0 || max_bytes > 1_048_576 {
        return Err(HarnessError::corpus(
            "provider-turn detach maxBytes is outside the bound",
        ));
    }
    let actor =
        object.get("actor").and_then(Value::as_str).ok_or_else(|| {
            HarnessError::corpus("provider-turn detach actor must be a string")
        })?;
    if actor.is_empty() || actor.len() > 128 {
        return Err(HarnessError::corpus(
            "provider-turn detach actor is outside the bound",
        ));
    }
    Ok(())
}
