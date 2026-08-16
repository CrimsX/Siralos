//! Synchronous pull-based Application Tool Loop around the R7.1
//! bounded provider turn.
//!
//! The TypeScript reference exposes an async generator. Rust represents
//! the same observable behavior as an explicit pull machine: the Host
//! calls [`SiralosApplication::poll_event`] one event at a time,
//! cancellation is Host-owned between polls, and a second
//! [`SiralosApplication::send_prompt`] while a response is active
//! receives the typed `AlreadyResponding` rejection. No async runtime,
//! thread, lock, or shared synchronization is introduced.

use std::collections::VecDeque;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::provider::{
    CancellationToken, ConversationItem, ModelProvider, ToolDefinition,
    ToolExecutionResult, TurnOutcome, collect_provider_turn,
};
use crate::tool::budget::RoundBudget;
use crate::tool::events::ToolLoopEvent;
use crate::tool::permission::{
    PermissionDecision, PermissionPolicy, evaluate_permission,
};
use crate::tool::registry::{ApprovedToolSurface, Tool, ToolRegistry};
use crate::tool::round::{
    ExecutableToolCall, ToolCallExecution, ToolCallExecutor, ToolRoundKind,
    ToolRoundRunner, ToolRoundStep,
};

/// Why a prompt could not be started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStartError {
    /// Another prompt response is still active.
    AlreadyResponding,
}

impl PromptStartError {
    /// The exact externally observable message.
    pub fn message(&self) -> &'static str {
        match self {
            Self::AlreadyResponding => {
                "Siralos is already responding to a prompt."
            }
        }
    }
}

impl fmt::Display for PromptStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for PromptStartError {}

/// Host Tool-call executor: the exact per-call authorization gate order
/// plus guarded `Tool::execute`.
#[derive(Clone)]
struct HostToolExecutor<'a> {
    registry: &'a ToolRegistry,
    policy: PermissionPolicy,
    surface: Option<ApprovedToolSurface>,
}

impl HostToolExecutor<'_> {
    /// Policy-filtered provider-visible definitions (denied tools are
    /// never shown to the provider; ask remains visible).
    fn provider_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.registry
            .definitions()
            .into_iter()
            .filter(|info| {
                !matches!(
                    evaluate_permission(&info.capability, &self.policy),
                    PermissionDecision::Deny { .. }
                )
            })
            .map(|info| info.definition)
            .collect()
    }
}

impl ToolCallExecutor for HostToolExecutor<'_> {
    fn execute_call(
        &mut self,
        call: ExecutableToolCall<'_>,
        cancellation: crate::provider::CancellationSignal<'_>,
    ) -> ToolCallExecution {
        let mut events = vec![ToolLoopEvent::ToolStarted {
            call_id: call.call_id.to_owned(),
            tool_name: call.tool_name.to_owned(),
            display_input: call.display_input(),
        }];
        let Some(tool) = self.registry.get(call.tool_name) else {
            let message = format!("Unknown tool: {}.", call.tool_name);
            events.push(failure_event(call.call_id, call.tool_name, &message));
            return ToolCallExecution {
                events,
                result: ToolExecutionResult::Failed { message },
            };
        };
        if let Some(surface) = &self.surface {
            if !surface.contains(call.tool_name) {
                let message = format!(
                    "Tool {} is not in the projected tool schema for this session and was denied before execution.",
                    call.tool_name
                );
                events.push(failure_event(
                    call.call_id,
                    call.tool_name,
                    &message,
                ));
                return ToolCallExecution {
                    events,
                    result: ToolExecutionResult::Denied { message },
                };
            }
        }
        let capability = tool.capability();
        match evaluate_permission(capability, &self.policy) {
            PermissionDecision::Deny { reason } => {
                let message = format!(
                    "Capability {capability} is denied by policy: {reason}"
                );
                events.push(failure_event(
                    call.call_id,
                    call.tool_name,
                    &message,
                ));
                return ToolCallExecution {
                    events,
                    result: ToolExecutionResult::Denied { message },
                };
            }
            PermissionDecision::Ask { .. } => {
                let message = format!(
                    "Capability {capability} requires approval, but this tool does not support a reviewable preparation protocol; the call was denied without execution."
                );
                events.push(failure_event(
                    call.call_id,
                    call.tool_name,
                    &message,
                ));
                return ToolCallExecution {
                    events,
                    result: ToolExecutionResult::Denied { message },
                };
            }
            PermissionDecision::Allow => {}
        }
        let result = execute_guarded(tool, call.input, cancellation);
        events.push(outcome_event(call.call_id, call.tool_name, &result));
        ToolCallExecution { events, result }
    }
}

/// Invoke a Tool exactly once; no automatic retries. A panicking Tool
/// implementation is converted to the reference non-Error throw result
/// (the Tool boundary is trusted Host code and the conversion is the
/// observable TypeScript catch semantics).
fn execute_guarded(
    tool: &dyn Tool,
    input: &serde_json::Value,
    cancellation: crate::provider::CancellationSignal<'_>,
) -> ToolExecutionResult {
    match catch_unwind(AssertUnwindSafe(|| tool.execute(input, cancellation)))
    {
        Ok(result) => result,
        Err(_) => ToolExecutionResult::Failed {
            message: "The provider failed with an unknown error.".to_owned(),
        },
    }
}

fn failure_event(
    call_id: &str,
    tool_name: &str,
    message: &str,
) -> ToolLoopEvent {
    ToolLoopEvent::ToolFailed {
        call_id: call_id.to_owned(),
        tool_name: tool_name.to_owned(),
        message: message.to_owned(),
    }
}

fn outcome_event(
    call_id: &str,
    tool_name: &str,
    result: &ToolExecutionResult,
) -> ToolLoopEvent {
    match result {
        ToolExecutionResult::Success { summary, .. } => {
            ToolLoopEvent::ToolCompleted {
                call_id: call_id.to_owned(),
                tool_name: tool_name.to_owned(),
                summary: summary.clone(),
            }
        }
        ToolExecutionResult::Cancelled { .. } => {
            ToolLoopEvent::ToolCancelled {
                call_id: call_id.to_owned(),
                tool_name: tool_name.to_owned(),
            }
        }
        other => ToolLoopEvent::ToolFailed {
            call_id: call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            message: other.message().to_owned(),
        },
    }
}

/// One collected successful provider turn pending text replay and Tool
/// Round handling.
struct CollectedTurn {
    assistant_text: String,
    text_deltas: VecDeque<String>,
    tool_calls: Vec<crate::provider::TurnToolCall>,
}

enum Phase<'a> {
    Start,
    CollectTurn,
    EmitText {
        turn: CollectedTurn,
    },
    RunningRound {
        runner: ToolRoundRunner<HostToolExecutor<'a>>,
        assistant_text: String,
    },
    Terminal {
        event: ToolLoopEvent,
    },
    Done,
}

/// One active prompt response machine.
struct ResponseMachine<'a, P: ModelProvider> {
    provider: &'a P,
    host: HostToolExecutor<'a>,
    token: CancellationToken,
    max_tool_rounds: RoundBudget,
    history: Vec<ConversationItem>,
    attempted_tool_rounds: u32,
    completed_tool_rounds: u32,
    provider_turns: u32,
    phase: Phase<'a>,
}

impl<'a, P: ModelProvider> ResponseMachine<'a, P> {
    fn new(
        provider: &'a P,
        host: HostToolExecutor<'a>,
        max_tool_rounds: RoundBudget,
    ) -> Self {
        Self {
            provider,
            host,
            token: CancellationToken::new(),
            max_tool_rounds,
            history: Vec::new(),
            attempted_tool_rounds: 0,
            completed_tool_rounds: 0,
            provider_turns: 0,
            phase: Phase::Start,
        }
    }

    fn cancel(&self) {
        self.token.cancel();
    }

    fn next_event(&mut self) -> Option<ToolLoopEvent> {
        loop {
            let phase = std::mem::replace(&mut self.phase, Phase::Done);
            match phase {
                Phase::Start => {
                    self.phase = Phase::CollectTurn;
                    return Some(ToolLoopEvent::ResponseStarted);
                }
                Phase::CollectTurn => {
                    if self.token.is_cancelled() {
                        self.phase = Phase::Terminal {
                            event: ToolLoopEvent::ResponseCancelled,
                        };
                        continue;
                    }
                    self.provider_turns += 1;
                    let definitions = self.host.provider_tool_definitions();
                    let outcome = collect_provider_turn(
                        self.provider,
                        &self.history,
                        &definitions,
                        None,
                        &self.token,
                    );
                    match outcome {
                        TurnOutcome::Cancelled => {
                            self.phase = Phase::Terminal {
                                event: ToolLoopEvent::ResponseCancelled,
                            };
                        }
                        TurnOutcome::Failed { failure } => {
                            self.phase = Phase::Terminal {
                                event: ToolLoopEvent::ResponseFailed {
                                    message: failure.application_message(),
                                },
                            };
                        }
                        TurnOutcome::Turn {
                            assistant_text,
                            text_deltas,
                            tool_calls,
                        } => {
                            let turn = CollectedTurn {
                                assistant_text,
                                text_deltas: text_deltas.into(),
                                tool_calls,
                            };
                            if turn.text_deltas.is_empty() {
                                self.phase = self.handle_collected_turn(turn);
                            } else {
                                self.phase = Phase::EmitText { turn };
                            }
                        }
                    }
                }
                Phase::EmitText { mut turn } => {
                    match turn.text_deltas.pop_front() {
                        Some(text) => {
                            self.phase = Phase::EmitText { turn };
                            return Some(ToolLoopEvent::TextDelta { text });
                        }
                        None => {
                            self.phase = self.handle_collected_turn(turn);
                        }
                    }
                }
                Phase::RunningRound { mut runner, assistant_text } => {
                    match runner.next(&self.token) {
                        Some(ToolRoundStep::Event(event)) => {
                            self.phase =
                                Phase::RunningRound { runner, assistant_text };
                            return Some(event);
                        }
                        Some(ToolRoundStep::Finished(outcome)) => {
                            self.phase =
                                self.finish_round(assistant_text, outcome);
                        }
                        None => {
                            // A finished runner is always consumed by the
                            // Finished step above; reaching this branch
                            // would indicate an internal state error.
                            self.phase = Phase::Done;
                        }
                    }
                }
                Phase::Terminal { event } => {
                    self.phase = Phase::Done;
                    return Some(event);
                }
                Phase::Done => return None,
            }
        }
    }

    fn handle_collected_turn(&mut self, turn: CollectedTurn) -> Phase<'a> {
        if turn.tool_calls.is_empty() {
            if !turn.assistant_text.is_empty() {
                self.history.push(ConversationItem::AssistantMessage {
                    content: turn.assistant_text,
                });
            }
            return Phase::Terminal {
                event: ToolLoopEvent::ResponseCompleted,
            };
        }
        if self.attempted_tool_rounds >= self.max_tool_rounds.get() {
            return Phase::Terminal {
                event: ToolLoopEvent::ResponseFailed {
                    message: self.max_tool_rounds.cap_message(),
                },
            };
        }
        self.attempted_tool_rounds += 1;
        let runner = ToolRoundRunner::new(turn.tool_calls, self.host.clone());
        Phase::RunningRound { runner, assistant_text: turn.assistant_text }
    }

    fn finish_round(
        &mut self,
        assistant_text: String,
        outcome: crate::tool::round::ToolRoundOutcome,
    ) -> Phase<'a> {
        match outcome.kind {
            ToolRoundKind::Completed => {
                self.completed_tool_rounds += 1;
                if !assistant_text.is_empty() {
                    self.history.push(ConversationItem::AssistantMessage {
                        content: assistant_text,
                    });
                }
                self.history.extend(outcome.transcript);
                Phase::CollectTurn
            }
            ToolRoundKind::Cancelled => {
                // A cancelled mixed turn commits the full paired round
                // transcript but never the assistant text.
                self.history.extend(outcome.transcript);
                Phase::Terminal { event: ToolLoopEvent::ResponseCancelled }
            }
        }
    }

    fn provider_turn_count(&self) -> u32 {
        self.provider_turns
    }
}

enum AppState<'a, P: ModelProvider> {
    Idle,
    Responding(Box<ResponseMachine<'a, P>>),
}

/// Generic single-flight Application Tool Loop.
///
/// The application owns authoritative history. One prompt response is
/// active at a time; starting another while responding fails with the
/// exact `AlreadyResponding` message.
pub struct SiralosApplication<'a, P: ModelProvider> {
    provider: &'a P,
    registry: &'a ToolRegistry,
    policy: PermissionPolicy,
    surface: Option<ApprovedToolSurface>,
    max_tool_rounds: RoundBudget,
    history: Vec<ConversationItem>,
    completed_tool_rounds: u32,
    provider_turn_count: u32,
    state: AppState<'a, P>,
}

impl<'a, P: ModelProvider> SiralosApplication<'a, P> {
    /// Compose the loop from Host-owned dependencies.
    ///
    /// `max_tool_rounds` is normalized with the exact reference rules
    /// (missing/non-finite → 8; floor; clamp to `0..=32`).
    pub fn new(
        provider: &'a P,
        registry: &'a ToolRegistry,
        policy: PermissionPolicy,
        surface: Option<ApprovedToolSurface>,
        max_tool_rounds: Option<f64>,
    ) -> Self {
        Self {
            provider,
            registry,
            policy,
            surface,
            max_tool_rounds: RoundBudget::normalize(max_tool_rounds),
            history: Vec::new(),
            completed_tool_rounds: 0,
            provider_turn_count: 0,
            state: AppState::Idle,
        }
    }

    /// Start one prompt response.
    ///
    /// Appends the user message exactly once and emits
    /// `response_started` as the first pull event. Returns
    /// [`PromptStartError::AlreadyResponding`] while another response is
    /// active.
    pub fn send_prompt(
        &mut self,
        text: String,
    ) -> Result<(), PromptStartError> {
        if matches!(&self.state, AppState::Responding(_)) {
            return Err(PromptStartError::AlreadyResponding);
        }
        let host = HostToolExecutor {
            registry: self.registry,
            policy: self.policy.clone(),
            surface: self.surface.clone(),
        };
        let mut machine =
            ResponseMachine::new(self.provider, host, self.max_tool_rounds);
        machine.history = std::mem::take(&mut self.history);
        machine.history.push(ConversationItem::UserMessage { content: text });
        self.state = AppState::Responding(Box::new(machine));
        Ok(())
    }

    /// Pull the next Tool-loop event, advancing at most one observable
    /// step.
    ///
    /// Returns `None` after the terminal event has been consumed; the
    /// authoritative history and counters are restored to the
    /// application at that point.
    pub fn poll_event(&mut self) -> Option<ToolLoopEvent> {
        let event = match &mut self.state {
            AppState::Idle => return None,
            AppState::Responding(machine) => machine.next_event(),
        };
        if event.is_none() {
            self.restore_machine();
        }
        event
    }

    /// Host cancellation authority. Tools and providers only ever
    /// observe the read-only signal.
    pub fn cancel(&mut self) {
        if let AppState::Responding(machine) = &self.state {
            machine.cancel();
        }
    }

    /// Whether a response is currently active.
    pub fn is_responding(&self) -> bool {
        matches!(&self.state, AppState::Responding(_))
    }

    /// The authoritative Host-owned conversation history.
    pub fn history(&self) -> &[ConversationItem] {
        match &self.state {
            AppState::Idle => &self.history,
            AppState::Responding(machine) => &machine.history,
        }
    }

    /// The number of completed Tool Rounds in the current (or most
    /// recent) response.
    pub fn completed_tool_rounds(&self) -> u32 {
        match &self.state {
            AppState::Idle => self.completed_tool_rounds,
            AppState::Responding(machine) => machine.completed_tool_rounds,
        }
    }

    /// The number of provider turns collected by the current (or most
    /// recent) response.
    pub fn provider_turn_count(&self) -> u32 {
        match &self.state {
            AppState::Idle => self.provider_turn_count,
            AppState::Responding(machine) => machine.provider_turn_count(),
        }
    }

    fn restore_machine(&mut self) {
        let state = std::mem::replace(&mut self.state, AppState::Idle);
        if let AppState::Responding(machine) = state {
            let provider_turn_count = machine.provider_turn_count();
            self.history = machine.history;
            self.completed_tool_rounds = machine.completed_tool_rounds;
            self.provider_turn_count = provider_turn_count;
        }
    }
}
