//! Tool execution result value and the bounded detach/validation
//! boundary (Stage 3R R7.1).
//!
//! The typed result is a value/protocol contract only: no tool executes
//! in R7.1. 'detach_bounded_tool_result' is the external boundary for
//! host tool-adapter results: it validates, byte-bounds, and detaches
//! the value so unknown fields and caller-owned mutable state never
//! survive into the retained typed result. Rust ownership makes
//! detachment natural; serialization remains only at this external
//! JSON boundary for size accounting and canonical observable behavior.

use serde_json::Value;

/// One tool execution result value (contract only; no execution).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecutionResult {
    /// The tool completed successfully.
    Success {
        /// Structured output.
        output: Value,
        /// Bounded human-readable summary.
        summary: String,
    },
    /// The input was invalid.
    InvalidInput {
        /// Failure message.
        message: String,
    },
    /// Execution was denied by Host policy.
    Denied {
        /// Failure message.
        message: String,
    },
    /// A conflict (for example a stale revision) blocked execution.
    Conflict {
        /// Failure message.
        message: String,
    },
    /// The tool failed.
    Failed {
        /// Failure message.
        message: String,
    },
    /// Execution was cancelled.
    Cancelled {
        /// Failure message.
        message: String,
    },
    /// Execution timed out.
    TimedOut {
        /// Failure message.
        message: String,
    },
    /// The output exceeded its bound.
    OutputLimit {
        /// Failure message.
        message: String,
    },
    /// The sandbox denied execution.
    SandboxDenied {
        /// Failure message.
        message: String,
    },
    /// The sandbox is unavailable.
    SandboxUnavailable {
        /// Failure message.
        message: String,
    },
    /// The workspace changed in an unexpected way.
    WorkspaceViolation {
        /// Failure message.
        message: String,
    },
    /// The capability is unavailable.
    Unavailable {
        /// Failure message.
        message: String,
    },
}

impl ToolExecutionResult {
    /// Stable wire status matching the reference vocabulary.
    pub fn status_str(&self) -> &'static str {
        match self {
            ToolExecutionResult::Success { .. } => "success",
            ToolExecutionResult::InvalidInput { .. } => "invalid_input",
            ToolExecutionResult::Denied { .. } => "denied",
            ToolExecutionResult::Conflict { .. } => "conflict",
            ToolExecutionResult::Failed { .. } => "failed",
            ToolExecutionResult::Cancelled { .. } => "cancelled",
            ToolExecutionResult::TimedOut { .. } => "timed_out",
            ToolExecutionResult::OutputLimit { .. } => "output_limit",
            ToolExecutionResult::SandboxDenied { .. } => "sandbox_denied",
            ToolExecutionResult::SandboxUnavailable { .. } => {
                "sandbox_unavailable"
            }
            ToolExecutionResult::WorkspaceViolation { .. } => {
                "workspace_violation"
            }
            ToolExecutionResult::Unavailable { .. } => "unavailable",
        }
    }

    /// The failure message of a non-success result.
    pub fn message(&self) -> &str {
        match self {
            ToolExecutionResult::Success { .. } => "",
            ToolExecutionResult::InvalidInput { message }
            | ToolExecutionResult::Denied { message }
            | ToolExecutionResult::Conflict { message }
            | ToolExecutionResult::Failed { message }
            | ToolExecutionResult::Cancelled { message }
            | ToolExecutionResult::TimedOut { message }
            | ToolExecutionResult::OutputLimit { message }
            | ToolExecutionResult::SandboxDenied { message }
            | ToolExecutionResult::SandboxUnavailable { message }
            | ToolExecutionResult::WorkspaceViolation { message }
            | ToolExecutionResult::Unavailable { message } => message,
        }
    }
}

/// The frozen generic failure status set accepted by the detach boundary.
pub const TOOL_FAILURE_STATUSES: &[&str] = &[
    "invalid_input",
    "denied",
    "conflict",
    "failed",
    "cancelled",
    "timed_out",
    "output_limit",
    "sandbox_denied",
    "sandbox_unavailable",
    "workspace_violation",
    "unavailable",
];

/// Typed rejection at the tool-result detach boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetachFailure {
    /// The boundary value was not JSON-serializable. Structurally
    /// unreachable for an already-parsed 'serde_json::Value'; the class
    /// is preserved with its frozen external message for the boundary
    /// contract.
    NonJson,
    /// The serialized value exceeded the caller's byte bound.
    Oversize {
        /// The caller-supplied bound.
        max_bytes: usize,
    },
    /// The value is not a JSON object.
    InvalidShape,
    /// The status is not success and not an allowed failure status.
    UnknownStatus,
    /// A success result without output or with a non-string summary.
    InvalidSuccess,
    /// A failure result without a string message.
    InvalidFailure,
}

impl DetachFailure {
    /// The exact externally observable rejection message.
    pub fn message(&self, actor: &str) -> String {
        match self {
            DetachFailure::NonJson => {
                format!("{actor} returned a non-JSON tool result.")
            }
            DetachFailure::Oversize { max_bytes } => format!(
                "{actor} returned a tool result exceeding the {max_bytes}-byte limit."
            ),
            DetachFailure::InvalidShape => {
                format!("{actor} returned an invalid tool-result shape.")
            }
            DetachFailure::UnknownStatus => {
                format!("{actor} returned an unknown tool-result status.")
            }
            DetachFailure::InvalidSuccess => {
                format!("{actor} returned an invalid success tool result.")
            }
            DetachFailure::InvalidFailure => {
                format!("{actor} returned an invalid failure tool result.")
            }
        }
    }
}

/// Validate, byte-bound, and JSON-detach one tool result at the external
/// boundary.
///
/// Rules: the value must be valid JSON (it already is for
/// 'serde_json::Value'), must be a JSON object, and its serialized
/// UTF-8 size must not exceed 'max_bytes'. A success requires
/// 'status == "success"' with an 'output' and a string 'summary'; a
/// failure requires one of the frozen failure statuses with a string
/// 'message'. Unknown statuses and wrong shapes are rejected, and
/// unknown fields never survive the typed retained result.
///
/// Returns the detached typed result and its serialized byte length.
pub fn detach_bounded_tool_result(
    value: &Value,
    max_bytes: usize,
) -> Result<(ToolExecutionResult, usize), DetachFailure> {
    let serialized = serde_json::to_string(value)
        .expect("serde_json::Value is always serializable");
    let byte_length = serialized.len();
    if byte_length > max_bytes {
        return Err(DetachFailure::Oversize { max_bytes });
    }
    let Some(record) = value.as_object() else {
        return Err(DetachFailure::InvalidShape);
    };
    match record.get("status") {
        Some(Value::String(status)) if status == "success" => {
            let Some(output) = record.get("output") else {
                return Err(DetachFailure::InvalidSuccess);
            };
            let Some(Value::String(summary)) = record.get("summary") else {
                return Err(DetachFailure::InvalidSuccess);
            };
            Ok((
                ToolExecutionResult::Success {
                    output: output.clone(),
                    summary: summary.clone(),
                },
                byte_length,
            ))
        }
        Some(Value::String(status)) => {
            let message = match record.get("message") {
                Some(Value::String(message)) => message.clone(),
                _ => return Err(DetachFailure::InvalidFailure),
            };
            let result = match status.as_str() {
                "invalid_input" => {
                    ToolExecutionResult::InvalidInput { message }
                }
                "denied" => ToolExecutionResult::Denied { message },
                "conflict" => ToolExecutionResult::Conflict { message },
                "failed" => ToolExecutionResult::Failed { message },
                "cancelled" => ToolExecutionResult::Cancelled { message },
                "timed_out" => ToolExecutionResult::TimedOut { message },
                "output_limit" => ToolExecutionResult::OutputLimit { message },
                "sandbox_denied" => {
                    ToolExecutionResult::SandboxDenied { message }
                }
                "sandbox_unavailable" => {
                    ToolExecutionResult::SandboxUnavailable { message }
                }
                "workspace_violation" => {
                    ToolExecutionResult::WorkspaceViolation { message }
                }
                "unavailable" => ToolExecutionResult::Unavailable { message },
                _ => return Err(DetachFailure::UnknownStatus),
            };
            Ok((result, byte_length))
        }
        _ => Err(DetachFailure::UnknownStatus),
    }
}
