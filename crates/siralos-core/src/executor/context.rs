//! Architecture context, documentation selection, new-file discipline,
//! and the executor context pack (executor briefing foundation,
//! ADR 0023–0024; Stage 3R R13.4).
//!
//! All selection is deterministic — path/domain mapping, never semantic
//! search. The pack is DERIVED context and never grants capability; the
//! index inputs are documentation metadata, not runtime policy.

use crate::executor::scope::{ActiveWorkingSet, WorkspaceScope};

// ---------------------------------------------------------------------------
// Architecture context.
// ---------------------------------------------------------------------------

/// One architecture-index entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureContextEntry {
    /// Stable entry id.
    pub id: String,
    /// Repository-relative doc path.
    pub path: String,
    /// Deterministic concern tags this entry covers.
    pub concerns: Vec<String>,
}

/// One selected architecture document reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchitectureContextRef {
    /// Entry id.
    pub id: String,
    /// Doc path.
    pub path: String,
}

/// Deterministic selection in canonical index order, bounded.
pub fn select_architecture_context(
    concerns: &[String],
    index: &[ArchitectureContextEntry],
    max_entries: usize,
) -> Vec<ArchitectureContextRef> {
    let mut selected = Vec::new();
    for entry in index {
        if selected.len() >= max_entries {
            break;
        }
        if entry.concerns.iter().any(|concern| concerns.contains(concern)) {
            selected.push(ArchitectureContextRef {
                id: entry.id.clone(),
                path: entry.path.clone(),
            });
        }
    }
    selected
}

// ---------------------------------------------------------------------------
// Documentation context.
// ---------------------------------------------------------------------------

/// Documentation entry kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentationKind {
    /// Root AGENTS.md.
    RootAgents,
    /// Nested scoped AGENTS.md.
    NestedAgents,
    /// Architecture document.
    Architecture,
    /// ADR.
    Adr,
    /// Development doc.
    Development,
}

impl DocumentationKind {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentationKind::RootAgents => "root-agents",
            DocumentationKind::NestedAgents => "nested-agents",
            DocumentationKind::Architecture => "architecture",
            DocumentationKind::Adr => "adr",
            DocumentationKind::Development => "development",
        }
    }
}

/// Documentation lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentationStatus {
    /// Accepted.
    Accepted,
    /// Superseded by another entry.
    Superseded,
    /// Deprecated.
    Deprecated,
}

impl DocumentationStatus {
    /// Stable machine-readable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            DocumentationStatus::Accepted => "accepted",
            DocumentationStatus::Superseded => "superseded",
            DocumentationStatus::Deprecated => "deprecated",
        }
    }
}

/// One documentation-index entry (metadata only, never policy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationEntry {
    /// Stable entry id.
    pub id: String,
    /// Repository-relative doc path.
    pub path: String,
    /// Kind.
    pub kind: DocumentationKind,
    /// Concern tags this entry covers (selection key).
    pub concerns: Vec<String>,
    /// Lifecycle status.
    pub status: DocumentationStatus,
    /// Source-path globs this entry is scoped to.
    pub paths: Vec<String>,
}

/// One deterministic documentation selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentationSelection {
    /// Always-selected root guidance.
    pub root_agents: Vec<String>,
    /// Path-scoped nested guidance.
    pub nested_agents: Vec<String>,
    /// Concern-matched architecture docs.
    pub architecture_docs: Vec<String>,
    /// Concern-ranked accepted ADRs.
    pub adrs: Vec<String>,
    /// Concern-matched development docs.
    pub development_docs: Vec<String>,
    /// Entries dropped by the budget (`kind:path`).
    pub dropped: Vec<String>,
}

/// Host-owned documentation budget.
pub struct DocumentationBudget;

impl DocumentationBudget {
    /// Maximum nested AGENTS.md files.
    pub const MAX_NESTED_AGENTS: usize = 4;
    /// Maximum architecture docs.
    pub const MAX_ARCHITECTURE_DOCS: usize = 2;
    /// Maximum ADRs.
    pub const MAX_ADRS: usize = 4;
    /// Maximum development docs.
    pub const MAX_DEVELOPMENT_DOCS: usize = 2;
    /// Maximum total selection.
    pub const MAX_SELECTED: usize = 12;
}

/// Archived documentation is excluded from ordinary selection.
pub const ARCHIVE_DOCUMENTATION_PREFIX: &str = "docs/archive/";

/// Whether a path is archived documentation.
pub fn is_archived_documentation_path(path: &str) -> bool {
    path.starts_with(ARCHIVE_DOCUMENTATION_PREFIX)
        || path.starts_with(&format!("./{ARCHIVE_DOCUMENTATION_PREFIX}"))
}

/// Selection input.
pub struct SelectDocumentationContextInput<'a> {
    /// Requested concern tags.
    pub concerns: &'a [String],
    /// Task-relevant paths for nested-AGENTS scoping.
    pub paths: &'a [String],
    /// Injected documentation index.
    pub index: &'a [DocumentationEntry],
}

/// Deterministic selection in canonical order with budget drops recorded.
pub fn select_documentation_context(
    input: &SelectDocumentationContextInput<'_>,
) -> DocumentationSelection {
    let index = input.index;
    let wanted = input.concerns;
    let task_paths = input.paths;
    let collect =
        |kind: DocumentationKind, concern_filter: bool| -> Vec<String> {
            let mut paths = Vec::new();
            for entry in index {
                if entry.kind != kind {
                    continue;
                }
                if entry.status != DocumentationStatus::Accepted
                    || is_archived_documentation_path(&entry.path)
                {
                    continue;
                }
                if concern_filter
                    && !entry
                        .concerns
                        .iter()
                        .any(|concern| wanted.contains(concern))
                {
                    continue;
                }
                paths.push(entry.path.clone());
            }
            paths
        };
    let root_agents = collect(DocumentationKind::RootAgents, false);
    let nested_all = collect(DocumentationKind::NestedAgents, false);
    let nested_agents: Vec<String> = nested_all
        .into_iter()
        .filter(|path| {
            let patterns = index
                .iter()
                .find(|candidate| candidate.path == *path)
                .map(|entry| entry.paths.clone())
                .unwrap_or_default();
            patterns.is_empty()
                || task_paths.iter().any(|task_path| {
                    patterns.iter().any(|pattern| {
                        crate::executor::new_files::path_matches_pattern(
                            task_path, pattern,
                        )
                    })
                })
        })
        .collect();
    let architecture_docs = collect(DocumentationKind::Architecture, true);
    // ADR candidates ordered by concern overlap (most-specific first);
    // ties keep the canonical index order via stable sort.
    let adr_candidates: Vec<(&DocumentationEntry, usize)> = index
        .iter()
        .filter(|entry| {
            entry.kind == DocumentationKind::Adr
                && entry.status == DocumentationStatus::Accepted
                && !is_archived_documentation_path(&entry.path)
                && entry
                    .concerns
                    .iter()
                    .any(|concern| wanted.contains(concern))
        })
        .map(|entry| {
            (
                entry,
                entry
                    .concerns
                    .iter()
                    .filter(|concern| wanted.contains(*concern))
                    .count(),
            )
        })
        .collect();
    let mut adr_ranked = adr_candidates;
    adr_ranked.sort_by_key(|(_, overlap)| std::cmp::Reverse(*overlap));
    let adr_ordered: Vec<String> =
        adr_ranked.into_iter().map(|(entry, _)| entry.path.clone()).collect();
    let development_docs = collect(DocumentationKind::Development, true);
    let mut dropped = Vec::new();
    let drop = |list: &mut Vec<String>,
                max: usize,
                list_name: &str,
                dropped: &mut Vec<String>| {
        if list.len() > max {
            dropped.extend(
                list[max..].iter().map(|path| format!("{list_name}:{path}")),
            );
            list.truncate(max);
        }
    };
    let mut bounded_nested = nested_agents;
    drop(
        &mut bounded_nested,
        DocumentationBudget::MAX_NESTED_AGENTS,
        "nested",
        &mut dropped,
    );
    let mut bounded_architecture = architecture_docs;
    drop(
        &mut bounded_architecture,
        DocumentationBudget::MAX_ARCHITECTURE_DOCS,
        "architecture",
        &mut dropped,
    );
    let mut bounded_adrs = adr_ordered;
    drop(
        &mut bounded_adrs,
        DocumentationBudget::MAX_ADRS,
        "adr",
        &mut dropped,
    );
    let mut bounded_development = development_docs;
    drop(
        &mut bounded_development,
        DocumentationBudget::MAX_DEVELOPMENT_DOCS,
        "development",
        &mut dropped,
    );
    DocumentationSelection {
        root_agents,
        nested_agents: bounded_nested,
        architecture_docs: bounded_architecture,
        adrs: bounded_adrs,
        development_docs: bounded_development,
        dropped,
    }
}

// ---------------------------------------------------------------------------
// Context pack.
// ---------------------------------------------------------------------------

/// Task contract reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlanRefId {
    /// Contract id.
    pub id: String,
    /// Contract revision.
    pub revision: u64,
}

/// Plan reference embedded in the pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPlanRef {
    /// Plan id.
    pub id: String,
    /// Plan revision.
    pub revision: u64,
    /// Approval state spelling ("none"|"approved"|"invalidated").
    pub approval: String,
}

/// Instruction reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionRef {
    /// Source label `kind:path` or `kind:.`.
    pub source: String,
    /// Bounded rendered instruction text.
    pub summary: String,
}

/// Touchpoint reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchpointRef {
    /// Touchpoint id.
    pub id: String,
    /// Touchpoint path.
    pub path: String,
    /// Confidence spelling.
    pub confidence: String,
}

/// Capability area states reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRef {
    /// True when a capability snapshot was available at build time.
    pub available: bool,
    /// Per-area state summaries; empty when no snapshot was available.
    pub states: Vec<(String, String)>,
}

/// Acceptance requirement reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceRequirementPackRef {
    /// Requirement id.
    pub id: String,
    /// Description.
    pub description: String,
    /// Optional linked criterion.
    pub criterion_id: Option<String>,
}

/// Bounded workspace-scope references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceScopeRef {
    /// Verified file references.
    pub verified_files: Vec<(String, Option<String>, Option<String>)>,
    /// Candidate paths only — candidate contents never enter context.
    pub candidate_files: Vec<String>,
    /// Promotion references.
    pub promotions: Vec<(String, String)>,
}

/// Bounded working-set reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkingSetRef {
    /// Step id.
    pub step_id: String,
    /// File references (path, reason, view).
    pub files: Vec<(String, String, String)>,
}

/// Deterministic review-signal reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSignalRef {
    /// Signal id.
    pub id: String,
    /// Message.
    pub message: String,
}

/// New-production-file rationale reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFileRef {
    /// New-file path.
    pub path: String,
    /// Recorded reason.
    pub reason: String,
    /// Existing owner modules inspected before creating the file.
    pub existing_owners_inspected: Vec<String>,
}

/// Derived context pack for one executor invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorContextPack {
    /// Task identity.
    pub task: TaskPlanRefId,
    /// Current plan reference, when any.
    pub plan: Option<TaskPlanRef>,
    /// Execution-contract reference.
    pub execution_contract: crate::executor::contracts::ExecutionContractRef,
    /// Milestone manifest reference, when any.
    pub milestone: Option<crate::executor::milestone::MilestoneManifestRef>,
    /// Instruction references.
    pub instructions: Vec<InstructionRef>,
    /// Selected architecture references.
    pub architecture: Vec<ArchitectureContextRef>,
    /// Verified touchpoints.
    pub verified_touchpoints: Vec<TouchpointRef>,
    /// Candidate touchpoints.
    pub candidate_touchpoints: Vec<TouchpointRef>,
    /// Capability summary.
    pub capabilities: CapabilityRef,
    /// Unresolved findings (bounded).
    pub unresolved_findings: Vec<crate::task::FindingRef>,
    /// Acceptance requirement refs from the milestone.
    pub acceptance: Vec<AcceptanceRequirementPackRef>,
    /// Derived workspace scope, when provided.
    pub workspace_scope: Option<WorkspaceScopeRef>,
    /// Current-step working set, when provided.
    pub active_working_set: Option<ActiveWorkingSetRef>,
    /// Deterministically selected documentation.
    pub documentation: Option<DocumentationSelection>,
    /// Review signals.
    pub scope_signals: Option<Vec<ScopeSignalRef>>,
    /// Recorded new-file rationales.
    pub new_files: Option<Vec<NewFileRef>>,
}

/// Pack hard bounds.
pub struct ExecutorContextPackLimits;

impl ExecutorContextPackLimits {
    /// Maximum instructions.
    pub const MAX_INSTRUCTIONS: usize = 8;
    /// Maximum instruction-summary bytes.
    pub const MAX_INSTRUCTION_SUMMARY_BYTES: usize = 1024;
    /// Maximum architecture entries.
    pub const MAX_ARCHITECTURE_ENTRIES: usize = 4;
    /// Maximum findings.
    pub const MAX_FINDINGS: usize = 16;
    /// Maximum acceptance entries.
    pub const MAX_ACCEPTANCE: usize = 32;
    /// Maximum workspace verified files.
    pub const MAX_WORKSPACE_VERIFIED_FILES: usize = 12;
    /// Maximum workspace candidate files.
    pub const MAX_WORKSPACE_CANDIDATE_FILES: usize = 12;
    /// Maximum working-set files.
    pub const MAX_WORKING_SET_FILES: usize = 8;
    /// Maximum scope signals.
    pub const MAX_SCOPE_SIGNALS: usize = 8;
    /// Maximum new files.
    pub const MAX_NEW_FILES: usize = 8;
    /// Maximum documentation entries per list.
    pub const MAX_DOCUMENTATION_ENTRIES: usize = 12;
}

/// Lightweight instruction input mirroring the fields the pack consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionLite {
    /// Instruction content.
    pub content: String,
    /// Source kind label.
    pub source_kind: String,
    /// Source path, when present.
    pub source_path: Option<String>,
    /// Scope path, when present.
    pub scope_path: Option<String>,
}

/// Lightweight capability-snapshot view carrying exactly the fields the
/// projection consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityAreasSnapshot {
    /// Provider states (first one wins).
    pub providers: Vec<String>,
    /// Sandbox state.
    pub sandbox: String,
    /// Workspace state.
    pub workspace: String,
    /// Godot state.
    pub godot: String,
    /// References state.
    pub references: String,
    /// Research state.
    pub research: String,
    /// Tools state.
    pub tools: String,
}

/// Build input for [`build_executor_context_pack`].
pub struct BuildExecutorContextPackInput<'a> {
    /// The task contract.
    pub contract: &'a crate::task::TaskContract,
    /// The current plan, when any.
    pub plan: Option<&'a crate::planning::TaskPlan>,
    /// Execution-contract reference.
    pub execution_contract: crate::executor::contracts::ExecutionContractRef,
    /// Milestone manifest, when any.
    pub milestone: Option<&'a crate::executor::milestone::MilestoneManifest>,
    /// Resolved instructions.
    pub instructions: &'a [InstructionLite],
    /// Explicit architecture concerns override.
    pub architecture_concerns: Option<&'a [String]>,
    /// Architecture index override.
    pub architecture_index: &'a [ArchitectureContextEntry],
    /// Derived workspace scope.
    pub workspace_scope: Option<&'a WorkspaceScope>,
    /// Current-step working set.
    pub active_working_set: Option<&'a ActiveWorkingSet>,
    /// Documentation index override.
    pub documentation_index: &'a [DocumentationEntry],
    /// Paths used to scope nested AGENTS.md selection.
    pub documentation_paths: Option<&'a [String]>,
    /// Review signals.
    pub scope_signals: Option<&'a [ScopeSignalRef]>,
    /// New-file rationales.
    pub new_files: Option<&'a [NewFileRef]>,
    /// Restrict capability guidance to these areas.
    pub capability_areas: Option<&'a [String]>,
    /// Capability snapshot.
    pub capability_snapshot: Option<&'a CapabilityAreasSnapshot>,
    /// Findings.
    pub findings: &'a [crate::task::FindingRef],
    /// Approval state of the current plan revision.
    pub plan_approval: Option<&'a str>,
}

fn instruction_refs(instructions: &[InstructionLite]) -> Vec<InstructionRef> {
    let mut refs = Vec::new();
    for instruction in instructions {
        if refs.len() >= ExecutorContextPackLimits::MAX_INSTRUCTIONS {
            break;
        }
        let summary = instruction.content.trim();
        let rendered = if summary.len()
            > ExecutorContextPackLimits::MAX_INSTRUCTION_SUMMARY_BYTES
        {
            format!("{}\u{2026}", &summary[..512.min(summary.len())])
        } else {
            summary.to_owned()
        };
        let source_label = instruction
            .source_path
            .clone()
            .unwrap_or_else(|| instruction.source_kind.clone());
        let scope_label =
            instruction.scope_path.clone().unwrap_or_else(|| ".".to_owned());
        refs.push(InstructionRef {
            source: format!("{source_label}:{scope_label}"),
            summary: rendered,
        });
    }
    refs
}

fn touchpoint_refs(
    plan: Option<&crate::planning::TaskPlan>,
    confidence: crate::planning::TouchpointConfidence,
) -> Vec<TouchpointRef> {
    let Some(plan) = plan else {
        return Vec::new();
    };
    plan.content
        .touchpoints
        .iter()
        .filter(|touchpoint| touchpoint.confidence == confidence)
        .map(|touchpoint| TouchpointRef {
            id: touchpoint.id.clone(),
            path: touchpoint.path.clone(),
            confidence: touchpoint.confidence.as_str().to_owned(),
        })
        .collect()
}

fn capability_ref(
    snapshot: Option<&CapabilityAreasSnapshot>,
    areas: Option<&[String]>,
) -> CapabilityRef {
    let Some(snapshot) = snapshot else {
        return CapabilityRef { available: false, states: Vec::new() };
    };
    let all_areas = vec![
        (
            "providers".to_owned(),
            snapshot
                .providers
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_owned()),
        ),
        ("sandbox".to_owned(), snapshot.sandbox.clone()),
        ("workspace".to_owned(), snapshot.workspace.clone()),
        ("godot".to_owned(), snapshot.godot.clone()),
        ("references".to_owned(), snapshot.references.clone()),
        ("research".to_owned(), snapshot.research.clone()),
        ("tools".to_owned(), snapshot.tools.clone()),
    ];
    let states = match areas {
        Some(areas) => all_areas
            .into_iter()
            .filter(|(area, _)| areas.contains(area))
            .collect(),
        None => all_areas,
    };
    CapabilityRef { available: true, states }
}

fn truncate_bounded(text: &str, max_bytes: usize, cut: usize) -> String {
    if text.len() > max_bytes {
        format!("{}\u{2026}", &text[..cut.min(text.len())])
    } else {
        text.trim().to_owned()
    }
}

fn workspace_scope_ref(scope: &WorkspaceScope) -> WorkspaceScopeRef {
    WorkspaceScopeRef {
        verified_files: scope
            .verified_files
            .iter()
            .take(ExecutorContextPackLimits::MAX_WORKSPACE_VERIFIED_FILES)
            .map(|file| {
                (
                    file.path.clone(),
                    file.revision.clone(),
                    file.evidence.clone(),
                )
            })
            .collect(),
        candidate_files: scope
            .candidate_files
            .iter()
            .take(ExecutorContextPackLimits::MAX_WORKSPACE_CANDIDATE_FILES)
            .map(|file| file.path.clone())
            .collect(),
        promotions: scope
            .promotions
            .iter()
            .map(|record| (record.path.clone(), record.evidence.clone()))
            .collect(),
    }
}

fn active_working_set_ref(set: &ActiveWorkingSet) -> ActiveWorkingSetRef {
    ActiveWorkingSetRef {
        step_id: set.step_id.clone(),
        files: set
            .files
            .iter()
            .take(ExecutorContextPackLimits::MAX_WORKING_SET_FILES)
            .map(|file| {
                (
                    file.path.clone(),
                    file.reason.as_str().to_owned(),
                    file.view.as_str().to_owned(),
                )
            })
            .collect(),
    }
}

/// Build the derived context pack for one task. Deterministic: identical
/// inputs produce identical packs.
#[allow(clippy::too_many_lines)]
pub fn build_executor_context_pack(
    input: &BuildExecutorContextPackInput<'_>,
) -> ExecutorContextPack {
    let concerns: Vec<String> = input
        .architecture_concerns
        .map(<[String]>::to_vec)
        .or_else(|| {
            input
                .milestone
                .map(|manifest| manifest.architecture_concerns.clone())
        })
        .unwrap_or_default();
    let architecture = select_architecture_context(
        &concerns,
        input.architecture_index,
        ExecutorContextPackLimits::MAX_ARCHITECTURE_ENTRIES,
    );
    let documentation_paths: Vec<String> = input
        .documentation_paths
        .map(<[String]>::to_vec)
        .or_else(|| {
            input.plan.map(|plan| {
                plan.content
                    .touchpoints
                    .iter()
                    .filter(|touchpoint| {
                        touchpoint.confidence
                            == crate::planning::TouchpointConfidence::Verified
                    })
                    .map(|touchpoint| touchpoint.path.clone())
                    .collect()
            })
        })
        .unwrap_or_default();
    let documentation =
        select_documentation_context(&SelectDocumentationContextInput {
            concerns: &concerns,
            paths: &documentation_paths,
            index: input.documentation_index,
        });
    let findings: Vec<crate::task::FindingRef> = input
        .findings
        .iter()
        .take(ExecutorContextPackLimits::MAX_FINDINGS)
        .cloned()
        .collect();
    let plan_ref = input.plan.map(|plan| TaskPlanRef {
        id: plan.id.clone(),
        revision: plan.revision,
        approval: input.plan_approval.unwrap_or("none").to_owned(),
    });
    let instructions = instruction_refs(input.instructions);
    let verified_touchpoints = touchpoint_refs(
        input.plan,
        crate::planning::TouchpointConfidence::Verified,
    );
    let candidate_touchpoints = touchpoint_refs(
        input.plan,
        crate::planning::TouchpointConfidence::Candidate,
    );
    let capabilities =
        capability_ref(input.capability_snapshot, input.capability_areas);
    let workspace_scope = input.workspace_scope.map(workspace_scope_ref);
    let active_working_set =
        input.active_working_set.map(active_working_set_ref);
    let bounded_documentation = DocumentationSelection {
        root_agents: documentation
            .root_agents
            .iter()
            .take(ExecutorContextPackLimits::MAX_DOCUMENTATION_ENTRIES)
            .cloned()
            .collect(),
        nested_agents: documentation
            .nested_agents
            .iter()
            .take(ExecutorContextPackLimits::MAX_DOCUMENTATION_ENTRIES)
            .cloned()
            .collect(),
        architecture_docs: documentation
            .architecture_docs
            .iter()
            .take(ExecutorContextPackLimits::MAX_DOCUMENTATION_ENTRIES)
            .cloned()
            .collect(),
        adrs: documentation
            .adrs
            .iter()
            .take(ExecutorContextPackLimits::MAX_DOCUMENTATION_ENTRIES)
            .cloned()
            .collect(),
        development_docs: documentation
            .development_docs
            .iter()
            .take(ExecutorContextPackLimits::MAX_DOCUMENTATION_ENTRIES)
            .cloned()
            .collect(),
        dropped: documentation
            .dropped
            .iter()
            .take(ExecutorContextPackLimits::MAX_DOCUMENTATION_ENTRIES)
            .cloned()
            .collect(),
    };
    let scope_signals = input.scope_signals.map(|signals| {
        signals
            .iter()
            .take(ExecutorContextPackLimits::MAX_SCOPE_SIGNALS)
            .map(|signal| ScopeSignalRef {
                id: signal.id.clone(),
                message: signal.message.clone(),
            })
            .collect()
    });
    let new_files = input.new_files.map(|files| {
        files
            .iter()
            .take(ExecutorContextPackLimits::MAX_NEW_FILES)
            .map(|file| NewFileRef {
                path: file.path.clone(),
                reason: file.reason.clone(),
                existing_owners_inspected: file
                    .existing_owners_inspected
                    .clone(),
            })
            .collect()
    });
    let acceptance = input
        .milestone
        .map(|manifest| {
            manifest
                .acceptance
                .iter()
                .take(ExecutorContextPackLimits::MAX_ACCEPTANCE)
                .map(|requirement| AcceptanceRequirementPackRef {
                    id: requirement.id.clone(),
                    description: requirement.description.clone(),
                    criterion_id: requirement.criterion_id.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let unused = truncate_bounded("", 1, 1);
    let _ = unused;
    ExecutorContextPack {
        task: TaskPlanRefId {
            id: input.contract.id().to_owned(),
            revision: input.contract.revision(),
        },
        plan: plan_ref,
        execution_contract: input.execution_contract.clone(),
        milestone: input.milestone.map(|manifest| {
            crate::executor::milestone::MilestoneManifestRef {
                id: manifest.id.clone(),
                version: manifest.version,
            }
        }),
        instructions,
        architecture,
        verified_touchpoints,
        candidate_touchpoints,
        capabilities,
        unresolved_findings: findings,
        acceptance,
        workspace_scope,
        active_working_set,
        documentation: Some(bounded_documentation),
        scope_signals,
        new_files,
    }
}
