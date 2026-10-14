//! CLI-owned composition and input loop for the R7.5 observability slice.
//!
//! The session reads commands synchronously, delegates prompt execution to
//! the existing Host application, and renders only detached projection
//! snapshots. It does not implement projection policy, Tool authorization,
//! persistence, mutation, or an asynchronous runtime.

use std::cell::RefCell;
use std::fmt;
use std::io::{self, BufRead, Write};
use std::path::Path;

use std::collections::{BTreeMap, VecDeque};

use siralos_adapters::lockfile::{LockVerification, verify_workspace_lock};
use siralos_adapters::module::{
    ModuleHost, ModuleHostBounds, ModuleManifest, ModuleRecord, load_manifest,
    load_module_records, verify_component,
};
use siralos_adapters::profile_config::{
    WorkspaceProfileLoad, WorkspaceProfileSnapshot,
    WorkspaceProfileWriteToken, load_workspace_profile_snapshot,
    load_workspace_profile_write_token,
};
use siralos_adapters::provider::{
    HostCredential, HostProvider, replay::RecordedReplayProvider,
};
use siralos_adapters::replay_store::{
    ReplayStoreLoadError, load_replay_store, write_replay_store,
};
use siralos_adapters::skills_loader::{
    SkillCatalogLoad, load_workspace_skills,
};
use siralos_adapters::tool::{
    BashTool, EditTool, ReadTool, WorkspaceListTool, WorkspaceReadTool,
    WorkspaceSearchTool, WriteTool,
};
use siralos_adapters::workspace::fs::{
    BoundedFileRead, is_model_protected_workspace_path,
    read_complete_file_bounded,
};
use siralos_adapters::workspace::resolve::resolve_workspace_path;
use siralos_adapters::workspace::root::{
    WorkspaceRootError, resolve_workspace_root,
};
use siralos_core::composition::lock::{
    LockModuleIdentity, create_workspace_lock,
};
use siralos_core::composition::{
    DeclaredProfile, EffectiveRunPolicy, LockVerificationDecision,
    SkillCatalogState, StoredLockDigest, compose_effective_policy,
    compose_skill_consumption, create_effective_policy_evidence,
    decide_context_control, decide_lock_verification,
    decide_module_activation, declare_profile,
};
use siralos_core::context::ContextPolicy;
use siralos_core::determinism::RetainingReplayRecorder;
use siralos_core::module::capability::HostAuthority;
use siralos_core::module::lifecycle::{
    ActivationRequest, LifecycleState, RuntimeCheckResult,
};
use siralos_core::projection::{
    ProjectionService,
    capacity::ContextCapacity,
    segments::{SegmentInput, Stability},
};
use siralos_core::provider::ConversationItem;
use siralos_core::tool::session::ApplicationProjectionConfig;
use siralos_core::tool::{
    PermissionPolicy, PermissionRule, PolicyRule, SiralosApplication,
    ToolLoopEvent, ToolRegistry, ToolRegistryError,
};
use siralos_core::workspace::path::{
    PathValidationError, validate_relative_path,
};
use std::rc::Rc;

use crate::configuration::{
    ComposedUserConfig, ConfigurationError, DEFAULT_REVIEW_PROVIDER_ID,
    load_user_configuration,
};
use crate::output::{
    format_context_audit, format_context_status, format_module_added,
    format_modules, format_tool_projection, format_tools,
};
use crate::tui::TuiState;
// `EventSource` is imported for its `cancel`: the relay cancels through the
// same seam the shared drain uses, so one request covers both channels.
use crate::sanitize::{TerminalSanitizer, sanitize_for_display};
use crate::session_worker::{
    EventSource, FlushError, FlushOutcome, WorkerCommand, WorkerEvent,
    WorkerGuard, WorkerSource, WorkerWait,
};

// Submodules hold the implementation; this module root keeps the
// public surface at `crate::interactive::*` through explicit
// re-exports below.
mod approval;
mod commands;
mod config;
mod display;
mod model;
mod modules;
mod profile;
mod session;
mod stdio;
mod tui_loop;
mod worker;

use approval::StdioApprovalPrompter;
pub use approval::apply_provider_remove_confirmation;
pub use approval::apply_provider_remove_confirmation_at;
pub use approval::build_approval_modal;
pub use approval::handle_pending_approval_key;
use approval::handle_pending_approval_key_outcome;
pub use approval::handle_tui_mouse;
pub use approval::read_approval_via_input_queue;
pub use approval::shared_evaluate_approval;
pub(crate) use approval::{ApprovalExchange, exchange_resolver};
use commands::SlashCommand;
pub use commands::is_unknown_slash_command;
use commands::parse_slash_command;
use commands::render_evolve_lines;
pub use commands::slash_command_catalog;
use config::ReloadAuthorityEvidence;
#[cfg(test)]
use config::ReloadedConfig;
use config::apply_reloaded_config;
use config::approve_workspace_profile;
use config::declare_and_compose_profile;
use config::effective_profile_protocol;
use config::non_live_state_digest;
use config::reload_authority_is_revalidated;
#[cfg(test)]
use config::reload_report;
use config::reload_report_from_load;
use config::session_host_rules;
use display::ModelListingGate;
use display::compose_skills_segment;
use display::display_endpoint_redacted;
use display::display_field_redacted;
use display::model_listing_diagnostic;
use display::model_listing_gate;
use display::redacted_credential_display;
use display::redacted_credential_token;
use display::render_answer;
use display::render_approval_request;
use display::render_context_segment;
use display::render_model_line;
use display::render_provider_line;
use display::render_provider_line_display;
pub(crate) use display::render_startup_notices;
use display::render_tools_segment;
pub(crate) use display::safe_alias_for_display;
use display::safe_profile_diagnostic;
use display::safe_report_text;
use display::transient_status;
pub use model::InteractiveError;
pub use model::InteractiveOptions;
use model::SIRALOS_SYSTEM_INSTRUCTIONS;
use model::SessionProvider;
use modules::render_activate_module;
use modules::render_add_module;
use modules::render_enable_module;
use modules::render_modules;
use profile::apply_model_switch;
pub use profile::persist_switched_model;
pub use profile::persist_switched_model_at;
use profile::read_profile_bytes_bounded;
pub use profile::remove_profile_config;
pub use profile::remove_profile_config_at;
#[cfg(test)]
use profile::unique_scratch_name;
use profile::verify_session_lock;
pub use profile::write_profile_config;
pub use profile::write_profile_config_at;
pub(crate) use session::SessionComposition;
pub(crate) use session::compose_session;
pub use session::context_audit_session;
use session::drive_context_demand;
use session::flush_record_replay;
pub use stdio::run_interactive_session;
pub use stdio::run_interactive_session_with_options;
pub use stdio::run_interactive_stdio;
#[cfg(test)]
use tui_loop::deliver_tool_approval_verdict;
#[cfg(test)]
use tui_loop::dispatch_tui_command;
#[cfg(test)]
use tui_loop::resume_suspended_turn;
pub use tui_loop::run_interactive_tui_stdio;
pub use tui_loop::run_interactive_tui_with_options;
use worker::Until;
use worker::ask_worker;
use worker::ask_worker_effects;
use worker::await_worker_ready;
use worker::drain_events;
use worker::drain_pending_worker;
use worker::open_model_picker_via_worker;
#[cfg(test)]
use worker::pump_worker;
use worker::relay_suspended_turn;
use worker::spawn_tui_worker;
use worker::switch_model_via_worker;

#[cfg(test)]
mod tests;
