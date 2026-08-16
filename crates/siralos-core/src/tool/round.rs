//! Generic Tool Round: pre-seeded transcript, sequential execution,
//! invalid-call pairing, and cancelled-tail pairing.
//!
//! The frozen invariant is mechanical: every retained assistant tool
//! call receives exactly one tool result with the same call id and tool
//! name, in provider emission order, before the transcript leaves the
//! round. Invalid calls receive one failed result with no lookup,
//! authorization, or execution. Cancellation (Host pre-cancellation or
//! a tool returning `cancelled`) stops execution and pairs every
//! unstarted call with the deterministic skipped-call cancelled result.

use std::collections::VecDeque;

use serde_json::Value;

use crate::provider::{
    AssistantToolCallInput, CancellationSignal, CancellationToken,
    ConversationItem, ToolExecutionResult, TurnToolCall,
};
use crate::tool::display_input::{DisplayInput, to_display_input};
use crate::tool::events::ToolLoopEvent;

/// The deterministic skipped-call cancelled message.
pub const CANCELLED_BEFORE_EXECUTION_MESSAGE: &str =
    "The tool call was cancelled before it executed.";

/// One executable tool call view handed to the Tool-call executor.
#[derive(Debug, Clone, Copy)]
pub struct ExecutableToolCall<'a> {
    /// Correlation id of the call.
    pub call_id: &'a str,
    /// Name of the requested tool.
    pub tool_name: &'a str,
    /// Detached executable JSON input.
    pub input: &'a Value,
    /// Source-ordered canonical JSON text, when the Host attached it.
    pub ordered_json: Option<&'a str>,
}

impl ExecutableToolCall<'_> {
    /// Format the reference `tool_started.displayInput` from this call.
    pub fn display_input(&self) -> DisplayInput {
        let serialized = match self.ordered_json {
            Some(json) => json.to_owned(),
            None => serde_json::to_string(self.input)
                .expect("serde_json::Value is always serializable"),
        };
        to_display_input(&serialized)
    }
}

/// Events plus the owned result of one executable call.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallExecution {
    /// Tool-loop events emitted for this call (started + outcome).
    pub events: Vec<ToolLoopEvent>,
    /// The retained ToolExecutionResult.
    pub result: ToolExecutionResult,
}

/// The per-call Host authorization + invocation seam used by the round.
pub trait ToolCallExecutor {
    /// Authorize and (when permitted) invoke one executable call.
    ///
    /// The executor must apply the frozen gate order: `tool_started`,
    /// registry lookup, approved-surface check, per-call capability
    /// recheck, plain-Tool ask check, then `Tool::execute`.
    fn execute_call(
        &mut self,
        call: ExecutableToolCall<'_>,
        cancellation: CancellationSignal<'_>,
    ) -> ToolCallExecution;
}

/// Whether a Tool Round completed all calls or was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolRoundKind {
    /// Every retained call received its result without cancellation.
    Completed,
    /// The round stopped early; every retained call is still paired.
    Cancelled,
}

/// The final paired transcript of one Tool Round.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRoundOutcome {
    /// Completed or cancelled.
    pub kind: ToolRoundKind,
    /// The full paired round transcript in append order.
    pub transcript: Vec<ConversationItem>,
}

/// One step of the pull-based Tool Round.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolRoundStep {
    /// The next Tool-loop event to emit.
    Event(ToolLoopEvent),
    /// The round is finished; the outcome carries the full transcript.
    Finished(ToolRoundOutcome),
}

/// Pull-based Tool Round state machine.
///
/// The runner owns the retained call queue, the pre-seeded transcript,
/// and the queued events for one call. Each [`ToolRoundRunner::next`]
/// advances by at most one call so a Host can cancel between calls
/// without threads or an async runtime.
pub struct ToolRoundRunner<E: ToolCallExecutor> {
    calls: VecDeque<TurnToolCall>,
    transcript: Vec<ConversationItem>,
    pending_events: VecDeque<ToolLoopEvent>,
    finish_cancelled_after_events: bool,
    finished: bool,
    executor: E,
}

impl<E: ToolCallExecutor> ToolRoundRunner<E> {
    /// Pre-seed the round transcript with every retained call in
    /// provider order and prepare sequential execution.
    pub fn new(calls: Vec<TurnToolCall>, executor: E) -> Self {
        let transcript = seed_transcript(&calls);
        Self {
            calls: calls.into(),
            transcript,
            pending_events: VecDeque::new(),
            finish_cancelled_after_events: false,
            finished: false,
            executor,
        }
    }

    /// Advance the round by one observable step.
    ///
    /// Returns `None` after [`ToolRoundStep::Finished`] has been
    /// consumed. Already-produced events are drained before the next
    /// call is authorized, so Host cancellation between calls can never
    /// suppress an already-authorized result. When a tool returns
    /// `cancelled`, its own started/outcome events are drained before
    /// the finished cancelled outcome.
    pub fn next(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Option<ToolRoundStep> {
        if self.finished {
            return None;
        }
        if let Some(event) = self.pending_events.pop_front() {
            return Some(ToolRoundStep::Event(event));
        }
        if self.finish_cancelled_after_events {
            self.finish_cancelled_after_events = false;
            return Some(ToolRoundStep::Finished(self.finish_cancelled()));
        }
        if self.calls.is_empty() {
            self.finished = true;
            return Some(ToolRoundStep::Finished(ToolRoundOutcome {
                kind: ToolRoundKind::Completed,
                transcript: std::mem::take(&mut self.transcript),
            }));
        }
        if cancellation.is_cancelled() {
            return Some(ToolRoundStep::Finished(self.finish_cancelled()));
        }
        let call = self.calls.pop_front().expect("checked non-empty");
        match call {
            TurnToolCall::Invalid { call_id, tool_name, message } => {
                self.pending_events.push_back(ToolLoopEvent::ToolFailed {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    message: message.clone(),
                });
                self.transcript.push(ConversationItem::ToolResult {
                    call_id,
                    tool_name,
                    result: ToolExecutionResult::Failed { message },
                });
                self.next(cancellation)
            }
            TurnToolCall::Execute { call_id, tool_name, input } => {
                let ordered_json = input.ordered_json().map(str::to_owned);
                let execution = self.executor.execute_call(
                    ExecutableToolCall {
                        call_id: &call_id,
                        tool_name: &tool_name,
                        input: input.value(),
                        ordered_json: ordered_json.as_deref(),
                    },
                    cancellation.signal(),
                );
                self.pending_events.extend(execution.events);
                let cancelled = execution.result.status_str() == "cancelled";
                self.transcript.push(ConversationItem::ToolResult {
                    call_id,
                    tool_name,
                    result: execution.result,
                });
                if cancelled {
                    self.finish_cancelled_after_events = true;
                }
                self.next(cancellation)
            }
        }
    }

    /// Pair every unstarted retained call (the current queue, or the
    /// queue plus nothing for the already-removed current call) with
    /// the skipped-call cancelled result and return the cancelled
    /// outcome.
    fn finish_cancelled(&mut self) -> ToolRoundOutcome {
        for call in std::mem::take(&mut self.calls) {
            let (call_id, tool_name) = match call {
                TurnToolCall::Execute { call_id, tool_name, .. }
                | TurnToolCall::Invalid { call_id, tool_name, .. } => {
                    (call_id, tool_name)
                }
            };
            self.transcript.push(ConversationItem::ToolResult {
                call_id,
                tool_name,
                result: ToolExecutionResult::Cancelled {
                    message: CANCELLED_BEFORE_EXECUTION_MESSAGE.to_owned(),
                },
            });
        }
        self.finished = true;
        ToolRoundOutcome {
            kind: ToolRoundKind::Cancelled,
            transcript: std::mem::take(&mut self.transcript),
        }
    }
}

/// The pre-seeded transcript: one assistant tool call per retained call
/// in provider order; invalid calls are recorded without an input
/// payload.
fn seed_transcript(calls: &[TurnToolCall]) -> Vec<ConversationItem> {
    calls
        .iter()
        .map(|call| match call {
            TurnToolCall::Execute { call_id, tool_name, input } => {
                ConversationItem::AssistantToolCall {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    input: AssistantToolCallInput::Present(
                        input.value().clone(),
                    ),
                }
            }
            TurnToolCall::Invalid { call_id, tool_name, .. } => {
                ConversationItem::AssistantToolCall {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                    input: AssistantToolCallInput::Omitted,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::provider::{
        CancellationToken, ConversationItem, ToolCallInput,
        ToolExecutionResult, TurnToolCall,
    };
    use serde_json::json;

    use super::{
        CANCELLED_BEFORE_EXECUTION_MESSAGE, ExecutableToolCall,
        ToolCallExecution, ToolCallExecutor, ToolLoopEvent, ToolRoundKind,
        ToolRoundRunner, ToolRoundStep,
    };

    struct FixedExecutor {
        events: Vec<ToolLoopEvent>,
        result: ToolExecutionResult,
    }

    impl ToolCallExecutor for FixedExecutor {
        fn execute_call(
            &mut self,
            _call: ExecutableToolCall<'_>,
            _cancellation: crate::provider::CancellationSignal<'_>,
        ) -> ToolCallExecution {
            ToolCallExecution {
                events: self.events.clone(),
                result: self.result.clone(),
            }
        }
    }

    fn execute_call(
        call_id: &str,
        tool_name: &str,
        input: serde_json::Value,
    ) -> TurnToolCall {
        TurnToolCall::Execute {
            call_id: call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            input: ToolCallInput::from_value(input),
        }
    }

    fn invalid_call(call_id: &str, tool_name: &str) -> TurnToolCall {
        TurnToolCall::Invalid {
            call_id: call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            message: "invalid call".to_owned(),
        }
    }

    fn success_executor() -> FixedExecutor {
        FixedExecutor {
            events: vec![ToolLoopEvent::ToolCompleted {
                call_id: "c1".to_owned(),
                tool_name: "a.tool".to_owned(),
                summary: "ok".to_owned(),
            }],
            result: ToolExecutionResult::Success {
                output: json!({ "ok": true }),
                summary: "ok".to_owned(),
            },
        }
    }

    #[test]
    fn seeds_every_call_and_pairs_one_result_in_order() {
        let calls = vec![
            execute_call("c1", "a.tool", json!({})),
            invalid_call("invalid-call-1", "<empty>"),
            execute_call("c2", "b.tool", json!({ "v": 2 })),
        ];
        let mut runner = ToolRoundRunner::new(calls, success_executor());
        let token = CancellationToken::new();
        let mut events = Vec::new();
        let outcome = loop {
            match runner.next(&token).expect("step") {
                ToolRoundStep::Event(event) => events.push(event),
                ToolRoundStep::Finished(outcome) => break outcome,
            }
        };
        assert_eq!(outcome.kind, ToolRoundKind::Completed);
        assert_eq!(outcome.transcript.len(), 6);
        assert!(matches!(
            &outcome.transcript[0],
            ConversationItem::AssistantToolCall { .. }
        ));
        assert!(matches!(
            &outcome.transcript[1],
            ConversationItem::AssistantToolCall { .. }
        ));
        let ids: Vec<&str> = outcome
            .transcript
            .iter()
            .filter_map(|item| match item {
                ConversationItem::ToolResult { call_id, .. } => {
                    Some(call_id.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["c1", "invalid-call-1", "c2"]);
    }

    #[test]
    fn invalid_calls_get_one_failed_result_without_execution() {
        let calls = vec![invalid_call("invalid-call-1", "mystery.tool")];
        let mut runner = ToolRoundRunner::new(calls, success_executor());
        let token = CancellationToken::new();
        let mut failed = Vec::new();
        let outcome = loop {
            match runner.next(&token).expect("step") {
                ToolRoundStep::Event(event) => {
                    failed.push(event);
                }
                ToolRoundStep::Finished(outcome) => break outcome,
            }
        };
        assert_eq!(failed.len(), 1);
        assert!(matches!(failed[0], ToolLoopEvent::ToolFailed { .. }));
        assert_eq!(outcome.transcript.len(), 2);
        assert!(matches!(
            &outcome.transcript[0],
            ConversationItem::AssistantToolCall { .. }
        ));
        assert!(matches!(
            &outcome.transcript[1],
            ConversationItem::ToolResult {
                result: ToolExecutionResult::Failed { .. },
                ..
            }
        ));
    }

    #[test]
    fn pre_cancelled_round_skips_every_call_with_exact_message() {
        let calls = vec![
            execute_call("c1", "a.tool", json!({})),
            execute_call("c2", "a.tool", json!({})),
        ];
        let mut runner = ToolRoundRunner::new(calls, success_executor());
        let token = CancellationToken::new();
        token.cancel();
        let outcome = match runner.next(&token).expect("step") {
            ToolRoundStep::Finished(outcome) => outcome,
            ToolRoundStep::Event(_) => {
                panic!("no events may precede cancellation")
            }
        };
        assert_eq!(outcome.kind, ToolRoundKind::Cancelled);
        let results: Vec<&ToolExecutionResult> = outcome
            .transcript
            .iter()
            .filter_map(|item| match item {
                ConversationItem::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 2);
        for result in results {
            assert!(matches!(
                result,
                ToolExecutionResult::Cancelled { message }
                    if message == CANCELLED_BEFORE_EXECUTION_MESSAGE
            ));
        }
    }

    #[test]
    fn tool_returned_cancelled_keeps_own_message_and_pairs_tail() {
        let calls = vec![
            execute_call("c1", "a.tool", json!({})),
            execute_call("c2", "a.tool", json!({})),
            execute_call("c3", "a.tool", json!({})),
        ];
        let mut runner = ToolRoundRunner::new(
            calls,
            FixedExecutor {
                events: vec![ToolLoopEvent::ToolCancelled {
                    call_id: "c1".to_owned(),
                    tool_name: "a.tool".to_owned(),
                }],
                result: ToolExecutionResult::Cancelled {
                    message: "tool cancelled".to_owned(),
                },
            },
        );
        let token = CancellationToken::new();
        let outcome = loop {
            match runner.next(&token).expect("step") {
                ToolRoundStep::Event(_) => {}
                ToolRoundStep::Finished(outcome) => break outcome,
            }
        };
        assert_eq!(outcome.kind, ToolRoundKind::Cancelled);
        let results: Vec<&ToolExecutionResult> = outcome
            .transcript
            .iter()
            .filter_map(|item| match item {
                ConversationItem::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].message(), "tool cancelled");
        assert_eq!(results[1].message(), CANCELLED_BEFORE_EXECUTION_MESSAGE);
        assert_eq!(results[2].message(), CANCELLED_BEFORE_EXECUTION_MESSAGE);
    }
}
