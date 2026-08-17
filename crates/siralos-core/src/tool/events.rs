//! Tool-loop event set (R7.2 surface + R7.3 context-pressure).

use crate::tool::display_input::DisplayInput;

/// One generic Tool-loop observable.
///
/// The R7.2 surface is closed; R7.3 adds the UI-neutral
/// `ContextPressure` event. Later milestones own approval, checkpoint,
/// command, and optional-domain events; none of those variants exist here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolLoopEvent {
    /// A prompt response has started.
    ResponseStarted,
    /// One provider text delta, replayed in provider order.
    TextDelta {
        /// The delta text.
        text: String,
    },
    /// The prompt completed successfully.
    ResponseCompleted,
    /// The prompt was cancelled by the Host.
    ResponseCancelled,
    /// The prompt failed terminally.
    ResponseFailed {
        /// The exact externally observable failure message.
        message: String,
    },
    /// A retained executable tool call has entered the Tool Round.
    ToolStarted {
        /// Correlation id of the call.
        call_id: String,
        /// Name of the requested tool.
        tool_name: String,
        /// Truncated displayInput with JavaScript UTF-16 semantics.
        display_input: DisplayInput,
    },
    /// A tool call completed successfully.
    ToolCompleted {
        /// Correlation id of the call.
        call_id: String,
        /// Name of the executed tool.
        tool_name: String,
        /// The tool's bounded summary.
        summary: String,
    },
    /// A tool call produced any ordinary failure result, or was denied
    /// by a Host gate before execution.
    ToolFailed {
        /// Correlation id of the call.
        call_id: String,
        /// Name of the requested tool.
        tool_name: String,
        /// The exact externally observable failure message.
        message: String,
    },
    /// A tool call returned `cancelled`.
    ToolCancelled {
        /// Correlation id of the call.
        call_id: String,
        /// Name of the requested tool.
        tool_name: String,
    },
    /// Context pressure classified for the current turn (R7.3).
    ///
    /// Emitted on the Host application boundary when the final
    /// classified pressure is non-normal; `hard` is followed by
    /// `ResponseFailed` without a provider call.
    ContextPressure {
        /// Final classified state (`warn`/`auto`/`hard`).
        state: String,
        /// Estimated tokens that produced the pressure.
        estimated_tokens: usize,
        /// Working maximum used for classification.
        working_maximum: i64,
    },
}

impl ToolLoopEvent {
    /// Whether this event is terminal for the current prompt.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::ResponseCompleted
                | Self::ResponseCancelled
                | Self::ResponseFailed { .. }
        )
    }
}
