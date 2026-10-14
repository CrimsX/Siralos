//! `write` / `edit` Tool adapters over the atomic staging and
//! checkpoint lifecycle primitives.
//!
//! `write` creates one new text file (the target must be absent);
//! `edit` replaces one exact substring occurrence in an existing
//! text file (the old string must occur exactly once). Neither tool
//! deletes: undoing a creation stays unavailable (ADR 0038 D5).
//!
//! Every mutation runs the same host gate order: input validation,
//! cancellation, containment-safe resolution, requested-name
//! stability, excluded-directory and protected-path refusal,
//! contract bounds, checkpoint-before-mutation, parent revalidation,
//! atomic compare-and-swap commit, post-commit verification (exact
//! bytes, parent still contained, target not a link), and
//! checkpoint state marking. A target or parent swapped
//! outside the final rename and verify-then-report gaps is detected
//! and refused (conflict, failure, or denial) with no success claim;
//! the two residual gaps are pathname-inherent on this platform (no
//! directory-relative primitives), so a swap landing inside them can
//! only be bounded after the fact: the checkpoint stays prepared and
//! reconciliation classifies the true state instead of recording a
//! guess. The rename-inside-resolution gap is closed, not residual:
//! the resolved path must still name the request, so an approval can
//! never be evaded by a same-object rename. Per-action approval is
//! enforced by the application executor ([`Tool::requires_approval`]),
//! never by these adapters: adapters never decide Host authorization.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use siralos_core::provider::{
    CancellationSignal, ToolDefinition, ToolExecutionResult,
};
use siralos_core::tool::{ApprovalPreview, CapabilityId, Tool};
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
    fold_path_component, is_case_insensitive_platform,
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
            return Err(WorkspaceRootError::StoreBindingMismatch);
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
/// conflict (newer work won); a parent that moved under the write is
/// a conflict (never retried into the substituted parent); a symlink
/// or special file at commit time is denied (never followed); every
/// other failure is a failure.
fn commit_failed(error: AtomicWriteFailure) -> ToolExecutionResult {
    if matches!(error, AtomicWriteFailure::TargetChanged { .. }) {
        ToolExecutionResult::Conflict {
            message: "The file changed while it was being written.".to_owned(),
        }
    } else if matches!(error, AtomicWriteFailure::ParentChanged { .. }) {
        parent_moved()
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

/// Re-check that a resolved parent directory still resolves inside
/// the canonical workspace root.
///
/// A cheap containment pre-check before staging and committing: it
/// refuses vanished or escaped parents early, before any temporary
/// is created outside the workspace. It cannot see a same-name
/// replacement, and it is not the authoritative gate — the commit
/// itself re-verifies the retained parent identity immediately
/// before the rename. Failures report the typed commit error (mapped
/// to a refusal by the caller), never a stringly refusal.
fn revalidate_parent(
    root: &Path,
    parent: &Path,
) -> Result<(), AtomicWriteFailure> {
    let canonical = std::fs::canonicalize(parent).map_err(|_| {
        AtomicWriteFailure::ParentChanged { path: parent.to_path_buf() }
    })?;
    if canonical == root || canonical.starts_with(root) {
        Ok(())
    } else {
        Err(AtomicWriteFailure::ParentChanged { path: parent.to_path_buf() })
    }
}

fn parent_moved() -> ToolExecutionResult {
    ToolExecutionResult::Conflict {
        message: "The target directory changed while it was being written."
            .to_owned(),
    }
}

/// Require the resolution to still name the requested path.
///
/// Windows canonicalization opens the target and queries its live
/// name, so a parent renamed inside `canonicalize` resolves the
/// same directory object under its NEW name (`hot` becomes
/// `hot-parked`). Containment still passes (the object never left
/// the root), but the approval bound the exact requested bytes to
/// the REQUESTED path — staging or committing at the new name
/// would evade the approval while honestly reporting a path the
/// caller never asked for. Refuse unless the resolved absolute
/// path still names the request (lexically normalized, case-folded
/// on folding platforms). This is the name half of the
/// substitution gate; the commit-time parent-identity check is the
/// object half (same name, replaced object).
fn require_requested_parent_name(
    root: &Path,
    requested: &str,
    resolved: &Path,
) -> Result<(), AtomicWriteFailure> {
    let failed =
        || AtomicWriteFailure::ParentChanged { path: resolved.to_path_buf() };
    let fold = is_case_insensitive_platform();
    let relative = resolved.strip_prefix(root).map_err(|_| failed())?;
    let mut live = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(name) => {
                live.push(fold_path_component(&name.to_string_lossy(), fold));
            }
            _ => return Err(failed()),
        }
    }
    let mut asked = Vec::new();
    for component in requested.split(['/', '\\']) {
        match component {
            "" | "." => {}
            ".." => {
                asked.pop();
            }
            name => asked.push(fold_path_component(name, fold)),
        }
    }
    if asked == live { Ok(()) } else { Err(failed()) }
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

fn preview_counts(removed_lines: u64, added_lines: u64) -> CheckpointPreview {
    CheckpointPreview { added_lines, removed_lines }
}

/// Maximum content lines carried by one approval preview. This is
/// the content layer only: the render layer caps detail lines at
/// `MAX_APPROVAL_PREVIEW_LINES` (24, core) and the modal fits 30
/// lines including its framing, so the preview leaves room for both.
const PREVIEW_MAX_LINES: usize = 20;
/// Maximum characters per preview line (readable width; the modal
/// truncates again at its own bound).
const PREVIEW_MAX_LINE_CHARS: usize = 160;

/// Truncate one preview line to the presentation bound.
fn preview_line(text: &str) -> String {
    if text.chars().count() > PREVIEW_MAX_LINE_CHARS {
        let kept: String = text.chars().take(PREVIEW_MAX_LINE_CHARS).collect();
        format!("{kept}…")
    } else {
        text.to_owned()
    }
}

/// Bounded content lines with an exact omitted-line count.
fn preview_content_lines(text: &str) -> Vec<String> {
    let total = count_lines(text);
    let mut lines: Vec<String> =
        text.lines().take(PREVIEW_MAX_LINES).map(preview_line).collect();
    if total > lines.len() as u64 {
        lines.push(format!("… ({} more lines)", total - lines.len() as u64));
    }
    lines
}

/// Reviewable preview for a `write` proposal: exact target, byte
/// identity, and bounded content. Read-only; a target that cannot
/// be resolved is still previewed (execution will refuse it).
fn write_preview(path: &str, content: &str) -> ApprovalPreview {
    let bytes = content.as_bytes();
    let total = count_lines(content);
    let mut lines = vec![
        format!("path: {path}"),
        format!(
            "create new file, {} bytes, sha256 {}",
            bytes.len(),
            sha256_hex(bytes)
        ),
        format!("content preview ({total} lines):"),
    ];
    lines.extend(preview_content_lines(content));
    ApprovalPreview { title: format!("write {path}"), lines }
}

/// Reviewable preview for an `edit` proposal: exact target, current
/// match count, and both bounded strings. The match count is
/// advisory (the tool re-verifies exact-once at execution); an
/// unreadable target is reported, not hidden.
fn edit_preview(
    root: &Path,
    path: &str,
    old_string: &str,
    new_string: &str,
) -> ApprovalPreview {
    let mut lines = vec![format!("path: {path}")];
    match resolve_workspace_path(root, path)
        .ok()
        .and_then(|resolved| read_edit_target(&resolved).ok())
    {
        Some((_, current)) => {
            let matches = current.match_indices(old_string).count();
            lines.push(format!(
                "replace exactly once ({matches} match(es) in current content)"
            ));
        }
        None => {
            lines.push(
                "current content unavailable (execution will refuse)"
                    .to_owned(),
            );
        }
    }
    lines.push(format!("old ({} lines):", count_lines(old_string)));
    lines.extend(preview_content_lines(old_string));
    lines.push(format!("new ({} lines):", count_lines(new_string)));
    lines.extend(preview_content_lines(new_string));
    ApprovalPreview { title: format!("edit {path}"), lines }
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

    /// Reviewable preview of a `write` proposal. Malformed input
    /// yields no preview (the gate denies unreviewable calls; the
    /// precise shape error stays with direct execution).
    fn approval_preview(&self, input: &Value) -> Option<ApprovalPreview> {
        let path = parse_string_field(input, "path").ok()?;
        let content = parse_string_field(input, "content").ok()?;
        Some(write_preview(&path, &content))
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
    // The parent may have been renamed inside resolution itself
    // (same object, new name): refuse before any checkpoint exists
    // so the approval stays bound to the requested path.
    let requested_parent = match path.rfind('/') {
        Some(index) => &path[..index],
        None => ".",
    };
    if let Err(error) = require_requested_parent_name(
        &backend.root,
        requested_parent,
        &target.parent,
    ) {
        return commit_failed(error);
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
    let Some(created_at) = host_timestamp() else {
        return ToolExecutionResult::Failed {
            message: "Host clock is unavailable; the mutation is refused without a creation timestamp."
                .to_owned(),
        };
    };
    let creation = CheckpointCreation {
        relative_path: relative.clone(),
        operation: CheckpointOperation::Create,
        tool_name: "write".to_owned(),
        created_at,
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
    // The parent resolved inside the root above; recheck it
    // immediately before staging so a swapped parent refuses here
    // instead of staging outside the workspace.
    if let Err(error) = revalidate_parent(&backend.root, &target.parent) {
        let _ = backend
            .store
            .mark_state(&checkpoint.id, CheckpointTerminalState::Abandoned);
        return commit_failed(error);
    }
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
    // Recheck again before the irreversible rename: staging took
    // observable time, and a parent swapped during it must refuse
    // rather than commit outside the workspace.
    if let Err(error) = revalidate_parent(&backend.root, &target.parent) {
        let _ = backend
            .store
            .mark_state(&checkpoint.id, CheckpointTerminalState::Abandoned);
        return commit_failed(error);
    }
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
    if let Err(result) = verify_committed(&backend.root, &absolute, bytes) {
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

/// Verify a committed mutation: exact bytes, a parent that still
/// resolves inside the workspace root, and a target that is not a
/// link.
///
/// Byte verification alone would read back attacker-redirected
/// content as success after a parent swap, so the parent
/// containment recheck and the no-link check are part of the same
/// verdict. A swap landing between this verification and the
/// report is outside every check (see [`revalidate_parent`]); the
/// checkpoint stays prepared then, and reconciliation classifies
/// the true state instead of recording a guess.
fn verify_committed(
    root: &Path,
    absolute: &Path,
    expected: &[u8],
) -> Result<(), ToolExecutionResult> {
    let stats = std::fs::symlink_metadata(absolute).map_err(|_| {
        ToolExecutionResult::Failed {
            message: "The written file could not be verified.".to_owned(),
        }
    })?;
    if stats.file_type().is_symlink() || !stats.is_file() {
        return Err(ToolExecutionResult::Denied {
            message: "Target must be a regular file; refusing symlink or special file."
                .to_owned(),
        });
    }
    if let Some(parent) = absolute.parent() {
        if let Err(error) = revalidate_parent(root, parent) {
            return Err(commit_failed(error));
        }
    }
    match read_complete_file_bounded(absolute, expected.len()) {
        BoundedFileRead::Complete(bytes) if bytes == expected => Ok(()),
        _ => Err(ToolExecutionResult::Failed {
            message: "The written file could not be verified.".to_owned(),
        }),
    }
}

/// Host creation timestamp for a checkpoint record, if the system
/// clock can supply one.
///
/// A pre-epoch clock is a broken-host condition, not a defaultable
/// one: a `"0.0"` sentinel would sort oldest and get pruned first,
/// so callers refuse the mutation instead.
fn host_timestamp() -> Option<String> {
    format_timestamp(std::time::SystemTime::now())
}

/// Pure timestamp formatting (testable clock edge).
fn format_timestamp(now: std::time::SystemTime) -> Option<String> {
    now.duration_since(std::time::UNIX_EPOCH).ok().map(|duration| {
        format!("{}.{:09}", duration.as_secs(), duration.subsec_nanos())
    })
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

    /// Reviewable preview of an `edit` proposal (match count is
    /// advisory; malformed input yields no preview — see `write`).
    fn approval_preview(&self, input: &Value) -> Option<ApprovalPreview> {
        let path = parse_string_field(input, "path").ok()?;
        let old_string = parse_string_field(input, "oldString").ok()?;
        let new_string = parse_string_field(input, "newString").ok()?;
        Some(edit_preview(&self.backend.root, &path, &old_string, &new_string))
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
    // Same rename-inside-resolution gate as creation: the resolved
    // file must still carry the requested name, or the approval is
    // evaded (the read below would otherwise inspect, and the
    // commit would rewrite, a path the caller never named).
    if let Err(error) = require_requested_parent_name(
        &backend.root,
        &path,
        &resolved.absolute_path,
    ) {
        return commit_failed(error);
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
    let Some(created_at) = host_timestamp() else {
        return ToolExecutionResult::Failed {
            message: "Host clock is unavailable; the mutation is refused without a creation timestamp."
                .to_owned(),
        };
    };
    let creation = CheckpointCreation {
        relative_path: resolved.workspace_relative_path.clone(),
        operation: CheckpointOperation::Update,
        tool_name: "edit".to_owned(),
        created_at,
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
    // The file content was read through this parent; recheck it
    // before staging so a swapped parent refuses here instead of
    // staging outside the workspace.
    if let Err(error) = revalidate_parent(&backend.root, &parent) {
        let _ = backend
            .store
            .mark_state(&checkpoint.id, CheckpointTerminalState::Abandoned);
        return commit_failed(error);
    }
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
    // Recheck again before the irreversible rename: staging took
    // observable time, and a parent swapped during it must refuse
    // rather than commit outside the workspace.
    if let Err(error) = revalidate_parent(&backend.root, &parent) {
        let _ = backend
            .store
            .mark_state(&checkpoint.id, CheckpointTerminalState::Abandoned);
        return commit_failed(error);
    }
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
    if let Err(result) =
        verify_committed(&backend.root, &resolved.absolute_path, new_bytes)
    {
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

    #[test]
    fn mismatched_checkpoint_store_is_a_typed_refusal() {
        let first = Fixture::new();
        let second = Fixture::new();
        let foreign =
            open_checkpoint_store(&second.root, &second.store_root).unwrap();
        assert!(matches!(
            WriteTool::new(&first.root, foreign),
            Err(crate::workspace::root::WorkspaceRootError::StoreBindingMismatch)
        ));
        let foreign =
            open_checkpoint_store(&second.root, &second.store_root).unwrap();
        assert!(matches!(
            EditTool::new(&first.root, foreign),
            Err(crate::workspace::root::WorkspaceRootError::StoreBindingMismatch)
        ));
        assert_eq!(
            crate::workspace::root::WorkspaceRootError::StoreBindingMismatch
                .to_string(),
            "Checkpoint store is bound to a different workspace."
        );
    }

    #[test]
    fn write_preview_carries_identity_and_bounded_content() {
        use super::PREVIEW_MAX_LINES;
        use siralos_core::tool::Tool as _;
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let content = (0..PREVIEW_MAX_LINES + 5)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let preview = tool
            .approval_preview(&json!({"path": "n.txt", "content": content}))
            .expect("well-formed input previews");
        assert_eq!(preview.title, "write n.txt");
        assert!(
            preview.lines.iter().any(|line| line.starts_with("path: n.txt"))
        );
        let digest = siralos_core::identity::sha256_hex(content.as_bytes());
        assert!(preview.lines.iter().any(|line| line.contains(&digest)));
        assert!(preview.lines.iter().any(|line| line.contains("more lines")));
        assert!(preview.lines.len() <= PREVIEW_MAX_LINES + 4);
        assert!(
            tool.approval_preview(&json!({"path": "n.txt"})).is_none(),
            "malformed input has no preview"
        );
    }

    #[test]
    fn edit_preview_reports_match_count_and_bounded_strings() {
        use siralos_core::tool::Tool as _;
        let fixture = Fixture::new();
        fs::write(fixture.root.join("m.txt"), b"one two one").unwrap();
        let tool = fixture.edit_tool();
        let preview = tool
            .approval_preview(&json!({
                "path": "m.txt",
                "oldString": "one",
                "newString": "1\n2\n3"
            }))
            .expect("well-formed input previews");
        assert_eq!(preview.title, "edit m.txt");
        assert!(
            preview.lines.iter().any(|line| line.contains("2 match(es)")),
            "{preview:?}"
        );
        assert!(preview.lines.iter().any(|line| line == "old (1 lines):"));
        assert!(preview.lines.iter().any(|line| line == "new (3 lines):"));
        let missing = tool
            .approval_preview(&json!({
                "path": "ghost.txt",
                "oldString": "a",
                "newString": "b"
            }))
            .expect("unreadable target still previews");
        assert!(missing.lines.iter().any(|line| line.contains("unavailable")));
    }

    #[test]
    fn timestamp_formats_epoch_and_refuses_pre_epoch_clocks() {
        let epoch = std::time::UNIX_EPOCH;
        assert_eq!(
            super::format_timestamp(epoch),
            Some("0.000000000".to_owned())
        );
        assert_eq!(
            super::format_timestamp(
                epoch + std::time::Duration::new(7, 1_200)
            ),
            Some("7.000001200".to_owned())
        );
        assert_eq!(
            super::format_timestamp(epoch - std::time::Duration::new(1, 0)),
            None
        );
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

    // --- A5: approval-gate proofs through the executor/round path ---
    //
    // The tests above call `tool.execute()` directly, which bypasses
    // the application gate by construction (adapters never decide
    // Host authorization). The tests below drive the same tools
    // through `SiralosApplication`, proving the gate refuses without
    // a recorded approval and executes only on an exact digest match.

    use std::cell::Cell;

    use siralos_core::provider::{
        CancellationSignal, ConversationItem, ModelEvent, ModelProvider,
        ModelRequest, ProviderEvent, ToolCallInput,
    };
    use siralos_core::tool::{
        ActionApproval, PermissionPolicy, PermissionRule, PolicyRule,
        SiralosApplication, ToolLoopEvent, ToolRegistry, action_digest,
        canonical_input_json,
    };

    struct ScriptProvider {
        turns: Vec<Vec<ProviderEvent>>,
        next: Cell<usize>,
    }

    impl ScriptProvider {
        fn new(turns: Vec<Vec<ProviderEvent>>) -> Self {
            Self { turns, next: Cell::new(0) }
        }
    }

    impl ModelProvider for ScriptProvider {
        type Stream<'a>
            = std::vec::IntoIter<ProviderEvent>
        where
            Self: 'a;

        fn id(&self) -> &str {
            "script-test"
        }

        fn stream<'a>(
            &'a self,
            _request: &'a ModelRequest,
            _cancellation: CancellationSignal<'a>,
        ) -> Self::Stream<'a> {
            let index =
                self.next.get().min(self.turns.len().saturating_sub(1));
            self.next.set(index + 1);
            self.turns[index].clone().into_iter()
        }
    }

    fn completed() -> ProviderEvent {
        ProviderEvent::Event(ModelEvent::Completed)
    }

    fn proposed_call(
        call_id: &str,
        tool_name: &str,
        input: serde_json::Value,
    ) -> ProviderEvent {
        ProviderEvent::Event(ModelEvent::ToolCall {
            call_id: call_id.to_owned(),
            tool_name: tool_name.to_owned(),
            input: ToolCallInput::from_value(input),
        })
    }

    fn write_capability() -> siralos_core::tool::CapabilityId {
        siralos_core::tool::CapabilityId::parse("workspace.write")
            .expect("workspace.write is a valid capability id")
    }

    fn write_policy(rule: PermissionRule) -> PermissionPolicy {
        PermissionPolicy::from_rules([PolicyRule {
            capability: write_capability(),
            rule,
        }])
    }

    fn drive<P: ModelProvider>(
        application: &mut SiralosApplication<'_, P>,
    ) -> Vec<ToolLoopEvent> {
        let mut events = Vec::new();
        while let Some(event) = application.poll_event() {
            events.push(event);
        }
        events
    }

    fn tool_result<'a>(
        history: &'a [ConversationItem],
        call_id: &str,
    ) -> &'a ToolExecutionResult {
        history
            .iter()
            .find_map(|item| match item {
                ConversationItem::ToolResult {
                    call_id: item_call_id,
                    result,
                    ..
                } if item_call_id == call_id => Some(result),
                _ => None,
            })
            .expect("tool result must be paired in history")
    }

    fn approval_for(
        tool_name: &str,
        input: &serde_json::Value,
    ) -> ActionApproval {
        let digest =
            action_digest(tool_name, &canonical_input_json(input), None);
        ActionApproval::new(tool_name, digest, None)
            .expect("computed digest is valid")
    }

    const NO_APPROVAL_MESSAGE: &str = "Capability workspace.write requires a one-time host approval for this exact action; no matching approval was recorded, so the call was denied without execution.";

    #[test]
    fn executor_refuses_write_without_approval() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let input = json!({"path": "gated.txt", "content": "x"});
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "write", input), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Ask),
            None,
            None,
        );
        app.send_prompt("create it".to_owned()).unwrap();
        drive(&mut app);
        assert!(
            matches!(
                tool_result(app.history(), "c1"),
                ToolExecutionResult::Denied { message }
                    if message == NO_APPROVAL_MESSAGE
            ),
            "write without approval must be denied without execution"
        );
        assert!(!fixture.root.join("gated.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn executor_refuses_edit_without_approval() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("a.txt"), b"keep me").unwrap();
        let tool = fixture.edit_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let input = json!({
            "path": "a.txt",
            "oldString": "keep",
            "newString": "drop"
        });
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "edit", input), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Ask),
            None,
            None,
        );
        app.send_prompt("edit it".to_owned()).unwrap();
        drive(&mut app);
        assert!(
            matches!(
                tool_result(app.history(), "c1"),
                ToolExecutionResult::Denied { message }
                    if message == NO_APPROVAL_MESSAGE
            ),
            "edit without approval must be denied without execution"
        );
        assert_eq!(fs::read(fixture.root.join("a.txt")).unwrap(), b"keep me");
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn executor_refusal_holds_under_allow() {
        // The approval gate is independent of the permission rule
        // (D4): even `allow` never executes a requires_approval tool
        // without a recorded one-time approval.
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let input = json!({"path": "allowed.txt", "content": "x"});
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "write", input), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Allow),
            None,
            None,
        );
        app.send_prompt("create it".to_owned()).unwrap();
        drive(&mut app);
        assert!(
            matches!(
                tool_result(app.history(), "c1"),
                ToolExecutionResult::Denied { message }
                    if message == NO_APPROVAL_MESSAGE
            ),
            "allow must not bypass the per-action approval gate"
        );
        assert!(!fixture.root.join("allowed.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn executor_runs_write_with_matching_approval() {
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let input = json!({"path": "made.txt", "content": "exact"});
        let approvals =
            siralos_core::tool::ActionApprovals::new().into_shared();
        approvals.approve(approval_for("write", &input));
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "write", input), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Ask),
            None,
            None,
        )
        .with_action_approvals(approvals);
        app.send_prompt("create it".to_owned()).unwrap();
        drive(&mut app);
        let output = match tool_result(app.history(), "c1") {
            ToolExecutionResult::Success { output, .. } => output.clone(),
            other => panic!("approved write must succeed: {other:?}"),
        };
        assert_eq!(fs::read(fixture.root.join("made.txt")).unwrap(), b"exact");
        let id = output["checkpoint"].as_str().expect("checkpoint id");
        let store =
            open_checkpoint_store(&fixture.root, &fixture.store_root).unwrap();
        let record = store.get(id).expect("checkpoint stored");
        assert_eq!(
            record.state,
            siralos_core::workspace::checkpoint::CheckpointState::Applied
        );
    }

    #[test]
    fn executor_runs_edit_with_matching_approval() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("b.txt"), b"hello world").unwrap();
        let tool = fixture.edit_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let input = json!({
            "path": "b.txt",
            "oldString": "world",
            "newString": "there"
        });
        let approvals =
            siralos_core::tool::ActionApprovals::new().into_shared();
        approvals.approve(approval_for("edit", &input));
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "edit", input), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Ask),
            None,
            None,
        )
        .with_action_approvals(approvals);
        app.send_prompt("edit it".to_owned()).unwrap();
        drive(&mut app);
        assert!(
            matches!(
                tool_result(app.history(), "c1"),
                ToolExecutionResult::Success { .. }
            ),
            "approved edit must succeed"
        );
        assert_eq!(
            fs::read(fixture.root.join("b.txt")).unwrap(),
            b"hello there"
        );
    }

    #[test]
    fn executor_refuses_write_on_digest_mismatch() {
        // Approval binds the exact bytes (D4): approving one content
        // and dispatching another must refuse before any effect.
        let fixture = Fixture::new();
        let tool = fixture.write_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let approved = json!({"path": "swapped.txt", "content": "approved"});
        let dispatched = json!({"path": "swapped.txt", "content": "mutated"});
        let approvals =
            siralos_core::tool::ActionApprovals::new().into_shared();
        approvals.approve(approval_for("write", &approved));
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "write", dispatched), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Ask),
            None,
            None,
        )
        .with_action_approvals(approvals);
        app.send_prompt("create it".to_owned()).unwrap();
        drive(&mut app);
        assert!(
            matches!(
                tool_result(app.history(), "c1"),
                ToolExecutionResult::Denied { message }
                    if message == NO_APPROVAL_MESSAGE
            ),
            "mutated bytes must not match the recorded approval"
        );
        assert!(!fixture.root.join("swapped.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn executor_refuses_edit_on_string_mismatch() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("c.txt"), b"alpha omega").unwrap();
        let tool = fixture.edit_tool();
        let registry = ToolRegistry::new(vec![
            Box::new(tool) as Box<dyn siralos_core::tool::Tool>
        ])
        .unwrap();
        let approved = json!({
            "path": "c.txt",
            "oldString": "omega",
            "newString": "alpha"
        });
        let dispatched = json!({
            "path": "c.txt",
            "oldString": "omega",
            "newString": "beta"
        });
        let approvals =
            siralos_core::tool::ActionApprovals::new().into_shared();
        approvals.approve(approval_for("edit", &approved));
        let provider = ScriptProvider::new(vec![
            vec![proposed_call("c1", "edit", dispatched), completed()],
            vec![completed()],
        ]);
        let mut app = SiralosApplication::new(
            &provider,
            &registry,
            write_policy(PermissionRule::Ask),
            None,
            None,
        )
        .with_action_approvals(approvals);
        app.send_prompt("edit it".to_owned()).unwrap();
        drive(&mut app);
        assert!(
            matches!(
                tool_result(app.history(), "c1"),
                ToolExecutionResult::Denied { message }
                    if message == NO_APPROVAL_MESSAGE
            ),
            "mutated strings must not match the recorded approval"
        );
        assert_eq!(
            fs::read(fixture.root.join("c.txt")).unwrap(),
            b"alpha omega"
        );
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn drive_and_unc_paths_are_denied() {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("v.txt"), b"stay").unwrap();
        let write = fixture.write_tool();
        let edit = fixture.edit_tool();
        let token = CancellationToken::new();
        for path in [
            "C:\\evil.txt",
            "C:evil.txt",
            "C:/evil.txt",
            "\\\\server\\share\\x.txt",
            "\\\\?\\C:\\x.txt",
            "/absolute.txt",
        ] {
            let write_result = write.execute(
                &json!({"path": path, "content": "x"}),
                token.signal(),
            );
            assert!(
                matches!(
                    &write_result,
                    ToolExecutionResult::Denied { message }
                        if message.as_str()
                            == "Path must be relative to the workspace."
                ),
                "{path}: {write_result:?}"
            );
            let edit_result = edit.execute(
                &json!({
                    "path": path,
                    "oldString": "a",
                    "newString": "b"
                }),
                token.signal(),
            );
            assert!(
                matches!(
                    &edit_result,
                    ToolExecutionResult::Denied { message }
                        if message.as_str()
                            == "Path must be relative to the workspace."
                ),
                "{path}: {edit_result:?}"
            );
        }
        assert_eq!(fs::read(fixture.root.join("v.txt")).unwrap(), b"stay");
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[test]
    fn revalidate_parent_refuses_moved_or_escaped_parents() {
        use super::revalidate_parent;
        let fixture = Fixture::new();
        // The backend root is always canonical; canonicalize here
        // for the same reason (verbatim paths never prefix-match).
        let root = std::fs::canonicalize(&fixture.root).unwrap();
        let sub = fixture.root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        assert!(revalidate_parent(&root, &sub).is_ok());
        let gone = fixture.root.join("gone");
        assert!(revalidate_parent(&root, &gone).is_err());
        let outside = std::env::temp_dir().join(format!(
            "siralos-mut-out-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        assert!(revalidate_parent(&root, &outside).is_err());
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn requested_name_gate_accepts_a_stable_resolution() {
        use super::require_requested_parent_name;
        let fixture = Fixture::new();
        let root = std::fs::canonicalize(&fixture.root).unwrap();
        let hot = root.join("hot");
        fs::create_dir_all(&hot).unwrap();
        assert!(require_requested_parent_name(&root, "hot", &hot).is_ok());
        assert!(require_requested_parent_name(&root, ".", &root).is_ok());
        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();
        assert!(require_requested_parent_name(&root, "a/b", &nested).is_ok());
        // Lexical normalization matches the resolver's own: a
        // dot-dot request that resolves inside still names itself
        // (`a/../b` is `b`, the same normalization the join uses).
        let bare = root.join("b");
        fs::create_dir_all(&bare).unwrap();
        assert!(require_requested_parent_name(&root, "a/../b", &bare).is_ok());
    }

    #[test]
    fn requested_name_gate_refuses_a_renamed_parent() {
        use super::require_requested_parent_name;
        let fixture = Fixture::new();
        let root = std::fs::canonicalize(&fixture.root).unwrap();
        let parked = root.join("hot-parked");
        fs::create_dir_all(&parked).unwrap();
        // The exact rename-inside-canonicalize shape: the caller
        // asked for `hot`, resolution returned the same object
        // under its new name.
        let refused = require_requested_parent_name(&root, "hot", &parked);
        assert!(
            matches!(
                refused,
                Err(crate::atomic::AtomicWriteFailure::ParentChanged { .. })
            ),
            "{refused:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_refuses_a_symlinked_parent_before_any_checkpoint() {
        let fixture = Fixture::new();
        let real = fixture.root.join("real");
        fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, fixture.root.join("link")).unwrap();
        let write = fixture.write_tool();
        let token = CancellationToken::new();
        // Resolution lands inside the workspace (the link target is
        // real), but under a name the caller never requested: the
        // approval is bound to `link/...`, so the write must refuse
        // rather than create `real/...`.
        let refused = write.execute(
            &json!({"path": "link/t.txt", "content": "payload"}),
            token.signal(),
        );
        assert!(
            matches!(
                refused,
                ToolExecutionResult::Conflict { message }
                    if message
                        == "The target directory changed while it was being written."
            ),
            "{refused:?}"
        );
        assert!(!real.join("t.txt").exists());
        assert_eq!(fixture.checkpoint_count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn edit_refuses_a_symlinked_parent_without_touching_bytes() {
        let fixture = Fixture::new();
        let real = fixture.root.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("f.txt"), b"needle in hay").unwrap();
        std::os::unix::fs::symlink(&real, fixture.root.join("link")).unwrap();
        let edit = fixture.edit_tool();
        let token = CancellationToken::new();
        let refused = edit.execute(
            &json!({
                "path": "link/f.txt",
                "oldString": "needle",
                "newString": "thread"
            }),
            token.signal(),
        );
        assert!(
            matches!(
                refused,
                ToolExecutionResult::Conflict { message }
                    if message
                        == "The target directory changed while it was being written."
            ),
            "{refused:?}"
        );
        assert_eq!(fs::read(real.join("f.txt")).unwrap(), b"needle in hay");
    }

    #[cfg(unix)]
    #[test]
    fn verify_committed_detects_a_swapped_parent() {
        use super::verify_committed;
        let fixture = Fixture::new();
        let sub = fixture.root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("f.txt"), b"data").unwrap();
        let absolute = sub.join("f.txt");
        assert!(verify_committed(&fixture.root, &absolute, b"data").is_ok());
        let outside = std::env::temp_dir().join(format!(
            "siralos-mut-out-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("f.txt"), b"data").unwrap();
        fs::remove_dir_all(&sub).unwrap();
        std::os::unix::fs::symlink(&outside, &sub).unwrap();
        // Same bytes through the swapped parent: the byte check
        // would pass, so the parent recheck must carry the verdict.
        let refused = verify_committed(&fixture.root, &absolute, b"data");
        assert!(
            matches!(
                refused,
                Err(ToolExecutionResult::Conflict { message })
                    if message
                        == "The target directory changed while it was being written."
            ),
            "{refused:?}"
        );
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn verify_committed_refuses_a_vanished_parent() {
        use super::verify_committed;
        let fixture = Fixture::new();
        let sub = fixture.root.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("f.txt"), b"data").unwrap();
        let absolute = sub.join("f.txt");
        fs::remove_dir_all(&sub).unwrap();
        assert!(verify_committed(&fixture.root, &absolute, b"data").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn fifo_files_are_not_mutation_targets() {
        // A named pipe at the target: create-only sees an existing
        // entry (conflict, never opened), and edit refuses the
        // non-regular target without reading it. FIFOs have no
        // safe std constructor (`libc::mkfifo` is `unsafe`, which
        // the workspace forbids), so the POSIX `mkfifo` helper
        // stages it; the refusal under test is ours, not its.
        let fixture = Fixture::new();
        let fifo = fixture.root.join("pipe");
        let staged = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo helper runs on unix test hosts");
        assert!(staged.success(), "mkfifo must stage the fifo");
        let write = fixture.write_tool();
        let edit = fixture.edit_tool();
        let token = CancellationToken::new();
        assert!(matches!(
            write.execute(
                &json!({"path": "pipe", "content": "x"}),
                token.signal()
            ),
            ToolExecutionResult::Conflict { .. }
        ));
        assert!(matches!(
            edit.execute(
                &json!({
                    "path": "pipe",
                    "oldString": "a",
                    "newString": "b"
                }),
                token.signal()
            ),
            ToolExecutionResult::Failed { .. }
        ));
    }

    #[cfg(unix)]
    #[test]
    fn socket_files_are_not_mutation_targets() {
        let fixture = Fixture::new();
        let socket = fixture.root.join("sock");
        let _listener =
            std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let write = fixture.write_tool();
        let edit = fixture.edit_tool();
        let token = CancellationToken::new();
        assert!(matches!(
            write.execute(
                &json!({"path": "sock", "content": "x"}),
                token.signal()
            ),
            ToolExecutionResult::Conflict { .. }
        ));
        assert!(matches!(
            edit.execute(
                &json!({
                    "path": "sock",
                    "oldString": "a",
                    "newString": "b"
                }),
                token.signal()
            ),
            ToolExecutionResult::Failed { .. }
        ));
    }

    #[test]
    fn rename_substitution_race_never_misdirects() {
        // The substitution window is the explicit subject here: one
        // thread renames the hot parent directory away and back while
        // the main thread drives hundreds of write/edit attempts.
        // Scheduling decides which outcomes occur, so the test
        // asserts only schedule-independent properties: nothing is
        // ever written outside the workspace, every success carries
        // the exact approved bytes at the intended path, and every
        // other outcome is a typed refusal.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let fixture = Fixture::new();
        let hot = fixture.root.join("hot");
        fs::create_dir_all(&hot).unwrap();
        fs::write(hot.join("e.txt"), b"needle in hay").unwrap();
        let outside = std::env::temp_dir().join(format!(
            "siralos-mut-out-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("canary.txt"), b"untouched").unwrap();
        let parked = fixture.root.join("hot-parked");

        let stop = Arc::new(AtomicBool::new(false));
        let swapper_stop = Arc::clone(&stop);
        let (hot_thread, parked_thread) = (hot.clone(), parked.clone());
        let swapper = std::thread::spawn(move || {
            for _ in 0..2000 {
                if swapper_stop.load(Ordering::Relaxed) {
                    break;
                }
                // Rename the parent away and back; failures (a
                // concurrent operation holding the directory) only
                // shorten the race, they never invalidate it.
                let _ = fs::rename(&hot_thread, &parked_thread);
                let _ = fs::rename(&parked_thread, &hot_thread);
                // Rewrite the edit target so compare-and-swap races
                // engage as well.
                if let Ok(guard) = fs::read(hot_thread.join("e.txt")) {
                    let _ = fs::write(
                        hot_thread.join("e.txt"),
                        if guard == b"needle in hay" {
                            b"moved hay".to_vec()
                        } else {
                            b"needle in hay".to_vec()
                        },
                    );
                }
            }
        });

        let write = fixture.write_tool();
        let edit = fixture.edit_tool();
        let token = CancellationToken::new();
        let mut refusals = 0u32;
        let mut created = Vec::new();
        for index in 0..200u32 {
            let name = format!("hot/t-{index}.txt");
            match write.execute(
                &json!({"path": name, "content": "payload"}),
                token.signal(),
            ) {
                ToolExecutionResult::Success { output, .. } => {
                    assert_eq!(output["path"], name);
                    created.push(name);
                }
                ToolExecutionResult::Denied { .. }
                | ToolExecutionResult::Conflict { .. }
                | ToolExecutionResult::Failed { .. } => {
                    refusals += 1;
                }
                other => panic!("untyped outcome: {other:?}"),
            }
            match edit.execute(
                &json!({
                    "path": "hot/e.txt",
                    "oldString": "needle",
                    "newString": "thread"
                }),
                token.signal(),
            ) {
                ToolExecutionResult::Success { .. } => {}
                ToolExecutionResult::Denied { .. }
                | ToolExecutionResult::Conflict { .. }
                | ToolExecutionResult::Failed { .. } => {
                    refusals += 1;
                }
                other => panic!("untyped outcome: {other:?}"),
            }
        }
        stop.store(true, Ordering::Relaxed);
        swapper.join().expect("swapper thread joins");
        // The swapper may have stopped with the parent parked away;
        // restore it before verifying in-workspace exactness.
        if !hot.exists() {
            fs::rename(&parked, &hot).expect("parent restores");
        }
        // Every reported success lands the exact bytes at the
        // intended in-workspace path: reads happen after the race,
        // so no transient rename can flake them.
        for name in &created {
            assert_eq!(
                fs::read(fixture.root.join(name)).unwrap(),
                b"payload",
                "{name}"
            );
        }
        // No file may land outside the requested set: walk the
        // workspace and require every created-pattern file to sit at
        // a path this test actually requested, carrying the exact
        // payload bytes. This is schedule-independent: it catches a
        // misdirected rename no matter what any single outcome
        // claimed. Files at requested paths with exact bytes but no
        // recorded Success are the documented verify-race aftermath
        // (rename succeeded correctly, a later swap broke post-commit
        // verification, the caller was honestly told Failed, the
        // checkpoint stayed prepared for reconciliation to heal):
        // coherent bytes, never misdirection, and tolerated here.
        // (The per-Success `output["path"]` assert above catches a
        // wrong-path claim at dispatch time.)
        let mut stray_payloads = Vec::new();
        let mut pending_dirs = vec![fixture.root.clone()];
        while let Some(directory) = pending_dirs.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    pending_dirs.push(path);
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with("t-") || !name.ends_with(".txt") {
                    continue;
                }
                let relative = path
                    .strip_prefix(&fixture.root)
                    .expect("workspace-relative")
                    .to_string_lossy()
                    .replace('\\', "/");
                let requested = relative
                    .strip_prefix("hot/t-")
                    .and_then(|tail| tail.strip_suffix(".txt"))
                    .and_then(|number| number.parse::<u32>().ok())
                    .is_some_and(|number| number < 200);
                let exact = fs::read(&path).unwrap_or_default() == b"payload";
                if !requested || !exact {
                    stray_payloads.push(relative);
                }
            }
        }
        assert!(
            stray_payloads.is_empty(),
            "misdirected or corrupt writes: {stray_payloads:?}"
        );
        let edit_bytes = fs::read(fixture.root.join("hot/e.txt")).unwrap();
        assert!(
            edit_bytes.windows(6).any(|w| w == b"thread")
                || edit_bytes == b"moved hay"
                || edit_bytes == b"needle in hay",
            "edit target holds coherent bytes"
        );
        // The race must have engaged at least once: hundreds of
        // multi-step mutations against two thousand renames.
        assert!(refusals > 0, "expected the race to refuse at least once");
        assert_eq!(
            fs::read(outside.join("canary.txt")).unwrap(),
            b"untouched"
        );
        let mut strays = Vec::new();
        for entry in fs::read_dir(&outside).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() != "canary.txt" {
                strays.push(entry.path());
            }
        }
        assert!(strays.is_empty(), "outside writes: {strays:?}");
        let _ = fs::remove_dir_all(&outside);
    }
}
