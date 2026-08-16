//! Rust candidate for the `tool-loop` differential subject
//! (Stage 3R R7.2).
//!
//! The candidate composes the REAL production seams: `siralos-core`
//! Tool Registry, permission evaluator, ApprovedToolSurface, Tool Round,
//! Application Tool Loop, and R7.1 provider-turn collector, plus the
//! production workspace read adapter where a scenario selects it.
//! Harness-local deterministic stub Tools exist only for controlled
//! statuses and always enter through the real registry/gates/round/loop.
//! This module never duplicates Tool-loop behavior in harness code.

mod tool_loop_validate;

pub(super) use tool_loop_validate::validate_tool_loop_input;

use std::path::Path;

use serde_json::{Value, json};
use siralos_core::provider::{
    AssistantToolCallInput, CancellationSignal, ConversationItem, ModelEvent,
    ModelProvider, ModelRequest, ProviderEvent, ToolCallInput, ToolDefinition,
    ToolExecutionResult,
};
use siralos_core::tool::{
    ApprovedToolSurface, CapabilityId, PermissionPolicy, PermissionRule,
    PolicyRule, SiralosApplication, Tool, ToolLoopEvent, ToolRegistry,
};

use super::{
    DeterministicFakeProvider, HarnessError, WorkspaceReadTool,
    create_fixture_workspace, materialize_value, scenario_array,
    scenario_string, scenario_u64, tool_execution_result_value,
};

const WORKSPACE_READ_CAPABILITY: &str = "workspace.read";
fn workspace_read_capability() -> CapabilityId {
    CapabilityId::parse(WORKSPACE_READ_CAPABILITY)
        .expect("workspace.read is a valid capability id")
}

#[derive(Clone)]
struct ExecutionEntry {
    tool_name: String,
}

type ExecutionLog = std::rc::Rc<std::cell::RefCell<Vec<ExecutionEntry>>>;

/// Instrumentation-only wrapper: records every real `Tool::execute`
/// invocation so the canonical record can distinguish "executed" from
/// "denied before execution".
struct ObservedTool {
    inner: Box<dyn Tool>,
    log: ExecutionLog,
}

impl Tool for ObservedTool {
    fn definition(&self) -> ToolDefinition {
        self.inner.definition()
    }

    fn capability(&self) -> &CapabilityId {
        self.inner.capability()
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        self.log
            .borrow_mut()
            .push(ExecutionEntry { tool_name: self.inner.definition().name });
        self.inner.execute(input, cancellation)
    }
}

fn observed(tool: impl Tool + 'static, log: &ExecutionLog) -> Box<dyn Tool> {
    Box::new(ObservedTool { inner: Box::new(tool), log: log.clone() })
}

struct StubTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    result: ToolExecutionResult,
}

impl StubTool {
    fn new(name: &str, result: ToolExecutionResult) -> Self {
        Self {
            definition: ToolDefinition {
                name: name.to_owned(),
                description: "Deterministic tool-loop stub.".to_owned(),
                input_schema: json!({
                    "type": "object",
                    "additionalProperties": false
                }),
            },
            capability: workspace_read_capability(),
            result,
        }
    }
}

impl Tool for StubTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        _input: &Value,
        _cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        self.result.clone()
    }
}

fn stub_result(name: &str) -> Result<ToolExecutionResult, HarnessError> {
    match name {
        "stub.success" | "a.tool" | "b.tool" => {
            Ok(ToolExecutionResult::Success {
                output: json!({ "ok": true }),
                summary: "stub success".to_owned(),
            })
        }
        "stub.invalid_input" => Ok(ToolExecutionResult::InvalidInput {
            message: "stub invalid input.".to_owned(),
        }),
        "stub.denied" => Ok(ToolExecutionResult::Denied {
            message: "stub denied.".to_owned(),
        }),
        "stub.failed" => Ok(ToolExecutionResult::Failed {
            message: "stub failed.".to_owned(),
        }),
        "stub.cancelled" => Ok(ToolExecutionResult::Cancelled {
            message: "stub cancelled.".to_owned(),
        }),
        _ => Err(HarnessError::corpus(format!(
            "unsupported tool-loop registry tool {name}"
        ))),
    }
}

fn build_registry(
    names: &[String],
    fixture_root: Option<&Path>,
    log: &ExecutionLog,
) -> Result<ToolRegistry, HarnessError> {
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    for name in names {
        if name == "workspace.read" {
            let root = fixture_root.ok_or_else(|| {
                HarnessError::corpus(
                    "workspace.read requires a tool-loop fixture workspace",
                )
            })?;
            let tool = WorkspaceReadTool::new(root).map_err(|error| {
                HarnessError::new(
                    super::HarnessErrorKind::ProbeSpawn,
                    format!("cannot create workspace.read tool: {error}"),
                )
            })?;
            tools.push(observed(tool, log));
        } else {
            tools.push(observed(StubTool::new(name, stub_result(name)?), log));
        }
    }
    ToolRegistry::new(tools).map_err(|error| {
        HarnessError::corpus(format!(
            "tool-loop registry construction failed: {}",
            error.message()
        ))
    })
}

fn parse_policy(case: &Value) -> Result<PermissionPolicy, HarnessError> {
    let mut rules = vec![PolicyRule {
        capability: workspace_read_capability(),
        rule: PermissionRule::Allow,
    }];
    if let Ok(entries) = scenario_array(case, "rules") {
        for entry in entries {
            let capability =
                CapabilityId::parse(&scenario_string(entry, "capability")?)
                    .map_err(|error| {
                        HarnessError::corpus(format!(
                            "invalid tool-loop rule capability: {error}"
                        ))
                    })?;
            let rule = match scenario_string(entry, "decision")?.as_str() {
                "allow" => PermissionRule::Allow,
                "ask" => PermissionRule::Ask,
                "deny" => PermissionRule::Deny,
                other => {
                    return Err(HarnessError::corpus(format!(
                        "invalid tool-loop rule decision {other}"
                    )));
                }
            };
            rules.push(PolicyRule { capability, rule });
        }
    }
    Ok(PermissionPolicy::from_rules(rules))
}

fn parse_max_tool_rounds(case: &Value) -> Option<f64> {
    match case.get("maxToolRounds") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if value == "non-finite" => Some(f64::NAN),
        Some(Value::Number(value)) => value.as_f64(),
        Some(_) => None,
    }
}

/// Deterministic scripted provider for one or more turns.
///
/// The event list is split at each raw `completed` event. Raw events
/// still flow through the production collector validation; the narrow
/// exception is a `tool_call` carrying `inputJson`, which the probe
/// constructs as a trusted typed event so the source-ordered JSON text
/// survives into the production displayInput path.
struct ToolLoopScriptedProvider {
    events: Vec<Value>,
    position: std::cell::Cell<usize>,
    request_count: std::rc::Rc<std::cell::Cell<usize>>,
}

impl ToolLoopScriptedProvider {
    fn new(events: Vec<Value>) -> Self {
        Self {
            events,
            position: std::cell::Cell::new(0),
            request_count: std::rc::Rc::new(std::cell::Cell::new(0)),
        }
    }

    fn next_turn(&self) -> Vec<Value> {
        let start = self.position.get();
        if start >= self.events.len() {
            return Vec::new();
        }
        for (offset, event) in self.events[start..].iter().enumerate() {
            if event
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "completed")
            {
                let end = start + offset + 1;
                self.position.set(end);
                return self.events[start..end].to_vec();
            }
        }
        self.position.set(self.events.len());
        self.events[start..].to_vec()
    }
}

impl ModelProvider for ToolLoopScriptedProvider {
    type Stream<'a>
        = std::vec::IntoIter<ProviderEvent>
    where
        Self: 'a;

    fn id(&self) -> &str {
        "tool-loop-scripted"
    }

    fn stream<'a>(
        &'a self,
        _request: &'a ModelRequest,
        _cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        self.request_count.set(self.request_count.get() + 1);
        self.next_turn()
            .into_iter()
            .map(|event| {
                if event.get("type").and_then(Value::as_str)
                    == Some("provider_error")
                {
                    return ProviderEvent::Failed(
                        event
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("provider error")
                            .to_owned(),
                    );
                }
                if event.get("type").and_then(Value::as_str)
                    == Some("tool_call")
                {
                    if let Some(input_json) =
                        event.get("inputJson").and_then(Value::as_str)
                    {
                        let input = serde_json::from_str::<Value>(input_json)
                            .expect("validated inputJson is valid JSON");
                        return ProviderEvent::Event(ModelEvent::ToolCall {
                            call_id: event
                                .get("callId")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            tool_name: event
                                .get("toolName")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                            input: ToolCallInput::from_ordered_json(
                                input,
                                input_json.to_owned(),
                            ),
                        });
                    }
                }
                ProviderEvent::Raw(event)
            })
            .collect::<Vec<_>>()
            .into_iter()
    }
}

/// Provider-counting wrapper (request/turn count for canonical output).
struct CountingProvider<P> {
    inner: P,
    count: std::rc::Rc<std::cell::Cell<usize>>,
}

impl<P: ModelProvider> ModelProvider for CountingProvider<P> {
    type Stream<'a>
        = P::Stream<'a>
    where
        Self: 'a,
        P: 'a;

    fn id(&self) -> &str {
        self.inner.id()
    }

    fn stream<'a>(
        &'a self,
        request: &'a ModelRequest,
        cancellation: CancellationSignal<'a>,
    ) -> Self::Stream<'a> {
        self.count.set(self.count.get() + 1);
        self.inner.stream(request, cancellation)
    }
}

fn parse_visible_surface(
    case: &Value,
) -> Result<Option<ApprovedToolSurface>, HarnessError> {
    if case.get("visibleTools").is_none() {
        return Ok(None);
    }
    let names = scenario_array(case, "visibleTools")?
        .iter()
        .map(|name| scenario_string_from_value(name, "visibleTools entry"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(ApprovedToolSurface::new(names)))
}

fn scenario_string_from_value(
    value: &Value,
    label: &str,
) -> Result<String, HarnessError> {
    value.as_str().map(str::to_owned).ok_or_else(|| {
        HarnessError::corpus(format!("{label} must be a string"))
    })
}

fn canonical_event(event: &ToolLoopEvent) -> Value {
    match event {
        ToolLoopEvent::ResponseStarted => {
            json!({ "type": "response_started" })
        }
        ToolLoopEvent::TextDelta { text } => {
            json!({ "type": "text_delta", "text": text })
        }
        ToolLoopEvent::ResponseCompleted => {
            json!({ "type": "response_completed" })
        }
        ToolLoopEvent::ResponseCancelled => {
            json!({ "type": "response_cancelled" })
        }
        ToolLoopEvent::ResponseFailed { message } => {
            json!({ "type": "response_failed", "message": message })
        }
        ToolLoopEvent::ToolStarted { call_id, tool_name, display_input } => {
            json!({
                "type": "tool_started",
                "callId": call_id,
                "toolName": tool_name,
                "displayInputUtf16": display_input.units(),
            })
        }
        ToolLoopEvent::ToolCompleted { call_id, tool_name, summary } => {
            json!({
                "type": "tool_completed",
                "callId": call_id,
                "toolName": tool_name,
                "summary": summary,
            })
        }
        ToolLoopEvent::ToolFailed { call_id, tool_name, message } => json!({
            "type": "tool_failed",
            "callId": call_id,
            "toolName": tool_name,
            "message": message,
        }),
        ToolLoopEvent::ToolCancelled { call_id, tool_name } => json!({
            "type": "tool_cancelled",
            "callId": call_id,
            "toolName": tool_name,
        }),
    }
}

fn canonical_history(history: &[ConversationItem]) -> Value {
    let items = history
        .iter()
        .map(|item| match item {
            ConversationItem::UserMessage { content } => {
                json!({ "type": "user_message", "content": content })
            }
            ConversationItem::AssistantMessage { content } => {
                json!({ "type": "assistant_message", "content": content })
            }
            ConversationItem::AssistantToolCall {
                call_id,
                tool_name,
                input,
            } => match input {
                AssistantToolCallInput::Present(input) => json!({
                    "type": "assistant_tool_call",
                    "callId": call_id,
                    "toolName": tool_name,
                    "input": input,
                }),
                AssistantToolCallInput::Omitted => json!({
                    "type": "assistant_tool_call",
                    "callId": call_id,
                    "toolName": tool_name,
                }),
            },
            ConversationItem::ToolResult { call_id, tool_name, result } => {
                json!({
                    "type": "tool_result",
                    "callId": call_id,
                    "toolName": tool_name,
                    "result": tool_execution_result_value(result),
                })
            }
        })
        .collect::<Vec<_>>();
    Value::Array(items)
}

fn canonical_terminal(events: &[ToolLoopEvent]) -> Value {
    match events.last() {
        Some(ToolLoopEvent::ResponseCompleted) => {
            json!({ "kind": "completed" })
        }
        Some(ToolLoopEvent::ResponseCancelled) => {
            json!({ "kind": "cancelled" })
        }
        Some(ToolLoopEvent::ResponseFailed { message }) => {
            json!({ "kind": "failed", "message": message })
        }
        _ => json!({ "kind": "failed", "message": "missing terminal event" }),
    }
}

fn canonical_tool_calls(
    history: &[ConversationItem],
    log: &[ExecutionEntry],
) -> Result<Value, HarnessError> {
    let mut results = std::collections::BTreeMap::new();
    let mut order = Vec::new();
    for item in history {
        match item {
            ConversationItem::AssistantToolCall {
                call_id,
                tool_name,
                input,
            } => {
                order.push((call_id.clone(), tool_name.clone(), input.clone()))
            }
            ConversationItem::ToolResult { call_id, result, .. } => {
                results.insert(call_id.clone(), result.clone());
            }
            _ => {}
        }
    }
    let mut execution_index = 0usize;
    let mut calls = Vec::new();
    for (call_id, tool_name, input) in order {
        let executed = match &input {
            AssistantToolCallInput::Present(_) => {
                let matched = log
                    .get(execution_index)
                    .is_some_and(|entry| entry.tool_name == tool_name);
                if matched {
                    execution_index += 1;
                }
                matched
            }
            AssistantToolCallInput::Omitted => false,
        };
        let result = results.get(&call_id).ok_or_else(|| {
            HarnessError::corpus(format!(
                "tool-loop transcript is missing a result for {call_id}"
            ))
        })?;
        let mut call = json!({
            "callId": call_id,
            "toolName": tool_name,
            "inputPresent": input.value().is_some(),
            "executed": executed,
            "result": tool_execution_result_value(result),
        });
        if let Some(input) = input.value() {
            call.as_object_mut()
                .expect("call object")
                .insert("input".to_owned(), input.clone());
        }
        calls.push(call);
    }
    Ok(Value::Array(calls))
}

fn run_tool_loop_case(
    scenario_id: &str,
    case_index: usize,
    case: &Value,
) -> Result<Value, HarnessError> {
    let case = materialize_value(case)?;
    let prompt = scenario_string(&case, "prompt")?;
    let tool_names = scenario_array(&case, "tools")?
        .iter()
        .map(|name| {
            name.as_str().map(str::to_owned).ok_or_else(|| {
                HarnessError::corpus("tool-loop tools entries must be strings")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fixture_input = json!({
        "files": [{
            "path": "fixture.txt",
            "content": "hello\nworld\n",
        }]
    });
    let fixture_root =
        if tool_names.iter().any(|name| name == "workspace.read") {
            Some(create_fixture_workspace(
                &fixture_input,
                &format!("{scenario_id}-case-{case_index}"),
            )?)
        } else {
            None
        };
    let outcome = (|| -> Result<Value, HarnessError> {
        let log: ExecutionLog =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let registry =
            build_registry(&tool_names, fixture_root.as_deref(), &log)?;
        let policy = parse_policy(&case)?;
        let surface = if case.get("visibleTools").is_none() {
            None
        } else {
            parse_visible_surface(&case)?
        };
        let max_tool_rounds = parse_max_tool_rounds(&case);
        let cancel_after =
            scenario_u64(&case, "cancelAfterCompletedToolCalls")
                .map(|value| value as usize);
        let provider_spec = case.get("provider").ok_or_else(|| {
            HarnessError::corpus("tool-loop case requires a provider")
        })?;
        let kind = scenario_string(provider_spec, "kind")?;
        let request_count = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let mut events = Vec::new();
        let mut completed_tool_calls = 0usize;
        match kind.as_str() {
            "fake" => {
                let provider = CountingProvider {
                    inner: DeterministicFakeProvider::new(),
                    count: request_count.clone(),
                };
                let mut app = SiralosApplication::new(
                    &provider,
                    &registry,
                    policy,
                    surface,
                    max_tool_rounds,
                );
                app.send_prompt(prompt)
                    .map_err(|error| HarnessError::corpus(error.message()))?;
                while let Some(event) = app.poll_event() {
                    let event_is_completion =
                        matches!(event, ToolLoopEvent::ToolCompleted { .. });
                    events.push(event);
                    if event_is_completion {
                        completed_tool_calls += 1;
                        if cancel_after
                            .is_some_and(|n| completed_tool_calls == n)
                        {
                            app.cancel();
                        }
                    }
                }
                Ok(json!({
                    "caseIndex": case_index,
                    "events": events.iter().map(canonical_event).collect::<Vec<_>>(),
                    "terminal": canonical_terminal(&events),
                    "providerTurnCount": request_count.get(),
                    "history": canonical_history(app.history()),
                    "completedToolRounds": app.completed_tool_rounds(),
                    "toolCalls": canonical_tool_calls(
                        app.history(),
                        &log.borrow(),
                    )?,
                }))
            }
            "scripted" => {
                let provider = ToolLoopScriptedProvider::new(
                    scenario_array(provider_spec, "events")?.to_vec(),
                );
                let counting = CountingProvider {
                    inner: provider,
                    count: request_count.clone(),
                };
                let mut app = SiralosApplication::new(
                    &counting,
                    &registry,
                    policy,
                    surface,
                    max_tool_rounds,
                );
                app.send_prompt(prompt)
                    .map_err(|error| HarnessError::corpus(error.message()))?;
                while let Some(event) = app.poll_event() {
                    let event_is_completion =
                        matches!(event, ToolLoopEvent::ToolCompleted { .. });
                    events.push(event);
                    if event_is_completion {
                        completed_tool_calls += 1;
                        if cancel_after
                            .is_some_and(|n| completed_tool_calls == n)
                        {
                            app.cancel();
                        }
                    }
                }
                Ok(json!({
                    "caseIndex": case_index,
                    "events": events.iter().map(canonical_event).collect::<Vec<_>>(),
                    "terminal": canonical_terminal(&events),
                    "providerTurnCount": request_count.get(),
                    "history": canonical_history(app.history()),
                    "completedToolRounds": app.completed_tool_rounds(),
                    "toolCalls": canonical_tool_calls(
                        app.history(),
                        &log.borrow(),
                    )?,
                }))
            }
            other => Err(HarnessError::corpus(format!(
                "tool-loop provider kind must be fake or scripted, got {other}"
            ))),
        }
    })();
    if let Some(root) = fixture_root {
        let _ = std::fs::remove_dir_all(&root);
    }
    outcome
}

/// Canonical tool-loop record: one canonical observation per input case.
pub(super) fn tool_loop_record(
    scenario_id: &str,
    input: &Value,
) -> Result<Value, HarnessError> {
    let cases = scenario_array(input, "cases")?;
    let mut records = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        records.push(run_tool_loop_case(scenario_id, index, case)?);
    }
    Ok(json!({ "cases": records }))
}
