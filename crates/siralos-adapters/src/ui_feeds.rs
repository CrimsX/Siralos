//! Read-only UI feed resolvers (ADR 0039 §2).
//!
//! Each feed reuses an existing read-only projection; no feed
//! grants authority, opens new surfaces, or bypasses gates. Feed
//! content is raw strings: the cli sanitizes every line before it
//! reaches the projector or the terminal. Params go through the
//! same bounded, path-contained parsing as the tools.

use std::collections::BTreeMap;
use std::path::Path;

use siralos_core::tool::{
    PermissionDecision, PermissionPolicy, RegisteredToolInfo,
    evaluate_permission,
};
use siralos_core::ui::UiFeed;
use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

use crate::module::manifest::ModuleRecord;
use crate::workspace::checkpoint::open_checkpoint_store;
use crate::workspace::list::{ListOutcome, list_directory};
use crate::workspace::search::{SearchInput, SearchOutcome, search};

/// Maximum checkpoints one feed resolution lists.
pub const MAX_FEED_CHECKPOINTS: usize = 20;
/// Maximum lines one feed resolution returns before projection
/// truncation. Feeds stay small; the projector enforces spec
/// bounds on top.
pub const MAX_FEED_LINES: usize = 200;

/// Bounded context summary, built from a projection snapshot by
/// the caller. Numbers and closed-vocabulary state only — no raw
/// content ever crosses this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSummary {
    /// Projection mode wire string.
    pub mode: String,
    /// Stable segment bytes.
    pub stable_bytes: usize,
    /// Contextual segment bytes.
    pub contextual_bytes: usize,
    /// Volatile segment bytes.
    pub volatile_bytes: usize,
    /// Estimated tokens.
    pub estimated_tokens: usize,
    /// Working maximum tokens (mirrors the pressure bound type).
    pub working_maximum: i64,
    /// Pressure state wire string.
    pub pressure_state: String,
    /// Pressure percent.
    pub pressure_percent: u64,
    /// Available tools in the projection.
    pub tools_available: usize,
    /// Gated tools in the projection.
    pub tools_gated: usize,
    /// Hidden tools in the projection.
    pub tools_hidden: usize,
}

/// Host state one feed resolution may read. All handles are
/// caller-owned; feeds borrow, never retain.
pub struct FeedContext<'a> {
    /// Canonical workspace root.
    pub workspace_root: &'a Path,
    /// Registry definitions snapshot in registration order.
    pub tools: &'a [RegisteredToolInfo],
    /// Effective permission policy.
    pub policy: &'a PermissionPolicy,
    /// Installed module records.
    pub modules: &'a [ModuleRecord],
    /// Latest context summary, when computed.
    pub context: Option<&'a ContextSummary>,
    /// Caller state directory for the checkpoint store. `None`
    /// refuses only the checkpoints feed; every other feed ignores
    /// it.
    pub checkpoint_state_dir: Option<&'a Path>,
}

/// Why a feed refused to resolve. Typed and stable; paths are
/// workspace-relative, never absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// A param is missing, misshaped, oversized, or unknown.
    InvalidParams(String),
    /// The feed input was rejected by a containment gate.
    Denied(String),
}

impl FeedError {
    /// The exact externally observable message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::InvalidParams(reason) => {
                format!("Invalid feed params: {reason}")
            }
            Self::Denied(reason) => reason.clone(),
        }
    }
}

impl std::fmt::Display for FeedError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for FeedError {}

fn param<'a>(
    params: &'a BTreeMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    params.get(key).map(String::as_str)
}

fn reject_unknown_params(
    params: &BTreeMap<String, String>,
    known: &[&str],
    feed: &str,
) -> Result<(), FeedError> {
    for key in params.keys() {
        if !known.contains(&key.as_str()) {
            return Err(FeedError::InvalidParams(format!(
                "unknown param {key:?} for feed {feed}"
            )));
        }
    }
    Ok(())
}

fn resolve_list(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    reject_unknown_params(params, &["path"], "workspace.list")?;
    let requested = param(params, "path").unwrap_or(".");
    match list_directory(context.workspace_root, requested, &WORKSPACE_LIMITS)
    {
        ListOutcome::Denied { message } => Err(FeedError::Denied(message)),
        ListOutcome::Failed { message } => Err(FeedError::Denied(message)),
        ListOutcome::Success { path, entries, truncated } => {
            let mut lines = vec![format!("{path}/")];
            for entry in entries {
                let kind = match entry.kind {
                    crate::workspace::list::EntryKind::Directory => "dir",
                    crate::workspace::list::EntryKind::File { .. } => "file",
                    crate::workspace::list::EntryKind::Symlink => "link",
                    crate::workspace::list::EntryKind::Other => "other",
                };
                lines.push(format!("{kind} {}", entry.name));
            }
            if truncated {
                lines.push("… (truncated)".to_owned());
            }
            Ok(lines)
        }
    }
}

fn resolve_search(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    reject_unknown_params(params, &["query", "path"], "workspace.search")?;
    let query = param(params, "query").ok_or_else(|| {
        FeedError::InvalidParams("missing required param \"query\"".to_owned())
    })?;
    if query.is_empty() {
        return Err(FeedError::InvalidParams(
            "\"query\" must not be empty".to_owned(),
        ));
    }
    let input = SearchInput {
        query: query.to_owned(),
        path: param(params, "path").unwrap_or(".").to_owned(),
        max_results: WORKSPACE_LIMITS.max_search_matches,
    };
    match search(context.workspace_root, &input, &WORKSPACE_LIMITS, false) {
        SearchOutcome::InvalidInput { message }
        | SearchOutcome::Denied { message } => Err(FeedError::Denied(message)),
        SearchOutcome::Cancelled => {
            Err(FeedError::Denied("search was cancelled".to_owned()))
        }
        SearchOutcome::Success { matches, truncated, .. } => {
            let mut lines = Vec::new();
            for each in matches {
                lines.push(format!(
                    "{}:{}:{}: {}",
                    each.path, each.line, each.column, each.text
                ));
            }
            if truncated {
                lines.push("… (truncated)".to_owned());
            }
            Ok(lines)
        }
    }
}

fn resolve_context_summary(
    context: &FeedContext<'_>,
) -> Result<Vec<String>, FeedError> {
    let Some(summary) = context.context else {
        return Ok(vec!["context: not computed".to_owned()]);
    };
    Ok(vec![
        format!("mode: {}", summary.mode),
        format!(
            "bytes: stable={} contextual={} volatile={}",
            summary.stable_bytes,
            summary.contextual_bytes,
            summary.volatile_bytes
        ),
        format!(
            "tokens: {} estimated / {} working",
            summary.estimated_tokens, summary.working_maximum
        ),
        format!(
            "pressure: {} ({}%)",
            summary.pressure_state, summary.pressure_percent
        ),
        format!(
            "tools: {} available, {} gated, {} hidden",
            summary.tools_available, summary.tools_gated, summary.tools_hidden
        ),
    ])
}

fn tool_kind(capability: &str) -> &'static str {
    match capability {
        "workspace.write" => "write",
        "process.execute" => "execute",
        _ => "read-only",
    }
}

fn resolve_tools_catalog(
    context: &FeedContext<'_>,
) -> Result<Vec<String>, FeedError> {
    if context.tools.is_empty() {
        return Ok(vec!["tools: none registered".to_owned()]);
    }
    let mut lines = Vec::new();
    for info in context.tools {
        let status =
            match evaluate_permission(&info.capability, context.policy) {
                PermissionDecision::Deny { .. } => "denied",
                PermissionDecision::Ask { .. } => "approval required",
                PermissionDecision::Allow => "allowed",
            };
        lines.push(format!(
            "{} ({}, {})",
            info.definition.name,
            tool_kind(info.capability.as_str()),
            status
        ));
    }
    Ok(lines)
}

fn resolve_modules_status(
    context: &FeedContext<'_>,
) -> Result<Vec<String>, FeedError> {
    if context.modules.is_empty() {
        return Ok(vec!["modules: none installed".to_owned()]);
    }
    let mut lines = Vec::new();
    for record in context.modules {
        let digest =
            record.digest.strip_prefix("sha256:").unwrap_or(&record.digest);
        lines.push(format!(
            "{} (digest {}, path {})",
            record.id,
            digest.chars().take(8).collect::<String>(),
            record.path
        ));
    }
    Ok(lines)
}

fn resolve_checkpoints_list(
    context: &FeedContext<'_>,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    reject_unknown_params(params, &["limit"], "checkpoints.list")?;
    let limit = match param(params, "limit") {
        None => MAX_FEED_CHECKPOINTS,
        Some(text) => text.parse::<usize>().map_err(|_| {
            FeedError::InvalidParams(
                "\"limit\" must be an integer 1..=100".to_owned(),
            )
        })?,
    };
    if !(1..=100).contains(&limit) {
        return Err(FeedError::InvalidParams(
            "\"limit\" must be an integer 1..=100".to_owned(),
        ));
    }
    let Some(state_dir) = context.checkpoint_state_dir else {
        return Err(FeedError::Denied(
            "checkpoint store unavailable".to_owned(),
        ));
    };
    let store = open_checkpoint_store(context.workspace_root, state_dir)
        .map_err(|_| {
            FeedError::Denied("checkpoint store unavailable".to_owned())
        })?;
    let listed = store.list(None, Some(limit));
    if listed.is_empty() {
        return Ok(vec!["checkpoints: none recorded".to_owned()]);
    }
    let mut lines = Vec::new();
    for checkpoint in listed {
        // Metadata only: id, path, creation, state, byte counts.
        // Preimage bytes and content hashes never cross this
        // boundary (adversarial-pinned below).
        let bytes = checkpoint.after.byte_length.unwrap_or(0);
        lines.push(format!(
            "{} {} {} {} {}B",
            checkpoint.id,
            checkpoint.relative_path,
            checkpoint.operation.as_str(),
            checkpoint.state.as_str(),
            bytes
        ));
    }
    Ok(lines)
}

/// Resolve one feed to raw content lines. Raw means unresolved
/// for display: the cli sanitizes every line before projection
/// or terminal output. Refusals are typed; a feed never renders.
pub fn resolve_feed(
    context: &FeedContext<'_>,
    feed: UiFeed,
    params: &BTreeMap<String, String>,
) -> Result<Vec<String>, FeedError> {
    let lines = match feed {
        UiFeed::WorkspaceList => resolve_list(context, params)?,
        UiFeed::WorkspaceSearch => resolve_search(context, params)?,
        UiFeed::ContextSummary => {
            reject_unknown_params(params, &[], "context.summary")?;
            resolve_context_summary(context)?
        }
        UiFeed::ToolsCatalog => {
            reject_unknown_params(params, &[], "tools.catalog")?;
            resolve_tools_catalog(context)?
        }
        UiFeed::ModulesStatus => {
            reject_unknown_params(params, &[], "modules.status")?;
            resolve_modules_status(context)?
        }
        UiFeed::CheckpointsList => resolve_checkpoints_list(context, params)?,
    };
    Ok(lines.into_iter().take(MAX_FEED_LINES).collect())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use siralos_core::provider::{CancellationToken, ToolDefinition};
    use siralos_core::tool::{
        CapabilityId, PermissionPolicy, PermissionRule, PolicyRule,
        RegisteredToolInfo, Tool,
    };
    use siralos_core::ui::UiFeed;

    use super::{
        ContextSummary, FeedContext, FeedError, MAX_FEED_CHECKPOINTS,
        resolve_feed,
    };
    use crate::module::manifest::ModuleRecord;
    use crate::tool::WriteTool;
    use crate::workspace::checkpoint::open_checkpoint_store;

    struct Fixture {
        root: std::path::PathBuf,
        store_root: std::path::PathBuf,
        state_dir: std::path::PathBuf,
        policy: PermissionPolicy,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let tag = format!(
                "{}-{}-{}",
                std::process::id(),
                label,
                // Monotonic per process without a global: micros
                // collide only under same-microsecond spawns, and
                // the tag also carries the label.
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_micros())
                    .unwrap_or(0)
            );
            let root =
                std::env::temp_dir().join(format!("siralos-ui-ws-{tag}"));
            let store_root =
                std::env::temp_dir().join(format!("siralos-ui-st-{tag}"));
            let state_dir =
                std::env::temp_dir().join(format!("siralos-ui-cp-{tag}"));
            std::fs::create_dir_all(&root).expect("fixture root");
            let policy = PermissionPolicy::from_rules([]);
            Self { root, store_root, state_dir, policy }
        }

        fn context(&self) -> FeedContext<'_> {
            FeedContext {
                workspace_root: &self.root,
                tools: &[],
                policy: &self.policy,
                modules: &[],
                context: None,
                checkpoint_state_dir: Some(&self.state_dir),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
            let _ = std::fs::remove_dir_all(&self.store_root);
            let _ = std::fs::remove_dir_all(&self.state_dir);
        }
    }

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn list_feed_lists_sorted_entries() {
        let fixture = Fixture::new("list");
        std::fs::write(fixture.root.join("b.txt"), b"b").expect("seed");
        std::fs::write(fixture.root.join("a.txt"), b"a").expect("seed");
        let context = fixture.context();
        let lines =
            resolve_feed(&context, UiFeed::WorkspaceList, &params(&[]))
                .expect("lists");
        assert_eq!(
            lines,
            vec![
                "./".to_owned(),
                "file a.txt".to_owned(),
                "file b.txt".to_owned()
            ]
        );
    }

    #[test]
    fn list_feed_refuses_traversal_and_protected_paths() {
        let fixture = Fixture::new("list-deny");
        let context = fixture.context();
        for path in ["../escape.txt", ".siralos/notes.txt"] {
            assert!(
                matches!(
                    resolve_feed(
                        &context,
                        UiFeed::WorkspaceList,
                        &params(&[("path", path)])
                    ),
                    Err(FeedError::Denied(_))
                ),
                "{path}"
            );
        }
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::WorkspaceList,
                &params(&[("bogus", "x")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
    }

    #[test]
    fn search_feed_requires_a_query_and_finds_needles() {
        let fixture = Fixture::new("search");
        std::fs::write(fixture.root.join("hay.txt"), b"needle here")
            .expect("seed");
        let context = fixture.context();
        assert!(matches!(
            resolve_feed(&context, UiFeed::WorkspaceSearch, &params(&[])),
            Err(FeedError::InvalidParams(_))
        ));
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::WorkspaceSearch,
                &params(&[("query", "needle"), ("bogus", "x")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
        let lines = resolve_feed(
            &context,
            UiFeed::WorkspaceSearch,
            &params(&[("query", "needle")]),
        )
        .expect("searches");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("hay.txt:1:1: "), "{lines:?}");
        assert!(lines[0].contains("needle here"));
    }

    #[test]
    fn paramless_feeds_reject_params() {
        let fixture = Fixture::new("no-params");
        let context = fixture.context();
        for feed in [
            UiFeed::ContextSummary,
            UiFeed::ToolsCatalog,
            UiFeed::ModulesStatus,
        ] {
            assert!(
                matches!(
                    resolve_feed(&context, feed, &params(&[("bogus", "x")])),
                    Err(FeedError::InvalidParams(_))
                ),
                "{feed:?}"
            );
        }
    }

    #[test]
    fn context_summary_renders_numbers_without_content() {
        let summary = ContextSummary {
            mode: "generic".to_owned(),
            stable_bytes: 100,
            contextual_bytes: 200,
            volatile_bytes: 50,
            estimated_tokens: 300,
            working_maximum: 4096,
            pressure_state: "normal".to_owned(),
            pressure_percent: 8,
            tools_available: 4,
            tools_gated: 2,
            tools_hidden: 0,
        };
        let fixture = Fixture::new("summary");
        let context =
            FeedContext { context: Some(&summary), ..fixture.context() };
        let lines =
            resolve_feed(&context, UiFeed::ContextSummary, &params(&[]))
                .expect("summarizes");
        let text = lines.join("\n");
        assert!(text.contains("mode: generic"), "{text:?}");
        assert!(text.contains("8%"), "{text:?}");
        assert!(text.contains("4 available, 2 gated, 0 hidden"), "{text:?}");
        assert!(context.context.is_some());
    }

    fn registered(name: &str, capability: &str) -> RegisteredToolInfo {
        RegisteredToolInfo {
            definition: ToolDefinition {
                name: name.to_owned(),
                description: format!("{name} tool"),
                input_schema: serde_json::json!({"type": "object"}),
            },
            capability: CapabilityId::parse(capability)
                .expect("test capability parses"),
        }
    }

    fn allow_all() -> PermissionPolicy {
        PermissionPolicy::from_rules([
            PolicyRule {
                capability: CapabilityId::parse("workspace.read")
                    .expect("parses"),
                rule: PermissionRule::Allow,
            },
            PolicyRule {
                capability: CapabilityId::parse("workspace.write")
                    .expect("parses"),
                rule: PermissionRule::Ask,
            },
        ])
    }

    #[test]
    fn tools_catalog_lists_registered_tools() {
        let tools = [
            registered("workspace.read", "workspace.read"),
            registered("write", "workspace.write"),
        ];
        let policy = allow_all();
        let fixture = Fixture::new("catalog");
        let context = FeedContext {
            tools: &tools,
            policy: &policy,
            ..fixture.context()
        };
        let lines = resolve_feed(&context, UiFeed::ToolsCatalog, &params(&[]))
            .expect("catalogs");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("workspace.read"), "{lines:?}");
        assert!(lines[0].contains("allowed"), "{lines:?}");
        assert!(lines[1].contains("approval required"), "{lines:?}");
    }

    #[test]
    fn modules_status_lists_install_records() {
        let modules = [ModuleRecord {
            id: "demo".to_owned(),
            path: "modules/demo".to_owned(),
            digest: format!("sha256:{}", "a".repeat(64)),
        }];
        let fixture = Fixture::new("modules");
        let context = FeedContext { modules: &modules, ..fixture.context() };
        let lines =
            resolve_feed(&context, UiFeed::ModulesStatus, &params(&[]))
                .expect("lists modules");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("demo"), "{lines:?}");
        assert!(lines[0].contains("modules/demo"), "{lines:?}");
    }

    #[test]
    fn checkpoints_feed_excludes_preimage_bytes() {
        let fixture = Fixture::new("checkpoints");
        // The feed opens its store under the same state dir the
        // tools checkpoint into: one root, one store view.
        let store = open_checkpoint_store(&fixture.root, &fixture.state_dir)
            .expect("store opens");
        let write = WriteTool::new(&fixture.root, store).expect("tool");
        let token = CancellationToken::new();
        let marker = "S3CR3T-PREIMAGE-BYTES-never-rendered";
        let outcome = write.execute(
            &serde_json::json!({
                "path": "secret.txt",
                "content": marker,
            }),
            token.signal(),
        );
        assert!(
            matches!(
                outcome,
                siralos_core::provider::ToolExecutionResult::Success { .. }
            ),
            "{outcome:?}"
        );
        let context = fixture.context();
        let lines =
            resolve_feed(&context, UiFeed::CheckpointsList, &params(&[]))
                .expect("lists checkpoints");
        assert_eq!(lines.len(), 1, "{lines:?}");
        let text = lines.join("\n");
        assert!(!text.contains(marker), "preimage bytes leaked: {text:?}");
        assert!(!text.contains(&"4".repeat(64)), "hash leaked: {text:?}");
        assert!(text.contains("secret.txt"), "{text:?}");
        assert!(text.contains("create"), "{text:?}");
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::CheckpointsList,
                &params(&[("limit", "0")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
        assert!(matches!(
            resolve_feed(
                &context,
                UiFeed::CheckpointsList,
                &params(&[("limit", "many")])
            ),
            Err(FeedError::InvalidParams(_))
        ));
        assert_eq!(MAX_FEED_CHECKPOINTS, 20);
    }
}
