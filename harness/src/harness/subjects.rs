//! Corpus-family subject record builders, split by protocol family.
//!
//! Mirrors the family split in `tests/differential/shared/protocol/`:
//! workspace-ops, language-domain, provider-config, context-session,
//! r13-authority, runtime records, and the shared scenario-input shape
//! gate. (The Godot subject arms left with the siralos-godot decoupling,
//! ADR 0037 option A, executed.)

pub(in crate::harness) mod composition;
pub(in crate::harness) mod context_session;
pub(in crate::harness) mod language_domain;
pub(in crate::harness) mod provider_config;
pub(in crate::harness) mod provider_turn;
pub(in crate::harness) mod r13_authority;
pub(in crate::harness) mod r13_reference;
pub(in crate::harness) mod runtime_records;
pub(in crate::harness) mod scenario_input;
pub(in crate::harness) mod task_contract;
pub(in crate::harness) mod ui_components;
pub(in crate::harness) mod workspace_ops;
