//! TUI shell — pure render model over the live interactive session (T1).
//!
//! The terminal sanitizer stays the single output boundary: every line entering
//! the transcript via [`TuiSink`] is already sanitized by the session (the same
//! `sanitize` code path the stdio frontend uses). The TUI adds no unsanitized
//! content of its own. The input queue stays the single interactive-read owner
//! and the command catalog the vocabulary source; approvals stay host-gated.
//!
//! T3 adds the context pane: for opted-in sessions the decision 100 audit
//! (counters + tick ring) and host-observed tool activity render as a live
//! right-hand pane, single-sourced from the same `ContextMetrics` the
//! `/context` audit segment uses. When the subsystem is off, rendering is
//! byte-identical to T2's (no pane, no placeholder).
//!
//! No threads are used anywhere in the session path: the event loop uses
//! `crossterm::event::poll` with a bounded timeout, blocking provider rounds
//! freeze the redraw (documented T1 limitation). A live spinner would require
//! a UI event-pump thread (recorded as open architecture question, decision 119).

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Widget};

use siralos_adapters::profile_config::WorkspaceProfileWriteToken;

// Submodules hold the implementation; this module root keeps the
// public surface at `crate::tui::*` through explicit
// re-exports below.
mod approval;
mod context_pane;
mod forms;
mod input;
mod limits;
mod panels;
mod render;
mod state;
mod status;
mod terminal;
mod text;

pub use approval::ApprovalDecision;
pub use approval::ApprovalModal;
pub use approval::evaluate_approval_char;
pub use approval::evaluate_approval_input;
pub use approval::handle_modal_key;
pub use approval::modal_key_decision;
pub use context_pane::CONTEXT_COUNTER_ORDER;
pub use context_pane::CONTEXT_PANE_ACTIVITY_LIMIT;
pub use context_pane::CONTEXT_PANE_RING_LIMIT;
pub use context_pane::CONTEXT_PANE_WIDTH;
pub use context_pane::ContextPaneData;
pub use context_pane::ToolActivityEntry;
pub use context_pane::build_context_pane;
pub use context_pane::context_counters;
pub use context_pane::context_pane_lines;
pub use context_pane::context_ring_tail;
pub use context_pane::format_tick_record_line;
pub use context_pane::tool_activity_from_history;
pub use forms::ModelPicker;
pub use forms::ProtocolPicker;
pub use forms::ProviderAddData;
pub use forms::ProviderAddField;
pub use forms::ProviderAddForm;
pub use forms::ProviderEntry;
pub use forms::ProviderPicker;
pub use forms::UiSpecData;
pub use forms::UiSpecField;
pub use forms::UiSpecForm;
pub use forms::derive_provider_name;
pub use forms::host_from_endpoint;
pub use forms::model_picker_lines;
pub use forms::model_picker_window;
use forms::normalize_credential_input;
use forms::open_protocol_picker;
pub use forms::provider_entries_from_session;
use forms::redacted_form_credential;
use forms::safe_display_label;
use forms::safe_form_endpoint_edit;
use forms::safe_form_endpoint_for_credential;
use forms::safe_form_message;
use forms::safe_form_message_for_credential;
use forms::safe_model_id;
use forms::validate_api_protocol;
use forms::validate_endpoint_value;
use forms::validate_model_display_name;
use forms::validate_model_name;
use forms::validate_provider_name;
pub use input::accept_submitted_input;
pub use input::apply_turn_key;
pub use input::handle_key;
pub use input::handle_mouse;
pub use input::toggle_mouse_capture;
pub use limits::APPROVAL_TRUNCATION_MARKER;
pub use limits::CONTEXT_BUDGET_TOKENS;
pub use limits::MAX_APPROVAL_LINES;
pub use limits::MAX_INPUT_BYTES;
pub use limits::MAX_REASONING_BUFFER_BYTES;
pub use limits::MAX_STREAM_BYTES;
pub use limits::MAX_TRANSCRIPT_LINE_BYTES;
pub use limits::MAX_TRANSCRIPT_LINES;
pub use limits::MOUSE_CAPTURE_OFF_MESSAGE;
pub use limits::MOUSE_CAPTURE_ON_MESSAGE;
pub use limits::MOUSE_STDIO_MESSAGE;
pub use limits::MOUSE_WHEEL_STEP;
pub use limits::REASONING_BYTES;
pub use limits::REASONING_ROWS;
pub use limits::REDRAW_INTERVAL;
pub use limits::SIRALOS_BANNER;
pub use limits::SIRALOS_GREETING;
pub use limits::THINKING_TAIL_CHARS;
pub use limits::TUI_DRAIN_POLL;
pub use limits::TUI_IDLE_POLL;
pub use limits::mouse_capture_message;
pub use limits::paint_interval;
pub use panels::EMPTY_CELL_TITLE;
pub use panels::GRID_TAB_LABEL;
pub use panels::PANELS_STATUS_HINT;
pub use panels::UiPanels;
pub use panels::cycle_panels_tab;
pub use panels::draw_panels;
pub use panels::draw_panels_snapshot;
pub use panels::render_panels_to_buffer;
pub use panels::sanitize_panel_line;
pub use panels::starter_feed;
pub use panels::starter_specs;
#[cfg(test)]
use render::FrameRows;
pub use render::draw;
pub use render::draw_with_pane;
#[cfg(test)]
use render::provider_add_form_lines;
pub use render::render_to_buffer;
pub use render::render_to_buffer_with_pane;
pub use render::style_for_transcript_line;
#[cfg(test)]
use render::visible_transcript_rows;
pub use state::TranscriptEntry;
pub use state::TuiState;
pub use state::command_catalog;
pub use state::open_model_switch_picker;
pub use state::open_provider_add_form;
pub use state::open_provider_picker;
pub use state::open_provider_remove_confirm;
pub use state::push_banner_and_greeting;
pub use status::WORKING_PULSE;
pub use status::append_context_usage;
pub use status::compose_status_line;
pub use status::compose_status_line_with_context;
pub use status::context_assembled_total;
pub use status::header_text;
pub use status::is_working_status;
pub use status::local_timestamp_from_millis_with_offset;
pub use status::local_timestamp_now;
pub use status::utc_timestamp_from_millis;
pub use status::utc_timestamp_now;
pub use status::working_line;
pub use terminal::TerminalGuard;
pub use terminal::TuiSink;
pub use terminal::should_launch_tui;
pub use terminal::should_use_tui;
pub use terminal::stdout_is_tty;
use text::display_width;
use text::truncate_to_width;
pub use text::wrap_line_to_width;

#[cfg(test)]
mod tests;
