//! `write` / `edit` Tool adapters over the atomic staging and
//! checkpoint lifecycle primitives.
//!
//! `write` creates one new text file (the target must be absent);
//! `edit` replaces one exact substring occurrence in an existing
//! text file (the old string must occur exactly once). Neither tool
//! deletes: undoing a creation stays unavailable (ADR 0038 D5).
//!
//! Every mutation runs the same host gate order: input validation,
//! cancellation, containment-safe resolution, excluded-directory and
//! protected-path refusal, contract bounds, checkpoint-before-mutation,
//! atomic compare-and-swap commit, post-commit verification, and
//! checkpoint state marking. The compare-and-swap commit narrows the
//! ordinary concurrent-writer window but is advisory, like every
//! pathname commit in this repository: resolution, staging, and rename
//! stay pathname-based, so a same-user process that substitutes the
//! target or a parent mid-operation can only cause refusal (conflict
//! or failure), never a misdirected write. Per-action approval is
//! enforced by the application executor ([`Tool::requires_approval`]),
//! never by these adapters: adapters never decide Host authorization.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use siralos_core::provider::{
    CancellationSignal, ToolDefinition, ToolExecutionResult,
};
use siralos_core::tool::{CapabilityId, Tool};
use siralos_core::workspace::bounds::WORKSPACE_LIMITS;

use crate::atomic::{AtomicWriteFailure, stage_atomic};
use crate::workspace::checkpoint::{
    CheckpointStore, CheckpointStoreError, open_checkpoint_store,
};
use crate::workspace::checkpoint_lifecycle::{
    CheckpointCreation, create_checkpoint, resolve_create_target,
};
use crate::workspace::fs::{
    BoundedFileRead, DEFAULT_EXCLUDED_DIRECTORIES, MUTATION_TEMP_PREFIX,
    is_model_protected_workspace_path, read_complete_file_bounded,
};
use crate::workspace::list::excluded_component;
use crate::workspace::resolve::{
    PathRejection, ResolvedWorkspacePath, resolve_workspace_path,
};
use crate::workspace::root::{WorkspaceRootError, resolve_workspace_root};
use siralos_core::identity::sha256_hex;
use siralos_core::workspace::checkpoint::{
    CheckpointFileState, CheckpointOperation, CheckpointPreview,
    CheckpointTerminalState,
};
use siralos_core::workspace::path::validate_relative_path;

const WORKSPACE_WRITE_CAPABILITY: &str = "workspace.write";

fn workspace_write_capability() -> CapabilityId {
    CapabilityId::parse(WORKSPACE_WRITE_CAPABILITY)
        .expect("workspace.write is a valid capability id")
}

/// Shared host-side mutation backend: the canonical root plus the
/// checkpoint store bound to the same workspace.
struct MutationBackend {
    root: PathBuf,
    store: CheckpointStore,
}

impl MutationBackend {
    /// Build the backend, refusing a checkpoint store bound to a
    /// different workspace (fail closed: checkpoints must never
    /// record one workspace while mutating another).
    fn new(
        root: &Path,
        store: CheckpointStore,
    ) -> Result<Self, WorkspaceRootError> {
        let canonical = resolve_workspace_root(root)?;
        if crate::workspace::checkpoint::fingerprint_of(&canonical)
            != store.workspace_fingerprint()
        {
            return Err(WorkspaceRootError::NotAccessible(
                "checkpoint store is bound to a different workspace"
                    .to_owned(),
            ));
        }
        Ok(Self { root: canonical, store })
    }

    /// Refuse excluded directories and protected paths with the
    /// stable read-parity messages (mutation wording).
    fn guarded(&self, relative: &str) -> Option<ToolExecutionResult> {
        if excluded_component(relative, &DEFAULT_EXCLUDED_DIRECTORIES)
            .is_some()
        {
            return Some(ToolExecutionResult::Denied {
                message: "Path is inside the excluded directory.".to_owned(),
            });
        }
        if is_model_protected_workspace_path(relative) {
            return Some(ToolExecutionResult::Denied {
                message:
                    "Path is protected from model-facing workspace mutation."
                        .to_owned(),
            });
        }
        None
    }
}

/// Map a checkpoint failure to a report-safe tool result.
///
/// Checkpoint messages carry no absolute paths (store roots are
/// host-only; relative paths are model-supplied), so the stable
/// text is safe to project.
fn checkpoint_failed(error: CheckpointStoreError) -> ToolExecutionResult {
    ToolExecutionResult::Failed { message: error.to_string() }
}

/// Map an atomic commit failure: a lost compare-and-swap race is a
/// conflict (newer work won); a symlink or special file at commit
/// time is denied (never followed); every other failure is a failure.
fn commit_failed(error: AtomicWriteFailure) -> ToolExecutionResult {
    if matches!(error, AtomicWriteFailure::TargetChanged { .. }) {
        ToolExecutionResult::Conflict {
            message: "The file changed while it was being written.".to_owned(),
        }
    } else if matches!(
        error,
        AtomicWriteFailure::TargetIsNotARegularFile { .. }
    ) {
        ToolExecutionResult::Denied {
            message: "Target must be a regular file; refusing symlink or special file."
                .to_owned(),
        }
    } else {
        ToolExecutionResult::Failed { message: error.to_string() }
    }
}

/// Resolve an existing-file target with read-parity rejections
/// (every resolution failure is a denial with the stable text).
fn resolve_existing(
    backend: &MutationBackend,
    path: &str,
) -> Result<ResolvedWorkspacePath, ToolExecutionResult> {
    resolve_workspace_path(&backend.root, path).map_err(|rejection| {
        ToolExecutionResult::Denied { message: rejection.to_string() }
    })
}

/// Read one complete text file for editing: regular file, bounded,
/// valid UTF-8. Anything else fails before any checkpoint exists.
fn read_edit_target(
    resolved: &ResolvedWorkspacePath,
) -> Result<(Vec<u8>, String), ToolExecutionResult> {
    let stats =
        std::fs::symlink_metadata(&resolved.absolute_path).map_err(|_| {
            ToolExecutionResult::Failed {
                message: "Cannot inspect file.".to_owned(),
            }
        })?;
    if stats.file_type().is_symlink() {
        return Err(ToolExecutionResult::Denied {
            message: "Editing through a symbolic link is denied.".to_owned(),
        });
    }
    if !stats.is_file() {
        return Err(ToolExecutionResult::Failed {
            message: "Target is not a regular file.".to_owned(),
        });
    }
    if stats.len() > WORKSPACE_LIMITS.max_text_file_size_bytes as u64 {
        return Err(ToolExecutionResult::Failed {
            message: format!(
                "File is too large ({} bytes; limit {}).",
                stats.len(),
                WORKSPACE_LIMITS.max_text_file_size_bytes
            ),
        });
    }
    let bytes = match read_complete_file_bounded(
        &resolved.absolute_path,
        WORKSPACE_LIMITS.max_text_file_size_bytes,
    ) {
        BoundedFileRead::Complete(bytes) => bytes,
        _ => {
            return Err(ToolExecutionResult::Failed {
                message: "Cannot inspect file.".to_owned(),
            });
        }
    };
    let text = String::from_utf8(bytes.clone()).map_err(|_| {
        ToolExecutionResult::Failed {
            message: "File is not valid UTF-8 text.".to_owned(),
        }
    })?;
    Ok((bytes, text))
}

fn preview_counts(before_lines: u64, after_lines: u64) -> CheckpointPreview {
    CheckpointPreview { added_lines: after_lines, removed_lines: before_lines }
}

fn count_lines(text: &str) -> u64 {
    if text.is_empty() { 0 } else { text.lines().count() as u64 }
}

/// The default `write` Tool: create one new text file.
///
/// Create-only: the target must be absent. Overwriting an existing
/// file is `edit` with the exact old content.
pub struct WriteTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    backend: MutationBackend,
}

impl WriteTool {
    /// Construct the adapter over a canonicalized workspace root
    /// with the checkpoint store bound to the same workspace.
    pub fn new(
        root: &Path,
        store: CheckpointStore,
    ) -> Result<Self, WorkspaceRootError> {
        Ok(Self {
            definition: ToolDefinition {
                name: "write".to_owned(),
                description:
                    "Create one new text file inside the workspace. Create-only: the target must not exist. Requires a one-time host approval for the exact bytes; the mutation is checkpointed before it applies."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path relative to the workspace root. Parent directories must exist."
                        },
                        "content": {
                            "type": "string",
                            "description": "Exact UTF-8 text content of the new file."
                        }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                }),
            },
            capability: workspace_write_capability(),
            backend: MutationBackend::new(root, store)?,
        })
    }

    /// Open the default checkpoint store for one workspace root.
    ///
    /// The store lives outside the workspace under the caller
    /// state directory; resolution failures are host-side
    /// configuration errors, not tool results.
    pub fn default_store(
        root: &Path,
    ) -> Result<CheckpointStore, CheckpointStoreError> {
        let canonical = resolve_workspace_root(root).map_err(|_| {
            CheckpointStoreError::RootUnavailable(
                "Checkpoint store paths cannot be resolved".to_owned(),
            )
        })?;
        let state = crate::paths::state_dir().map_err(|_| {
            CheckpointStoreError::RootUnavailable(
                "Checkpoint store root cannot be resolved".to_owned(),
            )
        })?;
        open_checkpoint_store(&canonical, &state.join("checkpoints"))
    }
}

impl Tool for WriteTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        execute_write(&self.backend, input, cancellation)
    }
}

fn parse_string_field(
    input: &Value,
    key: &str,
) -> Result<String, ToolExecutionResult> {
    let object = input.as_object().ok_or_else(|| {
        ToolExecutionResult::InvalidInput {
            message: "Input must be an object.".to_owned(),
        }
    })?;
    match object.get(key) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(ToolExecutionResult::InvalidInput {
            message: format!("{key} must be a string."),
        }),
        None => Err(ToolExecutionResult::InvalidInput {
            message: format!("{key} is required."),
        }),
    }
}

/// Shared creation-path execution behind `write`.
fn execute_write(
    backend: &MutationBackend,
    input: &Value,
    cancellation: CancellationSignal<'_>,
) -> ToolExecutionResult {
    let path = match parse_string_field(input, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    let content = match parse_string_field(input, "content") {
        Ok(content) => content,
        Err(result) => return result,
    };
    if cancellation.is_cancelled() {
        return ToolExecutionResult::Cancelled {
            message: "Writing was cancelled.".to_owned(),
        };
    }
    if content.len() > WORKSPACE_LIMITS.max_created_content_bytes {
        return ToolExecutionResult::InvalidInput {
            message: format!(
                "Content is too large ({} bytes; limit {}).",
                content.len(),
                WORKSPACE_LIMITS.max_created_content_bytes
            ),
        };
    }
    let target = match resolve_create_target(&backend.root, &path) {
        Ok(target) => target,
        Err(rejection) => {
            return ToolExecutionResult::Denied {
                message: rejection.to_string(),
            };
        }
    };
    let relative =
        create_relative(&backend.root, &target.parent, &target.leaf);
    let Some(relative) = relative else {
        return ToolExecutionResult::Denied {
            message: PathRejection::OutsideWorkspace.to_string(),
        };
    };
    if validate_relative_path(&relative).is_err() {
        return ToolExecutionResult::Denied {
            message: PathRejection::OutsideWorkspace.to_string(),
        };
    }
    if let Some(denied) = backend.guarded(&relative) {
        return denied;
    }
    let absolute = target.parent.join(&target.leaf);
    if std::fs::symlink_metadata(&absolute).is_ok() {
        return ToolExecutionResult::Conflict {
            message: "The file already exists; write creates new files only."
                .to_owned(),
        };
    }
    let bytes = content.as_bytes();
    let creation = CheckpointCreation {
        relative_path: relative.clone(),
        operation: CheckpointOperation::Create,
        tool_name: "write".to_owned(),
        created_at: host_timestamp(),
        before: CheckpointFileState {
            exists: false,
            sha256: None,
            byte_length: None,
        },
        before_bytes: None,
        after: CheckpointFileState {
            exists: true,
            sha256: Some(sha256_hex(bytes)),
            byte_length: Some(bytes.len() as u64),
        },
        preview: preview_counts(0, count_lines(&content)),
    };
    let checkpoint = match create_checkpoint(&backend.store, &creation) {
        Ok(checkpoint) => checkpoint,
        Err(error) => return checkpoint_failed(error),
    };
    let staged = match stage_atomic(
        &target.parent,
        &target.leaf,
        MUTATION_TEMP_PREFIX,
        bytes,
        Some(0o600),
    ) {
        Ok(staged) => staged,
        Err(error) => {
            // Best-effort: a failed mark leaves the record prepared,
            // and the before-state still holds, so reconciliation
            // heals it to abandoned.
            let _ = backend.store.mark_state(
                &checkpoint.id,
                CheckpointTerminalState::Abandoned,
            );
            return commit_failed(error);
        }
    };
    if let Err(error) = staged.commit_if_absent() {
        // Best-effort (see above): the failed commit changed no
        // bytes, so a prepared record still describes the truth.
        let _ = backend
            .store
            .mark_state(&checkpoint.id, CheckpointTerminalState::Abandoned);
        return commit_failed(error);
    }
    // A failed post-commit verification leaves the record prepared:
    // reconciliation classifies the true state (applied, abandoned,
    // or uncertain) instead of recording a guess.
    if let Err(result) = verify_written(&absolute, bytes) {
        return result;
    }
    // Best-effort: a failed mark leaves a prepared record whose
    // after-state holds, which reconciliation heals to applied.
    let _ = backend
        .store
        .mark_state(&checkpoint.id, CheckpointTerminalState::Applied);
    ToolExecutionResult::Success {
        output: json!({
            "path": relative,
            "sha256": sha256_hex(bytes),
            "bytes": bytes.len(),
            "checkpoint": checkpoint.id,
        }),
        summary: format!("created {} bytes", bytes.len()),
    }
}

/// Workspace-relative path of a creation target from the canonical
/// parent plus leaf (parent is already containment-checked).
fn create_relative(root: &Path, parent: &Path, leaf: &str) -> Option<String> {
    let relative_parent = parent.strip_prefix(root).ok()?;
    let mut components = Vec::new();
    for component in relative_parent.components() {
        if let std::path::Component::Normal(name) = component {
            components.push(name.to_string_lossy().into_owned());
        }
    }
    components.push(leaf.to_owned());
    Some(components.join("/"))
}

fn verify_written(
    absolute: &Path,
    expected: &[u8],
) -> Result<(), ToolExecutionResult> {
    match read_complete_file_bounded(absolute, expected.len()) {
        BoundedFileRead::Complete(bytes) if bytes == expected => Ok(()),
        _ => Err(ToolExecutionResult::Failed {
            message: "The written file could not be verified.".to_owned(),
        }),
    }
}

fn host_timestamp() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => {
            format!("{}.{:09}", duration.as_secs(), duration.subsec_nanos())
        }
        Err(_) => "0.0".to_owned(),
    }
}

/// The default `edit` Tool: replace one exact substring occurrence
/// in an existing text file.
///
/// Exact-once: the old string must occur exactly one time. Zero
/// matches (stale content) and ambiguous matches both refuse as
/// conflicts before any effect.
pub struct EditTool {
    definition: ToolDefinition,
    capability: CapabilityId,
    backend: MutationBackend,
}

impl EditTool {
    /// Construct the adapter over a canonicalized workspace root
    /// with the checkpoint store bound to the same workspace.
    pub fn new(
        root: &Path,
        store: CheckpointStore,
    ) -> Result<Self, WorkspaceRootError> {
        Ok(Self {
            definition: ToolDefinition {
                name: "edit".to_owned(),
                description:
                    "Replace one exact substring occurrence in an existing text file inside the workspace. The old string must occur exactly once; ambiguous or stale matches refuse. Requires a one-time host approval for the exact old and new strings; the mutation is checkpointed before it applies."
                        .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "File path relative to the workspace root."
                        },
                        "oldString": {
                            "type": "string",
                            "description": "Exact substring to replace. Must occur exactly once."
                        },
                        "newString": {
                            "type": "string",
                            "description": "Exact replacement text."
                        }
                    },
                    "required": ["path", "oldString", "newString"],
                    "additionalProperties": false
                }),
            },
            capability: workspace_write_capability(),
            backend: MutationBackend::new(root, store)?,
        })
    }
}

impl Tool for EditTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    fn requires_approval(&self) -> bool {
        true
    }

    fn execute(
        &self,
        input: &Value,
        cancellation: CancellationSignal<'_>,
    ) -> ToolExecutionResult {
        execute_edit(&self.backend, input, cancellation)
    }
}

/// Shared exact-replacement execution behind `edit`.
fn execute_edit(
    backend: &MutationBackend,
    input: &Value,
    cancellation: CancellationSignal<'_>,
) -> ToolExecutionResult {
    let path = match parse_string_field(input, "path") {
        Ok(path) => path,
        Err(result) => return result,
    };
    let old_string = match parse_string_field(input, "oldString") {
        Ok(old_string) => old_string,
        Err(result) => return result,
    };
    let new_string = match parse_string_field(input, "newString") {
        Ok(new_string) => new_string,
        Err(result) => return result,
    };
    if cancellation.is_cancelled() {
        return ToolExecutionResult::Cancelled {
            message: "Editing was cancelled.".to_owned(),
        };
    }
    if old_string.is_empty() {
        return ToolExecutionResult::InvalidInput {
            message: "oldString must not be empty.".to_owned(),
        };
    }
    for (key, value) in
        [("oldString", &old_string), ("newString", &new_string)]
    {
        if value.len() > WORKSPACE_LIMITS.max_replacement_text_bytes {
            return ToolExecutionResult::InvalidInput {
                message: format!(
                    "{key} is too large ({} bytes; limit {}).",
                    value.len(),
                    WORKSPACE_LIMITS.max_replacement_text_bytes
                ),
            };
        }
    }
    let resolved = match resolve_existing(backend, &path) {
        Ok(resolved) => resolved,
        Err(result) => return result,
    };
    if let Some(denied) = backend.guarded(&resolved.workspace_relative_path) {
        return denied;
    }
    let (current_bytes, current_text) = match read_edit_target(&resolved) {
        Ok(target) => target,
        Err(result) => return result,
    };
    let matches = current_text.match_indices(&old_string).count();
    if matches != 1 {
        return ToolExecutionResult::Conflict {
            message: if matches == 0 {
                "The old string does not occur in the current file content."
                    .to_owned()
            } else {
                format!(
                    "The old string is ambiguous ({matches} occurrences); exactly one is required."
                )
            },
        };
    }
    let new_text = current_text.replacen(&old_string, &new_string, 1);
    let new_bytes = new_text.as_bytes();
    let current_sha256 = sha256_hex(&current_bytes);
    let creation = CheckpointCreation {
        relative_path: resolved.workspace_relative_path.clone(),
        operation: CheckpointOperation::Update,
        tool_name: "edit".to_owned(),
        created_at: host_timestamp(),
        before: CheckpointFileState {
            exists: true,
            sha256: Some(current_sha256.clone()),
            byte_length: Some(current_bytes.len() as u64),
        },
        before_bytes: Some(current_bytes.clone()),
        after: CheckpointFileState {
            exists: true,
            sha256: Some(sha256_hex(new_bytes)),
            byte_length: Some(new_bytes.len() as u64),
        },
        preview: preview_counts(
            count_lines(&old_string),
            count_lines(&new_string),
        ),
    };
    let checkpoint = match create_checkpoint(&backend.store, &creation) {
        Ok(checkpoint) => checkpoint,
        Err(error) => return checkpoint_failed(error),
    };
    // The path resolved inside the canonical root, so a parent
    // exists; a non-UTF-8 leaf cannot be staged. Both leave the
    // record prepared for reconciliation to classify.
    let parent = match resolved.absolute_path.parent() {
        Some(parent) => parent.to_path_buf(),
        None => {
            return ToolExecutionResult::Failed {
                message: "Target has no parent directory.".to_owned(),
            };
        }
    };
    let leaf = match resolved
        .absolute_path
        .file_name()
        .and_then(|name| name.to_str())
    {
        Some(leaf) => leaf.to_owned(),
        None => {
            return ToolExecutionResult::Failed {
                message: "Target name is not UTF-8.".to_owned(),
            };
        }
    };
    let staged = match stage_atomic(
        &parent,
        &leaf,
        MUTATION_TEMP_PREFIX,
        new_bytes,
        Some(0o600),
    ) {
        Ok(staged) => staged,
        Err(error) => {
            // Best-effort: a failed mark leaves the record prepared,
            // and the before-state still holds, so reconciliation
            // heals it to abandoned.
            let _ = backend.store.mark_state(
                &checkpoint.id,
                CheckpointTerminalState::Abandoned,
            );
            return commit_failed(error);
        }
    };
    if let Err(error) = staged.commit_if_digest(&current_sha256) {
        // Best-effort (see above): the failed commit changed no
        // bytes, so a prepared record still describes the truth.
        let _ = backend
            .store
            .mark_state(&checkpoint.id, CheckpointTerminalState::Abandoned);
        return commit_failed(error);
    }
    // A failed post-commit verification leaves the record prepared:
    // reconciliation classifies the true state instead of recording
    // a guess.
    if let Err(result) = verify_written(&resolved.absolute_path, new_bytes) {
        return result;
    }
    // Best-effort: a failed mark leaves a prepared record whose
    // after-state holds, which reconciliation heals to applied.
    let _ = backend
        .store
        .mark_state(&checkpoint.id, CheckpointTerminalState::Applied);
    ToolExecutionResult::Success {
        output: json!({
            "path": resolved.workspace_relative_path,
            "sha256": sha256_hex(new_bytes),
            "bytes": new_bytes.len(),
            "checkpoint": checkpoint.id,
        }),
        summary: format!("replaced 1 occurrence ({} bytes)", new_bytes.len()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use siralos_core::provider::{CancellationToken, ToolExecutionResult};
    use siralos_core::tool::Tool;
    use siralos_core::workspace::checkpoint::CheckpointOperation;

    use super::{EditTool, WriteTool};
    use crate::test_support::unique;
    use crate::workspace::checkpoint::open_checkpoint_store;
    use crate::workspace::checkpoint_lifecycle::checkpoint_capacity;

    struct Fixture {
        root: std::path::PathBuf,
        store_root: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tag = format!("{}-{}", std::process::id(), unique());
            let root =
                std::env::temp_dir().join(format!("siralos-mut-ws-{tag}"));
            let store_root =
                std::env::temp_dir().join(format!("siralos-mut-st-{tag}"));
            let _ = fs::remove_dir_all(&root);
            let _ = fs::remove_dir_all(&store_root);
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(&store_root).unwrap();
            Self { root, store_root }
        }

        fn write_tool(&self) -> WriteTool {
            let store =
                open_checkpoint_store(&self.root, &self.store_root).unwrap();
            WriteTool::new(&self.root, store).expect("backend")
        }

        fn edit_tool(&self) -> EditTool {
            let store =
                open_checkpoint_store(&self.root, &self.store_root).unwrap();
            EditTool::new(&self.root, store).expect("backend")
        }

        fn checkpoint_count(&self) -> usize {
            let store =
                open_checkpoint_store(&self.root, &self.store_root).unwrap();
            checkpoint_capacity(&store).unwrap().checkpoints
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
            let _ = fs::remove_dir_all(&self.store_root);
        }
    }

    fn success_output(result: ToolExecutionResult) -> serde_json::Value {
        match result {
            ToolExecutionResult::Success { output, .. } => output,
            other => panic!("expected success: {other:?}"),
        }
    }

    #[test]
    fn tools_require_approval_and_share_write_capability() {
        let fixture = Fixture::new();
        let write = fixture.write_tool();
        let edit = fixture.edit_tool();
        assert_eq!(write.definition().name, "write");
        assert_eq!(edit.definition().name, "edit");
        assert_eq!(write.capability().as_str(), "workspace.write");
        assert_eq!(edit.capability().as_str(), "workspace.write");
        assert!(write.requires_approval());
        assert!(edit.requires_approval());
    }

    #[test]
    fn write_creates_file_with_checkpoint_and_identity() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        let output = success_output(tool.execute(
            &json!({"path": "new.txt", "content": "hello"}),
            token.signal(),
        ));
        assert_eq!(output["path"], "new.txt");
        assert_eq!(output["bytes"], 5);
        assert_eq!(
            output["sha256"],
            siralos_core::identity::sha256_hex(b"hello")
        );
        assert_eq!(fs::read(fixture.root.join("new.txt")).unwrap(), b"hello");
        let id = output["checkpoint"].as_str().unwrap();
        let store =
            open_checkpoint_store(&fixture.root, &fixture.store_root).unwrap();
        let record = store.get(id).expect("checkpoint stored");
        assert_eq!(record.operation, CheckpointOperation::Create);
        assert_eq!(
            record.state,
            siralos_core::workspace::checkpoint::CheckpointState::Applied
        );
        assert!(!record.before.exists);
        assert_eq!(
            record.after.sha256.as_deref(),
            Some(output["sha256"].as_str().unwrap())
        );
    }

    #[test]
    fn write_refuses_existing_target_as_conflict() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("taken.txt"), b"original").unwrap();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({"path": "taken.txt", "content": "other"}),
            token.signal(),
        );
        assert!(matches!(
            result,
            ToolExecutionResult::Conflict { message }
                if message == "The file already exists; write creates new files only."
        ));
        assert_eq!(
            fs::read(fixture.root.join("taken.txt")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn write_rejects_traversal_absolute_nul_and_empty_paths() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        for (path, message) in [
            ("../escape.txt", "Path is outside the Siralos workspace."),
            ("/etc/hosts", "Path must be relative to the workspace."),
            ("a\0b", "Path contains a null byte."),
            ("", "Path is empty."),
            (".", "Path contains an invalid character."),
        ] {
            let result = tool.execute(
                &json!({"path": path, "content": "x"}),
                token.signal(),
            );
            assert!(
                matches!(result, ToolExecutionResult::Denied { .. }),
                "{path}"
            );
            if let ToolExecutionResult::Denied { message: actual } = result {
                assert_eq!(actual, message, "{path}");
            }
        }
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_parent_symlink_escape() {
        let fixture = Fixture::new();
        let outside = std::env::temp_dir().join(format!(
            "siralos-mut-out-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join("link"))
            .unwrap();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({"path": "link/secret.txt", "content": "x"}),
            token.signal(),
        );
        assert!(matches!(
            result,
            ToolExecutionResult::Denied { message }
                if message == "Path is outside the Siralos workspace."
        ));
        assert!(!outside.join("secret.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn write_refuses_protected_and_excluded_paths() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("sub")).unwrap();
        fs::create_dir_all(fixture.root.join(".siralos")).unwrap();
        fs::create_dir_all(fixture.root.join(".git")).unwrap();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        for (path, message) in [
            (
                "AGENTS.md",
                "Path is protected from model-facing workspace mutation.",
            ),
            (
                "sub/AGENTS.md",
                "Path is protected from model-facing workspace mutation.",
            ),
            (
                "siralos.toml",
                "Path is protected from model-facing workspace mutation.",
            ),
            (
                ".siralos/notes.txt",
                "Path is protected from model-facing workspace mutation.",
            ),
            (".git/x.txt", "Path is inside the excluded directory."),
        ] {
            let result = tool.execute(
                &json!({"path": path, "content": "x"}),
                token.signal(),
            );
            assert!(
                matches!(
                    result,
                    ToolExecutionResult::Denied { message: ref actual }
                        if actual == message
                ),
                "{path}: {result:?}"
            );
        }
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn write_rejects_oversize_content() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        let big = "x".repeat(
            siralos_core::workspace::bounds::WORKSPACE_LIMITS
                .max_created_content_bytes
                + 1,
        );
        let result = tool.execute(
            &json!({"path": "big.txt", "content": big}),
            token.signal(),
        );
        assert!(matches!(result, ToolExecutionResult::InvalidInput { .. }));
        assert!(!fixture.root.join("big.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn write_requires_object_and_string_fields() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        for input in [
            json!(42),
            json!({"content": "x"}),
            json!({"path": "a.txt"}),
            json!({"path": 42, "content": "x"}),
            json!({"path": "a.txt", "content": 42}),
        ] {
            assert!(
                matches!(
                    tool.execute(&input, token.signal()),
                    ToolExecutionResult::InvalidInput { .. }
                ),
                "{input}"
            );
        }
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn write_observes_cancellation_before_effect() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        token.cancel();
        let result = tool.execute(
            &json!({"path": "cancelled.txt", "content": "x"}),
            token.signal(),
        );
        assert!(matches!(result, ToolExecutionResult::Cancelled { .. }));
        assert!(!fixture.root.join("cancelled.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn write_supports_nested_directories_and_refuses_missing_parent() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("sub")).unwrap();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        let output = success_output(tool.execute(
            &json!({"path": "sub/nested.txt", "content": "deep"}),
            token.signal(),
        ));
        assert_eq!(output["path"], "sub/nested.txt");
        assert_eq!(
            fs::read(fixture.root.join("sub/nested.txt")).unwrap(),
            b"deep"
        );
        let result = tool.execute(
            &json!({"path": "missing/nested.txt", "content": "x"}),
            token.signal(),
        );
        assert!(matches!(result, ToolExecutionResult::Denied { .. }));
    }

    #[test]
    fn edit_replaces_exactly_once_and_checkpoints() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a.txt"), b"hello world").unwrap();
        let tool = fixture.edit_tool();
        let token = CancellationToken::new();
        let output = success_output(tool.execute(
            &json!({
                "path": "a.txt",
                "oldString": "world",
                "newString": "there"
            }),
            token.signal(),
        ));
        assert_eq!(output["path"], "a.txt");
        assert_eq!(
            fs::read(fixture.root.join("a.txt")).unwrap(),
            b"hello there"
        );
        let id = output["checkpoint"].as_str().unwrap();
        let store =
            open_checkpoint_store(&fixture.root, &fixture.store_root).unwrap();
        let record = store.get(id).expect("checkpoint stored");
        assert_eq!(record.operation, CheckpointOperation::Update);
        assert_eq!(
            record.state,
            siralos_core::workspace::checkpoint::CheckpointState::Applied
        );
        assert_eq!(
            record.before.sha256.as_deref(),
            Some(siralos_core::identity::sha256_hex(b"hello world").as_str())
        );
        assert_eq!(
            record.after.sha256.as_deref(),
            Some(siralos_core::identity::sha256_hex(b"hello there").as_str())
        );
    }

    #[test]
    fn edit_refuses_zero_and_ambiguous_matches() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a.txt"), b"one two one").unwrap();
        let tool = fixture.edit_tool();
        let token = CancellationToken::new();
        let stale = tool.execute(
            &json!({
                "path": "a.txt",
                "oldString": "absent",
                "newString": "x"
            }),
            token.signal(),
        );
        assert!(matches!(
            stale,
            ToolExecutionResult::Conflict { message }
                if message
                    == "The old string does not occur in the current file content."
        ));
        let ambiguous = tool.execute(
            &json!({"path": "a.txt", "oldString": "one", "newString": "x"}),
            token.signal(),
        );
        assert!(matches!(
            ambiguous,
            ToolExecutionResult::Conflict { message }
                if message.contains("ambiguous (2 occurrences)")
        ));
        assert_eq!(
            fs::read(fixture.root.join("a.txt")).unwrap(),
            b"one two one"
        );
    }

    #[test]
    fn edit_rejects_empty_old_string_and_oversize_strings() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a.txt"), b"content").unwrap();
        let tool = fixture.edit_tool();
        let token = CancellationToken::new();
        let empty = tool.execute(
            &json!({"path": "a.txt", "oldString": "", "newString": "x"}),
            token.signal(),
        );
        assert!(matches!(
            empty,
            ToolExecutionResult::InvalidInput { message }
                if message == "oldString must not be empty."
        ));
        let big = "y".repeat(
            siralos_core::workspace::bounds::WORKSPACE_LIMITS
                .max_replacement_text_bytes
                + 1,
        );
        let oversize = tool.execute(
            &json!({"path": "a.txt", "oldString": "content", "newString": big}),
            token.signal(),
        );
        assert!(matches!(oversize, ToolExecutionResult::InvalidInput { .. }));
        assert_eq!(fs::read(fixture.root.join("a.txt")).unwrap(), b"content");
    }

    #[test]
    fn edit_refuses_symlink_directory_and_non_utf8_targets() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.join("adir")).unwrap();
        fs::write(fixture.root.join("binary.dat"), [0xff, 0xfe, 0x00])
            .unwrap();
        let tool = fixture.edit_tool();
        let token = CancellationToken::new();
        let directory = tool.execute(
            &json!({"path": "adir", "oldString": "a", "newString": "b"}),
            token.signal(),
        );
        assert!(matches!(
            directory,
            ToolExecutionResult::Failed { message }
                if message == "Target is not a regular file."
        ));
        let binary = tool.execute(
            &json!({"path": "binary.dat", "oldString": "a", "newString": "b"}),
            token.signal(),
        );
        assert!(matches!(
            binary,
            ToolExecutionResult::Failed { message }
                if message == "File is not valid UTF-8 text."
        ));
    }

    #[cfg(unix)]
    #[test]
    fn edit_refuses_symlink_targets() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("real.txt"), b"real content").unwrap();
        std::os::unix::fs::symlink(
            fixture.root.join("real.txt"),
            fixture.root.join("alias.txt"),
        )
        .unwrap();
        let tool = fixture.edit_tool();
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({
                "path": "alias.txt",
                "oldString": "real",
                "newString": "fake"
            }),
            token.signal(),
        );
        assert!(matches!(result, ToolExecutionResult::Denied { .. }));
        assert_eq!(
            fs::read(fixture.root.join("real.txt")).unwrap(),
            b"real content"
        );
    }

    #[test]
    fn edit_missing_file_is_denied() {
        let fixture = Fixture::new();
        let tool = fixture.edit_tool();
        let token = CancellationToken::new();
        let result = tool.execute(
            &json!({"path": "ghost.txt", "oldString": "a", "newString": "b"}),
            token.signal(),
        );
        assert!(matches!(
            result,
            ToolExecutionResult::Denied { message }
                if message == "Path cannot be resolved."
        ));
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn write_checkpoint_undo_is_delete_unavailable() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let token = CancellationToken::new();
        let output = success_output(tool.execute(
            &json!({"path": "doomed.txt", "content": "temporary"}),
            token.signal(),
        ));
        let id = output["checkpoint"].as_str().unwrap().to_owned();
        let store =
            open_checkpoint_store(&fixture.root, &fixture.store_root).unwrap();
        let outcome = crate::workspace::checkpoint_lifecycle::apply_undo(
            &store,
            &id,
            1024 * 1024,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            crate::workspace::checkpoint_lifecycle::UndoApplyOutcome::DeleteUnavailable { .. }
        ));
        assert_eq!(
            fs::read(fixture.root.join("doomed.txt")).unwrap(),
            b"temporary"
        );
    }
}
