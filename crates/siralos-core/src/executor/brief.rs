//! ExecutorBriefCompiler (executor briefing foundation, ADR 0022;
//! Stage 3R R13.4).
//!
//! Compiles the bounded, provider-neutral ExecutorBrief for one task.
//! Deterministic (identical inputs -> byte-identical briefs), explicitly
//! bounded, and never capability-granting: permanent rules are referenced
//! by contract revision instead of being restated.

use serde_json::json;

use crate::identity::{canonical_json_value, sha256_hex};
use crate::task::TaskContract;

use super::context::ExecutorContextPack;
use super::contracts::ExecutionContract;
use super::milestone::MilestoneManifest;

/// Brief schema version (stable identity, not a content revision).
pub const EXECUTOR_BRIEF_SCHEMA_VERSION: u64 = 2;

/// Host-owned hard bounds for compiled briefs.
pub struct ExecutorBriefLimits;

impl ExecutorBriefLimits {
    /// Maximum request bytes.
    pub(crate) const MAX_REQUEST_BYTES: usize = 1024;
    /// Maximum deliverables.
    pub const MAX_DELIVERABLES: usize = 8;
    /// Maximum deliverable bytes.
    pub(crate) const MAX_DELIVERABLE_BYTES: usize = 512;
    /// Maximum touchpoints per list.
    pub(crate) const MAX_TOUCHPOINTS: usize = 12;
    /// Maximum invariants.
    pub(crate) const MAX_INVARIANTS: usize = 12;
    /// Maximum invariant bytes.
    pub(crate) const MAX_INVARIANT_BYTES: usize = 512;
    /// Maximum non-goals.
    pub(crate) const MAX_NON_GOALS: usize = 12;
    /// Maximum acceptance ids.
    pub(crate) const MAX_ACCEPTANCE_IDS: usize = 32;
    /// Maximum test requirements.
    pub(crate) const MAX_TEST_REQUIREMENTS: usize = 8;
    /// Maximum architecture references.
    pub(crate) const MAX_ARCHITECTURE_REFERENCES: usize = 4;
    /// Maximum capability limits.
    pub(crate) const MAX_CAPABILITY_LIMITS: usize = 8;
    /// Maximum instruction sources.
    pub(crate) const MAX_INSTRUCTION_SOURCES: usize = 8;
    /// Maximum rendered bytes.
    pub const MAX_RENDERED_BYTES: usize = 8 * 1024;
    /// Maximum documentation sources.
    pub(crate) const MAX_DOCUMENTATION_SOURCES: usize = 12;
    /// Maximum working-set files.
    pub(crate) const MAX_WORKING_SET_FILES: usize = 8;
    /// Maximum workspace verified files.
    pub(crate) const MAX_WORKSPACE_VERIFIED_FILES: usize = 12;
    /// Maximum scope warnings.
    pub(crate) const MAX_SCOPE_WARNINGS: usize = 8;
    /// Maximum new-file rationales.
    pub(crate) const MAX_NEW_FILE_RATIONALES: usize = 8;
}

/// Compiled executor brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorBrief {
    /// Fixed format marker.
    pub format: &'static str,
    /// Schema version.
    pub version: u64,
    /// Task id.
    pub task_id: String,
    /// Contract revision.
    pub contract_revision: u64,
    /// Bounded request text.
    pub request: String,
    /// Execution-contract reference.
    pub execution_contract: (String, u64),
    /// Milestone reference, when any.
    pub milestone: Option<(String, u64)>,
    /// Task-specific deliverables.
    pub deliverables: Vec<String>,
    /// Verified touchpoint paths.
    pub verified_touchpoints: Vec<String>,
    /// Candidate touchpoint paths.
    pub candidate_touchpoints: Vec<String>,
    /// Task-specific invariants.
    pub invariants: Vec<String>,
    /// Non-goals.
    pub non_goals: Vec<String>,
    /// Acceptance ids.
    pub acceptance_ids: Vec<String>,
    /// Test requirements.
    pub test_requirements: Vec<String>,
    /// Architecture references.
    pub architecture_references: Vec<String>,
    /// Documentation sources.
    pub documentation_sources: Vec<String>,
    /// Working-set entries `path (reason)`.
    pub working_set_files: Vec<String>,
    /// Verified workspace-scope paths.
    pub workspace_verified_files: Vec<String>,
    /// Scope warnings `id: message`.
    pub scope_warnings: Vec<String>,
    /// New-file rationales with owners.
    pub new_file_rationales: Vec<String>,
    /// Capability limits for unavailable/blocked capabilities.
    pub capability_limits: Vec<String>,
    /// Plan reference, when any.
    pub plan: Option<(String, u64, String)>,
    /// Instruction sources.
    pub instruction_sources: Vec<String>,
}

fn bounded_strings(
    values: &[String],
    max: usize,
    max_bytes: usize,
) -> Vec<String> {
    let mut result = Vec::new();
    for value in values {
        if result.len() >= max {
            break;
        }
        let text = value.trim();
        if text.is_empty() {
            continue;
        }
        result.push(if text.len() > max_bytes {
            // The reference slices UTF-16 code units; fixture strings are
            // ASCII so a char-boundary slice matches exactly.
            format!("{}\u{2026}", &text[..240.min(text.len())])
        } else {
            text.to_owned()
        });
    }
    result
}

fn capability_limit_lines(pack: &ExecutorContextPack) -> Vec<String> {
    if !pack.capabilities.available {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for (area, state) in &pack.capabilities.states {
        if lines.len() >= ExecutorBriefLimits::MAX_CAPABILITY_LIMITS {
            break;
        }
        if state == "available" {
            continue;
        }
        let line = match state.as_str() {
            "blocked_by_policy" => format!("{area}: denied by policy"),
            "unavailable" | "unsupported" => format!("{area}: unavailable"),
            "degraded" => format!("{area}: degraded"),
            other => format!("{area}: {other}"),
        };
        lines.push(line);
    }
    lines
}

/// Compile input.
pub struct CompileExecutorBriefInput<'a> {
    /// The task contract.
    pub contract: &'a TaskContract,
    /// The execution contract.
    pub execution_contract: &'a ExecutionContract,
    /// Optional milestone manifest.
    pub milestone: Option<&'a MilestoneManifest>,
    /// The derived context pack.
    pub pack: &'a ExecutorContextPack,
}

/// Compile the brief for one task. Deterministic and bounded.
#[allow(clippy::too_many_lines)]
pub fn compile_executor_brief(
    input: &CompileExecutorBriefInput<'_>,
) -> ExecutorBrief {
    let pack = input.pack;
    let request = input.contract.request().trim().to_owned();
    let request_rendered =
        if request.len() > ExecutorBriefLimits::MAX_REQUEST_BYTES {
            format!("{}\u{2026}", &request[..480.min(request.len())])
        } else {
            request.clone()
        };
    let documentation_sources_input: Vec<String> = pack
        .documentation
        .as_ref()
        .map(|documentation| {
            [
                &documentation.root_agents,
                &documentation.nested_agents,
                &documentation.architecture_docs,
                &documentation.adrs,
                &documentation.development_docs,
            ]
            .into_iter()
            .flatten()
            .cloned()
            .collect()
        })
        .unwrap_or_default();
    let working_set_input: Vec<String> = pack
        .active_working_set
        .as_ref()
        .map(|set| {
            set.files
                .iter()
                .map(|file| format!("{} ({})", file.0, file.1))
                .collect()
        })
        .unwrap_or_default();
    let scope_warnings_input: Vec<String> = pack
        .scope_signals
        .as_ref()
        .map(|signals| {
            signals
                .iter()
                .map(|signal| format!("{}: {}", signal.id, signal.message))
                .collect()
        })
        .unwrap_or_default();
    let new_files_input: Vec<String> = pack
        .new_files
        .as_ref()
        .map(|files| {
            files
                .iter()
                .map(|file| {
                    format!(
                        "{} \u{2014} {} (owners: {})",
                        file.path,
                        file.reason,
                        if file.existing_owners_inspected.is_empty() {
                            "none".to_owned()
                        } else {
                            file.existing_owners_inspected.join(", ")
                        }
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    ExecutorBrief {
        format: "siralos-executor-brief",
        version: EXECUTOR_BRIEF_SCHEMA_VERSION,
        task_id: input.contract.id().to_owned(),
        contract_revision: input.contract.revision(),
        request: request_rendered,
        execution_contract: (
            input.execution_contract.id.clone(),
            input.execution_contract.revision,
        ),
        milestone: input
            .milestone
            .map(|manifest| (manifest.id.clone(), manifest.version)),
        deliverables: bounded_strings(
            &input
                .milestone
                .map(|manifest| {
                    manifest
                        .deliverables
                        .iter()
                        .map(|deliverable| deliverable.description.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ExecutorBriefLimits::MAX_DELIVERABLES,
            ExecutorBriefLimits::MAX_DELIVERABLE_BYTES,
        ),
        verified_touchpoints: bounded_strings(
            &pack
                .verified_touchpoints
                .iter()
                .map(|touchpoint| touchpoint.path.clone())
                .collect::<Vec<_>>(),
            ExecutorBriefLimits::MAX_TOUCHPOINTS,
            512,
        ),
        candidate_touchpoints: bounded_strings(
            &pack
                .candidate_touchpoints
                .iter()
                .map(|touchpoint| touchpoint.path.clone())
                .collect::<Vec<_>>(),
            ExecutorBriefLimits::MAX_TOUCHPOINTS,
            512,
        ),
        invariants: bounded_strings(
            &input
                .milestone
                .map(|manifest| {
                    manifest
                        .invariants
                        .iter()
                        .map(|invariant| invariant.description.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ExecutorBriefLimits::MAX_INVARIANTS,
            ExecutorBriefLimits::MAX_INVARIANT_BYTES,
        ),
        non_goals: bounded_strings(
            &input
                .milestone
                .map(|manifest| manifest.non_goals.clone())
                .unwrap_or_default(),
            ExecutorBriefLimits::MAX_NON_GOALS,
            ExecutorBriefLimits::MAX_INVARIANT_BYTES,
        ),
        acceptance_ids: bounded_strings(
            &input
                .milestone
                .map(|manifest| {
                    manifest
                        .acceptance
                        .iter()
                        .map(|requirement| requirement.id.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ExecutorBriefLimits::MAX_ACCEPTANCE_IDS,
            128,
        ),
        test_requirements: bounded_strings(
            &input
                .milestone
                .map(|manifest| {
                    manifest
                        .required_tests
                        .iter()
                        .map(|test| test.description.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ExecutorBriefLimits::MAX_TEST_REQUIREMENTS,
            ExecutorBriefLimits::MAX_INVARIANT_BYTES,
        ),
        architecture_references: bounded_strings(
            &pack
                .architecture
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
            ExecutorBriefLimits::MAX_ARCHITECTURE_REFERENCES,
            512,
        ),
        documentation_sources: bounded_strings(
            &documentation_sources_input,
            ExecutorBriefLimits::MAX_DOCUMENTATION_SOURCES,
            512,
        ),
        working_set_files: bounded_strings(
            &working_set_input,
            ExecutorBriefLimits::MAX_WORKING_SET_FILES,
            512,
        ),
        workspace_verified_files: bounded_strings(
            &pack
                .workspace_scope
                .as_ref()
                .map(|scope| {
                    scope
                        .verified_files
                        .iter()
                        .map(|file| file.0.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            ExecutorBriefLimits::MAX_WORKSPACE_VERIFIED_FILES,
            512,
        ),
        scope_warnings: bounded_strings(
            &scope_warnings_input,
            ExecutorBriefLimits::MAX_SCOPE_WARNINGS,
            512,
        ),
        new_file_rationales: bounded_strings(
            &new_files_input,
            ExecutorBriefLimits::MAX_NEW_FILE_RATIONALES,
            512,
        ),
        capability_limits: capability_limit_lines(pack),
        plan: pack.plan.as_ref().map(|plan| {
            (plan.id.clone(), plan.revision, plan.approval.clone())
        }),
        instruction_sources: bounded_strings(
            &pack
                .instructions
                .iter()
                .map(|instruction| instruction.source.clone())
                .collect::<Vec<_>>(),
            ExecutorBriefLimits::MAX_INSTRUCTION_SOURCES,
            256,
        ),
    }
}

fn brief_value(brief: &ExecutorBrief) -> serde_json::Value {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    map.insert("format".to_owned(), json!(brief.format));
    map.insert("version".to_owned(), json!(brief.version));
    map.insert("taskId".to_owned(), json!(brief.task_id));
    map.insert("contractRevision".to_owned(), json!(brief.contract_revision));
    map.insert("request".to_owned(), json!(brief.request));
    map.insert(
        "executionContract".to_owned(),
        json!({ "id": brief.execution_contract.0, "revision": brief.execution_contract.1 }),
    );
    map.insert(
        "milestone".to_owned(),
        brief
            .milestone
            .as_ref()
            .map(|(id, version)| json!({ "id": id, "version": version }))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert("deliverables".to_owned(), json!(brief.deliverables));
    map.insert(
        "verifiedTouchpoints".to_owned(),
        json!(brief.verified_touchpoints),
    );
    map.insert(
        "candidateTouchpoints".to_owned(),
        json!(brief.candidate_touchpoints),
    );
    map.insert("invariants".to_owned(), json!(brief.invariants));
    map.insert("nonGoals".to_owned(), json!(brief.non_goals));
    map.insert("acceptanceIds".to_owned(), json!(brief.acceptance_ids));
    map.insert("testRequirements".to_owned(), json!(brief.test_requirements));
    map.insert(
        "architectureReferences".to_owned(),
        json!(brief.architecture_references),
    );
    map.insert(
        "documentationSources".to_owned(),
        json!(brief.documentation_sources),
    );
    map.insert("workingSetFiles".to_owned(), json!(brief.working_set_files));
    map.insert(
        "workspaceVerifiedFiles".to_owned(),
        json!(brief.workspace_verified_files),
    );
    map.insert("scopeWarnings".to_owned(), json!(brief.scope_warnings));
    map.insert(
        "newFileRationales".to_owned(),
        json!(brief.new_file_rationales),
    );
    map.insert("capabilityLimits".to_owned(), json!(brief.capability_limits));
    map.insert(
        "plan".to_owned(),
        brief
            .plan
            .as_ref()
            .map(|(id, revision, approval)| {
                json!({
                    "id": id,
                    "revision": revision,
                    "approval": approval,
                })
            })
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "instructionSources".to_owned(),
        json!(brief.instruction_sources),
    );
    serde_json::Value::Object(map.into_iter().collect())
}

/// Deterministic fingerprint over the brief's canonical form.
pub fn compute_executor_brief_fingerprint(brief: &ExecutorBrief) -> String {
    sha256_hex(canonical_json_value(&brief_value(brief)).as_bytes())
}

/// Secret-only redaction, re-exported from the single owner in
/// [`crate::doctor`].
///
/// This module carried a second implementation of the same rule set until the
/// consolidation recorded in `ROADMAP.md` §10 item 3; the reference had one
/// owner, and the port's duplicate had drifted from it.
pub use crate::doctor::sanitize_secrets_only;

#[derive(Clone)]
struct RenderSection {
    title: &'static str,
    content: String,
    priority: u8,
}

fn bullet(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rendered_bytes_of(sections: &[RenderSection]) -> usize {
    sections
        .iter()
        .map(|section| format!("{}\n{}", section.title, section.content))
        .collect::<Vec<_>>()
        .join("\n\n")
        .len()
}

const TRUNCATION_MARKER: &str = "\n\u{2026} [brief truncated]";

fn trim_rendered(rendered: &str, max_bytes: usize) -> String {
    let marker_bytes = TRUNCATION_MARKER.len();
    if max_bytes <= marker_bytes {
        // Marker slice on UTF-16 units; the marker is BMP-only.
        let take = max_bytes.max(1).min(TRUNCATION_MARKER.chars().count());
        return TRUNCATION_MARKER.chars().take(take).collect();
    }
    let mut kept = String::new();
    for line in rendered.split('\n') {
        let candidate = if kept.is_empty() {
            line.to_owned()
        } else {
            format!("{kept}\n{line}")
        };
        if format!("{candidate}{TRUNCATION_MARKER}").len() > max_bytes {
            break;
        }
        kept = candidate;
    }
    format!("{kept}{TRUNCATION_MARKER}")
}

/// Rendered text representation of the brief: drops whole low-priority
/// sections under budget pressure, then truncates the tail, then applies
/// secret redaction at the boundary.
pub fn render_executor_brief(brief: &ExecutorBrief) -> String {
    render_executor_brief_bounded(
        brief,
        ExecutorBriefLimits::MAX_RENDERED_BYTES,
    )
}

/// Bounded variant of [`render_executor_brief`].
#[allow(clippy::too_many_lines)]
pub fn render_executor_brief_bounded(
    brief: &ExecutorBrief,
    max_bytes: usize,
) -> String {
    let mut sections: Vec<RenderSection> = vec![
        RenderSection {
            title: "TASK",
            content: brief.request.clone(),
            priority: 0,
        },
        RenderSection {
            title: "EXECUTION CONTRACT",
            content: format!(
                "Execution Contract: {} rev {}",
                brief.execution_contract.0, brief.execution_contract.1
            ),
            priority: 0,
        },
    ];
    if let Some((id, version)) = &brief.milestone {
        sections.push(RenderSection {
            title: "MILESTONE",
            content: format!("Milestone Manifest: {id} rev {version}"),
            priority: 0,
        });
    }
    if let Some((id, revision, approval)) = &brief.plan {
        sections.push(RenderSection {
            title: "PLAN",
            content: format!("Plan: {id} rev {revision} ({approval})"),
            priority: 0,
        });
    }
    if !brief.invariants.is_empty() {
        sections.push(RenderSection {
            title: "TASK-SPECIFIC INVARIANTS",
            content: bullet(&brief.invariants),
            priority: 0,
        });
    }
    if !brief.acceptance_ids.is_empty() {
        sections.push(RenderSection {
            title: "ACCEPTANCE",
            content: brief.acceptance_ids.join(", "),
            priority: 0,
        });
    }
    if !brief.verified_touchpoints.is_empty() {
        sections.push(RenderSection {
            title: "VERIFIED TOUCHPOINTS",
            content: bullet(&brief.verified_touchpoints),
            priority: 1,
        });
    }
    if !brief.workspace_verified_files.is_empty() {
        sections.push(RenderSection {
            title: "VERIFIED WORKSPACE FILES",
            content: bullet(&brief.workspace_verified_files),
            priority: 1,
        });
    }
    if !brief.working_set_files.is_empty() {
        sections.push(RenderSection {
            title: "WORKING SET (CURRENT STEP)",
            content: bullet(&brief.working_set_files),
            priority: 3,
        });
    }
    if !brief.deliverables.is_empty() {
        sections.push(RenderSection {
            title: "DELIVERABLES",
            content: bullet(&brief.deliverables),
            priority: 2,
        });
    }
    if !brief.candidate_touchpoints.is_empty() {
        sections.push(RenderSection {
            title: "CANDIDATE TOUCHPOINTS",
            content: bullet(&brief.candidate_touchpoints),
            priority: 3,
        });
    }
    if !brief.non_goals.is_empty() {
        sections.push(RenderSection {
            title: "NON-GOALS",
            content: bullet(&brief.non_goals),
            priority: 4,
        });
    }
    if !brief.architecture_references.is_empty() {
        sections.push(RenderSection {
            title: "ARCHITECTURE REFERENCES",
            content: bullet(&brief.architecture_references),
            priority: 5,
        });
    }
    if !brief.documentation_sources.is_empty() {
        sections.push(RenderSection {
            title: "DOCUMENTATION",
            content: bullet(&brief.documentation_sources),
            priority: 5,
        });
    }
    if !brief.new_file_rationales.is_empty() {
        sections.push(RenderSection {
            title: "NEW FILES (RATIONALE)",
            content: bullet(&brief.new_file_rationales),
            priority: 6,
        });
    }
    if !brief.test_requirements.is_empty() {
        sections.push(RenderSection {
            title: "MILESTONE-SPECIFIC TESTS",
            content: bullet(&brief.test_requirements),
            priority: 6,
        });
    }
    if !brief.capability_limits.is_empty() {
        sections.push(RenderSection {
            title: "CAPABILITY LIMITS",
            content: bullet(&brief.capability_limits),
            priority: 7,
        });
    }
    if !brief.scope_warnings.is_empty() {
        sections.push(RenderSection {
            title: "SCOPE WARNINGS",
            content: bullet(&brief.scope_warnings),
            priority: 7,
        });
    }
    let joined = sections
        .iter()
        .map(|section| format!("{}\n{}", section.title, section.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    let sanitized = sanitize_secrets_only(&joined);
    if sanitized.len() <= max_bytes {
        return sanitized;
    }
    let mut kept: Vec<RenderSection> = sections
        .iter()
        .filter(|section| section.priority == 0)
        .map(|section| RenderSection {
            title: section.title,
            content: section.content.clone(),
            priority: section.priority,
        })
        .collect();
    for priority in 1..=7u8 {
        if rendered_bytes_of(&kept) <= max_bytes {
            break;
        }
        kept.extend(
            sections
                .iter()
                .filter(|section| section.priority == priority)
                .map(|section| RenderSection {
                    title: section.title,
                    content: section.content.clone(),
                    priority: section.priority,
                }),
        );
        if rendered_bytes_of(&kept) <= max_bytes {
            break;
        }
        while kept.last().is_some_and(|section| section.priority == priority) {
            kept.pop();
        }
    }
    let rendered = kept
        .iter()
        .map(|section| format!("{}\n{}", section.title, section.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    sanitize_secrets_only(&trim_rendered(&rendered, max_bytes))
}

/// Compact deterministic description of a brief's shape.
pub fn summarize_executor_brief(brief: &ExecutorBrief) -> String {
    [
        format!(
            "{} rev {}",
            brief.execution_contract.0, brief.execution_contract.1
        ),
        brief.milestone.as_ref().map_or_else(
            || "no milestone".to_owned(),
            |(id, version)| format!("{id} rev {version}"),
        ),
        format!(
            "{} verified / {} candidate touchpoints",
            brief.verified_touchpoints.len(),
            brief.candidate_touchpoints.len()
        ),
        format!("{} acceptance ids", brief.acceptance_ids.len()),
    ]
    .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::new_files::path_matches_pattern;

    #[test]
    fn sanitizer_redacts_credential_shaped_tokens_only() {
        let sanitized = sanitize_secrets_only(
            "key sk-abcd12345678 and AKIAIOSFODNN7EXAMPLE plus ghp_abcdefghijklmnopqrst",
        );
        assert!(sanitized.contains("<secret>"));
        assert!(!sanitized.contains("sk-abcd12345678"));
        // Repository-relative paths survive secret-only redaction.
        let paths =
            sanitize_secrets_only("see crates/a.rs and docs/adr/0022-x.md");
        assert_eq!(paths, "see crates/a.rs and docs/adr/0022-x.md");
    }

    #[test]
    fn glob_matching_matches_reference_segment_semantics() {
        assert!(path_matches_pattern("src/abc.ts", "src/*.ts"));
        assert!(!path_matches_pattern("src/deep/a.ts", "src/*.ts"));
        assert!(path_matches_pattern("a/b/c/d.ts", "a/**/*.ts"));
        assert!(path_matches_pattern("a/file.ts", "a/**"));
        assert!(!path_matches_pattern("src/b.ts", "src/a.ts"));
    }
}
