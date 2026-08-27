//! Authoritative slash-command catalog surface (Stage 3R R13.1).
//!
//! Mirrors the TypeScript reference catalog: host-owned, immutable, in
//! registration order, with a content-bound revision digest. Providers
//! and projects can never register commands.

use crate::identity::{canonicalize_json, sha256_hex_str};
use serde_json::json;

/// The `session` catalog group.
pub const COMMAND_CATALOG_GROUP_SESSION: &str = "session";
/// The `inspection` catalog group.
pub const COMMAND_CATALOG_GROUP_INSPECTION: &str = "inspection";
/// The `workspace` catalog group.
pub const COMMAND_CATALOG_GROUP_WORKSPACE: &str = "workspace";
/// The `workflow` catalog group.
pub const COMMAND_CATALOG_GROUP_WORKFLOW: &str = "workflow";
/// The `godot` catalog group.
pub const COMMAND_CATALOG_GROUP_GODOT: &str = "godot";
/// The `knowledge` catalog group.
pub const COMMAND_CATALOG_GROUP_KNOWLEDGE: &str = "knowledge";
/// The `doctor` catalog group.
pub const COMMAND_CATALOG_GROUP_DOCTOR: &str = "doctor";

/// One catalogued command: id, human description, and group.
#[derive(Debug, Clone, Copy)]
pub struct CommandCatalogEntry {
    /// The stable command id.
    pub id: &'static str,
    /// The human-facing description rendered by `/help`.
    pub description: &'static str,
    /// The catalog group this command belongs to.
    pub group: &'static str,
}

/// The single source of truth for the interactive command surface.
pub const COMMAND_CATALOG: [CommandCatalogEntry; 47] = [
    CommandCatalogEntry {
        id: "help",
        description: "Show this help",
        group: COMMAND_CATALOG_GROUP_SESSION,
    },
    CommandCatalogEntry {
        id: "status",
        description: "Show provider, session, and workspace status",
        group: COMMAND_CATALOG_GROUP_SESSION,
    },
    CommandCatalogEntry {
        id: "clear",
        description: "Clear the terminal (conversation is kept)",
        group: COMMAND_CATALOG_GROUP_SESSION,
    },
    CommandCatalogEntry {
        id: "exit",
        description: "Close Siralos",
        group: COMMAND_CATALOG_GROUP_SESSION,
    },
    CommandCatalogEntry {
        id: "tools",
        description: "List the available tools",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "sandbox",
        description: "Show the sandbox backend status",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "permissions",
        description: "Show capability rules",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "commands",
        description: "Show command runners and command status",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "context",
        description: "Show the projected context (stable/contextual/volatile, pressure)",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "instructions",
        description: "Show discovered project instruction files with revisions",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "knowledge",
        description: "Show current project knowledge facts (/knowledge why: last retrieval trace)",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "references",
        description: "Show configured external references and their status",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "reference",
        description: "Show one reference's identity and availability",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "research-status",
        description: "Show research capability, sources, and recent evidence",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "development-status",
        description: "Show the active development workflow's bounded status",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "brief",
        description: "Show the compiled executor brief for the current task (task goal, manifest/contract identity, touchpoints, invariants, acceptance ids)",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "milestone",
        description: "Show the current milestone manifest and its evidence-backed acceptance status",
        group: COMMAND_CATALOG_GROUP_INSPECTION,
    },
    CommandCatalogEntry {
        id: "git-status",
        description: "Show Git availability and repository status",
        group: COMMAND_CATALOG_GROUP_WORKSPACE,
    },
    CommandCatalogEntry {
        id: "diff",
        description: "Show a bounded Git diff (working, staged, or head)",
        group: COMMAND_CATALOG_GROUP_WORKSPACE,
    },
    CommandCatalogEntry {
        id: "checkpoints",
        description: "List recorded recovery checkpoints",
        group: COMMAND_CATALOG_GROUP_WORKSPACE,
    },
    CommandCatalogEntry {
        id: "undo",
        description: "Undo the latest Siralos mutation (or /undo <checkpoint-id>)",
        group: COMMAND_CATALOG_GROUP_WORKSPACE,
    },
    CommandCatalogEntry {
        id: "cancel",
        description: "Cancel the running command",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "task",
        description: "Start a host-owned ad-hoc task (completion requires host verification)",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "task-status",
        description: "Show the current task: phase, contract revision, criteria, steps, progress",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "develop",
        description: "Start one GDScript development workflow (host-controlled planning; one-time approval; each source change is approved separately; /develop --plan <request> forces full planning before execution)",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "plan",
        description: "Plan-only mode: run read-only planning for a request and return a structured plan; no source is modified, no mutation approval is requested, and no execution follows",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "quality",
        description: "Show the current or final development quality report",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "review-change",
        description: "Run a fresh read-only independent review of the current development change (no approval, no modifications)",
        group: COMMAND_CATALOG_GROUP_WORKFLOW,
    },
    CommandCatalogEntry {
        id: "godot",
        description: "Show the selected Godot installation and project compatibility",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-installations",
        description: "Show all discovered Godot installations and selection rationale",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-project",
        description: "Show the static Godot project profile",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-doctor",
        description: "Run bounded Godot diagnostics",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-probe",
        description: "Prepare one recovery-mode Godot project probe (approval required; reports unavailable when the platform cannot bind execution)",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-probe-status",
        description: "Show the recovery probe capability and last outcome",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-knowledge",
        description: "Show the exact-engine API knowledge status",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-knowledge-refresh",
        description: "Regenerate the exact-engine API knowledge profile (reports unavailable when the platform cannot bind execution)",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "godot-api",
        description: "Search the exact engine's API documentation locally",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-check",
        description: "Check one .gd script with --check-only (approval required)",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-diagnostics",
        description: "Check the project's .gd scripts sequentially with --check-only (approval required)",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-lsp",
        description: "Start (approval required) or show the Godot GDScript language session",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-lsp-stop",
        description: "Gracefully stop the language session (no approval needed)",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-hover",
        description: "Hover information from the language session",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-complete",
        description: "Completion candidates from the language session",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "gdscript-definition",
        description: "Definition locations from the language session",
        group: COMMAND_CATALOG_GROUP_GODOT,
    },
    CommandCatalogEntry {
        id: "read-structure",
        description: "Show the GDScript declaration structure of a workspace file",
        group: COMMAND_CATALOG_GROUP_KNOWLEDGE,
    },
    CommandCatalogEntry {
        id: "doctor",
        description: "Run read-only Siralos capability diagnostics (areas: runtime, configuration, providers, sandbox, workspace, godot, project, references, research, capabilities)",
        group: COMMAND_CATALOG_GROUP_DOCTOR,
    },
    CommandCatalogEntry {
        id: "siralos",
        description: "Show the installed Siralos runtime identity and self-reference revision",
        group: COMMAND_CATALOG_GROUP_DOCTOR,
    },
];

/// Stable revision of the command surface, for self-reference fingerprints.
pub fn command_catalog_revision() -> String {
    let entries: Vec<serde_json::Value> = COMMAND_CATALOG
        .iter()
        .map(|entry| json!({ "id": entry.id, "description": entry.description }))
        .collect();
    sha256_hex_str(&canonicalize_json(&serde_json::Value::Array(entries)))
}

/// Look up one catalog entry by exact id.
pub fn catalog_entry(id: &str) -> Option<&'static CommandCatalogEntry> {
    COMMAND_CATALOG.iter().find(|entry| entry.id == id)
}

/// All catalog ids in registration order.
pub fn command_catalog_ids() -> Vec<&'static str> {
    COMMAND_CATALOG.iter().map(|entry| entry.id).collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_shape_is_stable() {
        assert_eq!(COMMAND_CATALOG.len(), 47);
        assert_eq!(command_catalog_ids()[0], "help");
        assert_eq!(command_catalog_ids()[46], "siralos");
        assert_eq!(
            command_catalog_ids().len(),
            command_catalog_ids()
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "catalog ids must be unique"
        );
    }

    #[test]
    fn revision_digest_is_stable_and_content_bound() {
        let first = command_catalog_revision();
        assert_eq!(first, command_catalog_revision());
        // The revision binds id+description pairs, so a different surface
        // hashes differently.
        let alternative = sha256_hex_str(&canonicalize_json(&json!([
            { "id": "help", "description": "Different help" }
        ])));
        assert_ne!(first, alternative);
    }

    #[test]
    fn lookups_are_exact() {
        let entry = catalog_entry("help").expect("catalogued");
        assert_eq!(entry.description, "Show this help");
        assert_eq!(entry.group, COMMAND_CATALOG_GROUP_SESSION);
        assert!(catalog_entry("definitely-not-a-command").is_none());
    }
}
