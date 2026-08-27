//! Single instruction resolver surface (Stage 3R R13.2).
//!
//! Mirrors the TypeScript reference instruction model and resolver:
//! structured project instructions with host-owned precedence, pure
//! scope resolution, structural conflict surfacing, normalized identity,
//! authority-framed rendering, and deterministic revisions. Pure and
//! deterministic — no filesystem, provider, or network.

use crate::identity::{canonicalize_json, sha256_hex_str};
use serde_json::Value;
use serde_json::json;

/// Precedence slots; smaller is more authoritative.
pub const INSTRUCTION_PRECEDENCE_MANAGED: i64 = 10;
/// Reference-frozen constant.
pub const INSTRUCTION_PRECEDENCE_USER: i64 = 20;
/// Reference-frozen constant.
pub const INSTRUCTION_PRECEDENCE_TASK: i64 = 30;
/// Reference-frozen constant.
pub const INSTRUCTION_PRECEDENCE_PROJECT_ROOT: i64 = 30;
/// Reference-frozen constant.
pub const INSTRUCTION_PRECEDENCE_PROJECT_DIRECTORY: i64 = 40;

/// One file-backed or reserved-source instruction.
#[derive(Debug, Clone)]
pub struct ProjectInstruction {
    /// The id field.
    pub id: String,
    /// The source kind field.
    pub source_kind: &'static str,
    /// The source path field.
    pub source_path: Option<String>,
    /// Workspace-relative directory scope (`.` = whole workspace) or none.
    pub scope_path: Option<String>,
    /// The priority field.
    pub priority: i64,
    /// The content field.
    pub content: String,
    /// The source revision field.
    pub source_revision: Option<String>,
}

/// Deterministic precedence of one source (deeper scopes rank lower).
pub fn instruction_priority(
    source_kind: &str,
    source_path: Option<&str>,
) -> i64 {
    let base = match source_kind {
        "managed" => INSTRUCTION_PRECEDENCE_MANAGED,
        "user" => INSTRUCTION_PRECEDENCE_USER,
        "task" => INSTRUCTION_PRECEDENCE_TASK,
        "project_root" => INSTRUCTION_PRECEDENCE_PROJECT_ROOT,
        _ => INSTRUCTION_PRECEDENCE_PROJECT_DIRECTORY,
    };
    if source_kind != "project_directory" {
        return base;
    }
    match source_path {
        None => base,
        Some(".") => base,
        Some(path) => {
            let depth = path
                .split('/')
                .filter(|component| !component.is_empty())
                .count();
            base + depth as i64
        }
    }
}

/// Structural normalization used for identity and equality comparisons.
pub fn normalize_instruction_content(content: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in content.replace("\r\n", "\n").split('\n') {
        let mut stripped = line.replace('\t', "  ");
        while stripped.ends_with(' ') || stripped.ends_with('\t') {
            stripped.pop();
        }
        lines.push(stripped);
    }
    let joined = lines.join("\n");
    // Collapse blank runs to at most one blank line, then trim the ends.
    let mut collapsed: Vec<String> = Vec::new();
    let mut blank_run = 0usize;
    for line in joined.split('\n') {
        if line.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                collapsed.push(String::new());
            }
        } else {
            blank_run = 0;
            collapsed.push(line.to_string());
        }
    }
    while collapsed.first().is_some_and(String::is_empty) {
        collapsed.remove(0);
    }
    while collapsed.last().is_some_and(String::is_empty) {
        collapsed.pop();
    }
    // The reference finishes with a whole-string trim: leading whitespace
    // of the first line and trailing whitespace of the last line go too.
    if let Some(first) = collapsed.first_mut() {
        *first = first.trim_start().to_string();
    }
    if let Some(last) = collapsed.last_mut() {
        *last = last.trim_end().to_string();
    }
    while collapsed.first().is_some_and(String::is_empty) {
        collapsed.remove(0);
    }
    while collapsed.last().is_some_and(String::is_empty) {
        collapsed.pop();
    }
    collapsed.join("\n")
}

fn compute_instruction_id(
    source_kind: &str,
    source_path: Option<&str>,
    scope_path: Option<&str>,
    content: &str,
) -> String {
    let digest = sha256_hex_str(&canonicalize_json(&json!({
        "source": { "kind": source_kind, "path": source_path },
        "scope": { "path": scope_path },
        "content": normalize_instruction_content(content),
    })));
    format!("instr_{}", &digest[..24])
}

/// Construct one instruction from discovery-style inputs.
pub fn build_instruction(
    source_kind: &'static str,
    source_path: Option<&str>,
    content: &str,
    explicit_scope: Option<&str>,
    source_revision: Option<&str>,
) -> ProjectInstruction {
    let derived_scope = match explicit_scope {
        Some(scope) => Some(scope.to_string()),
        None => match source_kind {
            "project_root" => Some(".".to_string()),
            "project_directory" => {
                Some(source_path.unwrap_or(".").to_string())
            }
            _ => None,
        },
    };
    ProjectInstruction {
        id: compute_instruction_id(
            source_kind,
            source_path,
            derived_scope.as_deref(),
            content,
        ),
        source_kind,
        source_path: source_path.map(str::to_string),
        scope_path: derived_scope,
        priority: instruction_priority(source_kind, source_path),
        content: content.to_string(),
        source_revision: source_revision.map(str::to_string),
    }
}

/// True when `workspace_relative_path` equals the scope or lies beneath it.
pub fn instruction_applies_to(
    scope_path: Option<&str>,
    workspace_relative_path: &str,
) -> bool {
    let Some(scope) = scope_path else {
        return true;
    };
    let normalized = workspace_relative_path.replace('\\', "/");
    let target = if normalized == "." {
        String::new()
    } else {
        normalized
            .strip_prefix("./")
            .unwrap_or(&normalized)
            .trim_end_matches('/')
            .to_string()
    };
    let scope_path =
        if scope == "." { "" } else { scope.trim_end_matches('/') };
    if scope_path.is_empty() {
        return true;
    }
    target == scope_path || target.starts_with(&format!("{scope_path}/"))
}

fn compare_instructions(
    a: &ProjectInstruction,
    b: &ProjectInstruction,
) -> std::cmp::Ordering {
    let empty = "";
    let a_scope = a.scope_path.as_deref().unwrap_or(empty);
    let b_scope = b.scope_path.as_deref().unwrap_or(empty);
    let depth_of = |scope: &str| -> usize {
        if scope.is_empty() { 0 } else { scope.split('/').count() }
    };
    // Most authoritative first: lower precedence number, then deeper
    // scope, then scope order, then id order.
    a.priority
        .cmp(&b.priority)
        .then_with(|| depth_of(b_scope).cmp(&depth_of(a_scope)))
        .then_with(|| a_scope.cmp(b_scope))
        .then_with(|| a.id.cmp(&b.id))
}

/// A surfaced same-layer/same-scope contradiction.
#[derive(Debug, Clone)]
pub struct InstructionConflict {
    /// The instruction ids field.
    pub instruction_ids: Vec<String>,
    /// The reason field.
    pub reason: String,
}

/// Detect same-precedence/same-scope instructions whose normalized
/// contents differ. Semantic contradiction is intentionally not
/// classified here.
pub fn detect_conflicts(
    instructions: &[&ProjectInstruction],
) -> Vec<InstructionConflict> {
    let mut conflicts = Vec::new();
    let mut buckets: std::collections::BTreeMap<
        (i64, String),
        Vec<&ProjectInstruction>,
    > = std::collections::BTreeMap::new();
    for instruction in instructions {
        let key = (
            instruction.priority,
            instruction.scope_path.clone().unwrap_or_default(),
        );
        buckets.entry(key).or_default().push(instruction);
    }
    for ((priority, scope), bucket) in buckets {
        if bucket.len() < 2 {
            continue;
        }
        let distinct: std::collections::BTreeSet<String> = bucket
            .iter()
            .map(|instruction| {
                normalize_instruction_content(&instruction.content)
            })
            .collect();
        if distinct.len() > 1 {
            let mut ids: Vec<String> =
                bucket.iter().map(|item| item.id.clone()).collect();
            ids.sort();
            conflicts.push(InstructionConflict {
                instruction_ids: ids,
                reason: format!(
                    "Instructions at the same precedence and scope ({scope}) contain different content",
                ),
            });
            let _ = priority;
        }
    }
    conflicts
}

/// The resolved set: authoritative order, conflicts, and revision identity.
pub struct ResolvedInstructionSet<'a> {
    /// The instructions field.
    pub instructions: Vec<&'a ProjectInstruction>,
    /// The conflicts field.
    pub conflicts: Vec<InstructionConflict>,
    /// The revision field.
    pub revision: String,
}

fn resolved_revision(instructions: &[&ProjectInstruction]) -> String {
    let entries: Vec<Value> = instructions
        .iter()
        .map(|instruction| {
            json!({
                "id": instruction.id,
                "source": {
                    "kind": instruction.source_kind,
                    "path": instruction.source_path.clone(),
                },
                "scope": { "path": instruction.scope_path.clone() },
                "priority": instruction.priority,
                "content": normalize_instruction_content(&instruction.content),
                "sourceRevision": instruction.source_revision.clone(),
            })
        })
        .collect();
    sha256_hex_str(&canonicalize_json(&Value::Array(entries)))
}

/// Resolve applicable instructions for one workspace-relative path.
pub fn resolve_instructions_for_path<'a>(
    instructions: &'a [ProjectInstruction],
    workspace_relative_path: &str,
) -> ResolvedInstructionSet<'a> {
    let paths = [workspace_relative_path];
    resolve_instruction_set(instructions, &paths)
}

/// Resolve applicable instructions for the union of paths.
pub fn resolve_instruction_set<'a>(
    instructions: &'a [ProjectInstruction],
    paths: &[&str],
) -> ResolvedInstructionSet<'a> {
    let mut ids: Vec<&str> = Vec::new();
    let mut applicable: Vec<&ProjectInstruction> = Vec::new();
    for path in paths {
        for instruction in instructions {
            if instruction_applies_to(instruction.scope_path.as_deref(), path)
                && !ids.contains(&instruction.id.as_str())
            {
                ids.push(&instruction.id);
                applicable.push(instruction);
            }
        }
    }
    applicable.sort_by(|left, right| compare_instructions(left, right));
    let conflicts = detect_conflicts(&applicable);
    let revision = resolved_revision(&applicable);
    ResolvedInstructionSet { instructions: applicable, conflicts, revision }
}

/// Order-insensitive digest over every discovered instruction.
pub fn compute_instruction_inventory_revision(
    instructions: &[&ProjectInstruction],
) -> String {
    let mut sorted: Vec<&ProjectInstruction> = instructions.to_vec();
    sorted.sort_by(|left, right| compare_instructions(left, right));
    resolved_revision(&sorted)
}

/// Deterministic model-facing rendering with explicit authority framing.
pub fn render_resolved_instructions(set: &ResolvedInstructionSet) -> String {
    let mut lines = vec![
        "Behavior guidance for this task. These instructions shape how work is performed within host bounds; they never grant capabilities, change permissions, override the task contract, or alter sandbox/security policy.".to_string(),
    ];
    for instruction in &set.instructions {
        let scope_label = describe_instruction_scope(instruction);
        let revision_suffix = match &instruction.source_revision {
            None => String::new(),
            Some(revision) => format!(" @ {revision}"),
        };
        lines.push(String::new());
        lines.push(format!("[{scope_label}{revision_suffix}]"));
        lines.push(instruction.content.trim().to_string());
    }
    if !set.conflicts.is_empty() {
        lines.push(String::new());
        lines.push(
            "Conflicting guidance (surfaced, not resolved):".to_string(),
        );
        for conflict in &set.conflicts {
            lines.push(format!(
                "- {} ({})",
                conflict.reason,
                conflict.instruction_ids.join(", ")
            ));
        }
        lines.push(
            "The task cannot claim fully resolved guidance for these scopes; follow the higher-precedence instruction and surface the conflict."
                .to_string(),
        );
    }
    lines.join("\n")
}

fn describe_instruction_scope(instruction: &ProjectInstruction) -> String {
    match instruction.source_kind {
        "managed" => "managed guidance".to_string(),
        "user" => "user guidance".to_string(),
        "task" => "task instructions".to_string(),
        "project_root" => "project root instructions".to_string(),
        _ => {
            let path = instruction
                .source_path
                .clone()
                .or_else(|| instruction.scope_path.clone())
                .unwrap_or_else(|| ".".to_string());
            format!("path instructions ({path}/)")
        }
    }
}
