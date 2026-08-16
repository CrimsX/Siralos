//! `workspace.list` / `workspace.read` / `workspace.search` Tool
//! adapters over the R4 workspace primitives.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use siralos_core::provider::{
    CancellationSignal, ToolDefinition, ToolExecutionResult,
};
use siralos_core::tool::{CapabilityId, Tool};
use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

use crate::workspace::list::{EntryKind, ListOutcome, list_directory};
use crate::workspace::read::{
    ReadInputError, ReadOutcome, parse_read_input, read_file,
};
use crate::workspace::root::{WorkspaceRootError, resolve_workspace_root};
use crate::workspace::search::{SearchOutcome, parse_search_input, search};

const WORKSPACE_READ_CAPABILITY: &str = "workspace.read";

fn workspace_read_capability() -> CapabilityId {
    CapabilityId::parse(WORKSPACE_READ_CAPABILITY)
        .expect("workspace.read is a valid capability id")
}

fn workspace_root(root: &Path) -> Result<PathBuf, WorkspaceRootError> {
    resolve_workspace_root(root)
}

/// The `workspace.list` Tool adapter.
pub struct WorkspaceListTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    root: PathBuf,
}

impl WorkspaceListTool {
    /// Construct the adapter over a canonicalized workspace root.
    pub fn new(root: &Path) -> Result<Self, WorkspaceRootError> {
        Ok(Self {
            definition: ToolDefinition {
                name: "workspace.list".to_owned(),
                description:
                    "List one directory within the approved workspace."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Directory path relative to the workspace root. Defaults to the workspace root."
                        }
                    },
                    "additionalProperties": false
                }),
            },
            capability: workspace_read_capability(),
            root: workspace_root(root)?,
        })
    }
}

impl Tool for WorkspaceListTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        let requested = match parse_list_input(input) {
            Ok(path) => path,
            Err(message) => {
                return ToolExecutionResult::InvalidInput { message };
            }
        };
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Listing was cancelled.".to_owned(),
            };
        }
        match list_directory(&self.root, &requested, &WORKSPACE_LIMITS) {
            ListOutcome::Denied { message } => {
                ToolExecutionResult::Denied { message }
            }
            ListOutcome::Failed { message } => {
                ToolExecutionResult::Failed { message }
            }
            ListOutcome::Success { path, entries, truncated } => {
                if cancellation.is_cancelled() {
                    return ToolExecutionResult::Cancelled {
                        message: "Listing was cancelled.".to_owned(),
                    };
                }
                let output_entries: Vec<Value> = entries
                    .into_iter()
                    .map(|entry| match entry.kind {
                        EntryKind::File { size } => json!({
                            "name": entry.name,
                            "path": entry.path,
                            "type": "file",
                            "size": size,
                        }),
                        EntryKind::Directory => json!({
                            "name": entry.name,
                            "path": entry.path,
                            "type": "directory",
                        }),
                        EntryKind::Symlink => json!({
                            "name": entry.name,
                            "path": entry.path,
                            "type": "symlink",
                        }),
                        EntryKind::Other => json!({
                            "name": entry.name,
                            "path": entry.path,
                            "type": "other",
                        }),
                    })
                    .collect();
                let count = output_entries.len();
                ToolExecutionResult::Success {
                    output: json!({
                        "path": path,
                        "entries": output_entries,
                        "truncated": truncated,
                    }),
                    summary: format!(
                        "{count} entries{}",
                        if truncated { " (truncated)" } else { "" }
                    ),
                }
            }
        }
    }
}

/// The `workspace.read` Tool adapter.
pub struct WorkspaceReadTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    root: PathBuf,
}

impl WorkspaceReadTool {
    /// Construct the adapter over a canonicalized workspace root.
    pub fn new(root: &Path) -> Result<Self, WorkspaceRootError> {
        Ok(Self {
            definition: ToolDefinition {
                name: "workspace.read".to_owned(),
                description: "Read one text file inside the workspace. Modes: exact (authoritative source for editing, returns a revision handle), structural (deterministic GDScript declarations), summary (bounded advisory overview). Summaries/structural views are never authoritative source."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path relative to the workspace root."
                        },
                        "startLine": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "One-based start line (exact mode). Defaults to 1."
                        },
                        "endLine": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Inclusive one-based end line (exact mode). Defaults to the last line."
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["exact", "structural", "summary"],
                            "description": "exact (default), structural, or summary."
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            },
            capability: workspace_read_capability(),
            root: workspace_root(root)?,
        })
    }
}

impl Tool for WorkspaceReadTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        let parsed = match parse_read_input(input) {
            Ok(parsed) => parsed,
            Err(error) => {
                return ToolExecutionResult::InvalidInput {
                    message: read_input_error_message(error),
                };
            }
        };
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Reading was cancelled.".to_owned(),
            };
        }
        match read_file(
            &self.root,
            &parsed,
            &WORKSPACE_LIMITS,
            None,
            cancellation.is_cancelled(),
        ) {
            ReadOutcome::InvalidInput { message } => {
                ToolExecutionResult::InvalidInput { message }
            }
            ReadOutcome::Denied { message } => {
                ToolExecutionResult::Denied { message }
            }
            ReadOutcome::Cancelled => ToolExecutionResult::Cancelled {
                message: "Reading was cancelled.".to_owned(),
            },
            ReadOutcome::Failed { message } => {
                ToolExecutionResult::Failed { message }
            }
            ReadOutcome::Unsupported {
                path,
                mode,
                revision,
                supported,
                reason,
            } => {
                if supported {
                    // GDScript structural/summary extraction is Godot
                    // language intelligence (R8/R9), not R7.2.
                    return ToolExecutionResult::Unavailable {
                        message: reason,
                    };
                }
                let mode = mode.as_str();
                ToolExecutionResult::Success {
                    output: json!({
                        "path": path,
                        "mode": mode,
                        "revision": revision,
                        "supported": false,
                        "reason": reason,
                    }),
                    summary: format!(
                        "{mode} read unsupported for this file type"
                    ),
                }
            }
            ReadOutcome::Success {
                path,
                sha256,
                revision,
                content,
                start_line,
                end_line,
                total_lines,
                truncated,
            } => {
                let lines = end_line - start_line + 1;
                ToolExecutionResult::Success {
                    output: json!({
                        "path": path,
                        "sha256": sha256,
                        "revision": revision,
                        "content": content,
                        "startLine": start_line,
                        "endLine": end_line,
                        "totalLines": total_lines,
                        "truncated": truncated,
                    }),
                    summary: format!(
                        "{lines} lines{}",
                        if truncated { " (truncated)" } else { "" }
                    ),
                }
            }
        }
    }
}

/// The `workspace.search` Tool adapter.
pub struct WorkspaceSearchTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    root: PathBuf,
}

impl WorkspaceSearchTool {
    /// Construct the adapter over a canonicalized workspace root.
    pub fn new(root: &Path) -> Result<Self, WorkspaceRootError> {
        Ok(Self {
            definition: ToolDefinition {
                name: "workspace.search".to_owned(),
                description: "Search text files recursively within a bounded workspace directory."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Literal text to search for (case-sensitive)."
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory path relative to the workspace root. Defaults to the workspace root."
                        },
                        "maxResults": {
                            "type": "integer",
                            "minimum": 1,
                            "description": "Maximum number of matches to return."
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            },
            capability: workspace_read_capability(),
            root: workspace_root(root)?,
        })
    }
}

impl Tool for WorkspaceSearchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        let parsed = match parse_search_input(input, &WORKSPACE_LIMITS) {
            Ok(parsed) => parsed,
            Err(message) => {
                return ToolExecutionResult::InvalidInput { message };
            }
        };
        if cancellation.is_cancelled() {
            return ToolExecutionResult::Cancelled {
                message: "Search was cancelled.".to_owned(),
            };
        }
        match search(
            &self.root,
            &parsed,
            &WORKSPACE_LIMITS,
            cancellation.is_cancelled(),
        ) {
            SearchOutcome::InvalidInput { message } => {
                ToolExecutionResult::InvalidInput { message }
            }
            SearchOutcome::Denied { message } => {
                ToolExecutionResult::Denied { message }
            }
            SearchOutcome::Cancelled => ToolExecutionResult::Cancelled {
                message: "Search was cancelled.".to_owned(),
            },
            SearchOutcome::Success {
                query,
                path,
                matches,
                scanned_files,
                skipped_files,
                truncated,
                truncation_reason,
            } => {
                let count = matches.len();
                let output_matches: Vec<Value> = matches
                    .into_iter()
                    .map(|entry| {
                        json!({
                            "path": entry.path,
                            "line": entry.line,
                            "column": entry.column,
                            "text": entry.text,
                        })
                    })
                    .collect();
                ToolExecutionResult::Success {
                    output: json!({
                        "query": query,
                        "path": path,
                        "matches": output_matches,
                        "scannedFiles": scanned_files,
                        "skippedFiles": skipped_files,
                        "truncated": truncated,
                        "truncationReason": truncation_reason
                            .map(|reason| reason.as_str()),
                    }),
                    summary: format!(
                        "{count} matches{}",
                        if truncated { " (truncated)" } else { "" }
                    ),
                }
            }
        }
    }
}

fn parse_list_input(input: &Value) -> Result<String, String> {
    let object = match input {
        Value::Object(object) => object,
        _ => return Err("Tool input must be a JSON object.".to_owned()),
    };
    match object.get("path") {
        None => Ok(".".to_owned()),
        Some(Value::String(path)) => Ok(path.clone()),
        Some(_) => Err("\"path\" must be a string.".to_owned()),
    }
}

fn read_input_error_message(error: ReadInputError) -> String {
    match error {
        ReadInputError::NotAnObject => {
            "Tool input must be a JSON object.".to_owned()
        }
        ReadInputError::MissingPath => "\"path\" is required.".to_owned(),
        ReadInputError::PathNotString => {
            "\"path\" must be a string.".to_owned()
        }
        ReadInputError::InvalidLineNumber(key) => {
            format!("\"{key}\" must be a positive integer.")
        }
        ReadInputError::EndBeforeStart => {
            "\"endLine\" must not precede \"startLine\".".to_owned()
        }
        ReadInputError::InvalidMode => {
            "\"mode\" must be \"exact\", \"structural\", or \"summary\"."
                .to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::json;
    use siralos_core::provider::{CancellationToken, ToolExecutionResult};
    use siralos_core::tool::Tool;

    use super::{WorkspaceListTool, WorkspaceReadTool, WorkspaceSearchTool};

    fn unique() -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    struct Fixture {
        root: std::path::PathBuf,
        before: BTreeMap<String, Vec<u8>>,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "siralos-tool-ws-{}-{}",
                std::process::id(),
                unique()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("a.txt"), b"hello\nworld\n").unwrap();
            fs::write(root.join("src/b.txt"), b"needle here\n").unwrap();
            let before = snapshot(&root);
            Self { root, before }
        }

        fn assert_unchanged(&self) {
            assert_eq!(snapshot(&self.root), self.before);
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn snapshot(root: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let key = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    files.insert(key, fs::read(&path).unwrap());
                }
            }
        }
        files
    }

    #[test]
    fn list_definition_capability_and_valid_output() {
        let fixture = Fixture::new();
        let tool =
            WorkspaceListTool::new(&fixture.root).expect("workspace root");
        assert_eq!(tool.definition().name, "workspace.list");
        assert_eq!(tool.capability().as_str(), "workspace.read");
        let token = CancellationToken::new();
        let result = tool.execute(&json!({}), token.signal());
        let ToolExecutionResult::Success { output, summary } = result else {
            panic!("list failed: {result:?}");
        };
        assert_eq!(summary, "2 entries");
        assert_eq!(output["path"], ".");
        assert_eq!(output["entries"].as_array().unwrap().len(), 2);
        fixture.assert_unchanged();
    }

    #[test]
    fn read_valid_input_returns_identity_and_lines() {
        let fixture = Fixture::new();
        let tool =
            WorkspaceReadTool::new(&fixture.root).expect("workspace root");
        let token = CancellationToken::new();
        let result = tool.execute(&json!({ "path": "a.txt" }), token.signal());
        let ToolExecutionResult::Success { output, summary } = result else {
            panic!("read failed: {result:?}");
        };
        assert_eq!(output["path"], "a.txt");
        assert_eq!(output["content"], "hello\nworld");
        assert_eq!(output["totalLines"], 2);
        assert_eq!(output["truncated"], false);
        assert_eq!(summary, "2 lines");
        fixture.assert_unchanged();
    }

    #[test]
    fn search_valid_input_returns_typed_matches() {
        let fixture = Fixture::new();
        let tool =
            WorkspaceSearchTool::new(&fixture.root).expect("workspace root");
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({ "query": "needle", "path": "." }),
            token.signal(),
        );
        let ToolExecutionResult::Success { output, summary } = result else {
            panic!("search failed: {result:?}");
        };
        assert_eq!(summary, "1 matches");
        assert_eq!(output["scannedFiles"], 2);
        assert_eq!(output["matches"][0]["path"], "src/b.txt");
        fixture.assert_unchanged();
    }

    #[test]
    fn invalid_inputs_are_rejected_without_workspace_effects() {
        let fixture = Fixture::new();
        let list = WorkspaceListTool::new(&fixture.root).unwrap();
        let read = WorkspaceReadTool::new(&fixture.root).unwrap();
        let search = WorkspaceSearchTool::new(&fixture.root).unwrap();
        let token = CancellationToken::new();
        assert!(matches!(
            list.execute(&json!([]), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(matches!(
            read.execute(&json!({}), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(matches!(
            read.execute(
                &json!({ "path": "a.txt", "startLine": 0 }),
                token.signal()
            ),
            ToolExecutionResult::InvalidInput { .. }
        ));
        assert!(matches!(
            search.execute(&json!({}), token.signal()),
            ToolExecutionResult::InvalidInput { .. }
        ));
        fixture.assert_unchanged();
    }

    #[test]
    fn cancelled_signals_are_observed_read_only() {
        let fixture = Fixture::new();
        let list = WorkspaceListTool::new(&fixture.root).unwrap();
        let read = WorkspaceReadTool::new(&fixture.root).unwrap();
        let search = WorkspaceSearchTool::new(&fixture.root).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            list.execute(&json!({}), token.signal()),
            ToolExecutionResult::Cancelled { .. }
        ));
        assert!(matches!(
            read.execute(&json!({ "path": "a.txt" }), token.signal()),
            ToolExecutionResult::Cancelled { .. }
        ));
        assert!(matches!(
            search.execute(&json!({ "query": "needle" }), token.signal()),
            ToolExecutionResult::Cancelled { .. }
        ));
        fixture.assert_unchanged();
    }
}
