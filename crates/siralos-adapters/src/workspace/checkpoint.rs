//! Checkpoint storage inspection and reconciliation (R4, ADR 0006).
//!
//! Siralos-owned recovery records stored outside the workspace at
//! `~/.siralos/checkpoints/<workspace-fingerprint>/<checkpoint-id>/`
//! with the exact `metadata.json` + `preimage.bin` layout. R4 ports the
//! inspection surface (parse, validate, get, list, deterministic state
//! transitions, reconciliation classification and reporting); new
//! checkpoint creation, retention capacity, and undo remain unavailable
//! exactly as the reference reports them, and checkpoints stay fully
//! independent of Git. Corrupt or invalid metadata fails closed: it is
//! never returned as valid, never repaired, and never deleted.
//!
//! The workspace fingerprint binding (SHA-256 of the canonical
//! workspace path) is a machine identity, so differential records
//! report fingerprint validity rather than the fingerprint itself.

use crate::workspace::fs::read_file_bounded;

use siralos_core::identity::sha256_hex;
use siralos_core::workspace::checkpoint::{
    CheckpointFileState, CheckpointOperation, CheckpointPreview,
    CheckpointState, CheckpointTerminalState, FileCheckpoint,
    ReconciliationClass, WorkspaceFileState, classify_reconciliation,
    validate_checkpoint_invariant,
};

use std::path::{Path, PathBuf};

/// Stored record schema version.
pub const CHECKPOINT_VERSION: u64 = 1;
/// Maximum serialized metadata bytes.
pub const MAX_METADATA_BYTES: u64 = 64 * 1024;
/// Maximum preimage bytes (before-state byte length bound).
pub const MAX_PREIMAGE_BYTES: u64 = 1024 * 1024;
/// Maximum tool-name length.
pub const MAX_TOOL_NAME_LENGTH: usize = 256;
/// Maximum createdAt length.
pub const MAX_CREATED_AT_LENGTH: usize = 64;
/// Known per-checkpoint layout: metadata.json and preimage.bin only.
pub const CHECKPOINT_DIR_MAX_ENTRIES: usize = 3;
/// Default checkpoint count bound for bounded enumeration.
pub const DEFAULT_MAX_CHECKPOINTS: usize = 100;

/// Checkpoint id pattern (`cp_<hex-or-dash>`, at least 10 tail chars).
pub fn is_valid_checkpoint_id(id: &str) -> bool {
    id.len() > 3
        && id.starts_with("cp_")
        && id[3..]
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
        && id[3..].chars().count() >= 10
}

/// Why a checkpoint storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointStoreError {
    /// The checkpoint root cannot be established.
    RootUnavailable(String),
    /// The checkpoint root is a symbolic link.
    RootIsLink,
    /// The checkpoint root resolves inside the active workspace.
    RootInsideWorkspace,
    /// A checkpoint or its metadata is unknown or invalid.
    Invalid(String),
    /// The checkpoint id is not in the canonical form.
    InvalidId(String),
    /// The metadata transition is not allowed.
    InvalidTransition {
        /// The checkpoint id.
        id: String,
        /// The state the checkpoint is in.
        from: CheckpointState,
        /// The state that was requested.
        to: CheckpointState,
    },
    /// Metadata could not be written.
    WriteFailed(String),
}

impl std::fmt::Display for CheckpointStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::RootUnavailable(detail) => write!(formatter, "{detail}"),
            Self::RootIsLink => formatter.write_str("The checkpoint root must not be a symbolic link."),
            Self::RootInsideWorkspace => {
                formatter.write_str("The checkpoint store must not resolve inside the active workspace.")
            }
            Self::Invalid(detail) => formatter.write_str(detail),
            Self::InvalidId(id) => write!(formatter, "Invalid checkpoint id: {id}."),
            Self::InvalidTransition { id, from, to } => {
                write!(
                    formatter,
                    "Checkpoint {id} is in state {}, which cannot transition to {}.",
                    from.as_str(),
                    to.as_str(),
                )
            }
            Self::WriteFailed(detail) => formatter.write_str(detail),
        }
    }
}

impl std::error::Error for CheckpointStoreError {}

/// A bounded checkpoint inspection store over the reference storage
/// layout. One clear owner per workspace checkpoint root.
#[derive(Debug, Clone)]
pub struct CheckpointStore {
    workspace_root: PathBuf,
    checkpoint_root: PathBuf,
    workspace_fingerprint: String,
    max_preimage_bytes: u64,
    max_checkpoints: usize,
}

/// Open (or create) a checkpoint store outside the workspace. The root
/// must not be a link and must not resolve inside the workspace; the
/// fingerprint is the SHA-256 of the canonical workspace path bytes.
pub fn open_checkpoint_store(
    workspace_root: &Path,
    checkpoint_root: &Path,
) -> Result<CheckpointStore, CheckpointStoreError> {
    let canonical_workspace =
        std::fs::canonicalize(workspace_root).map_err(|error| {
            CheckpointStoreError::RootUnavailable(format!(
                "Checkpoint store paths cannot be resolved: {error}"
            ))
        })?;
    if !checkpoint_root.exists() {
        std::fs::create_dir_all(checkpoint_root).map_err(|error| {
            CheckpointStoreError::RootUnavailable(format!(
                "Checkpoint root cannot be created: {error}"
            ))
        })?;
    }
    let canonical_root =
        std::fs::canonicalize(checkpoint_root).map_err(|error| {
            CheckpointStoreError::RootUnavailable(format!(
                "Checkpoint root cannot be resolved: {error}"
            ))
        })?;
    let root_metadata =
        std::fs::symlink_metadata(&canonical_root).map_err(|error| {
            CheckpointStoreError::RootUnavailable(format!(
                "Checkpoint root cannot be inspected: {error}"
            ))
        })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(CheckpointStoreError::RootIsLink);
    }
    if canonical_root == canonical_workspace
        || is_inside(&canonical_workspace, &canonical_root)
    {
        return Err(CheckpointStoreError::RootInsideWorkspace);
    }
    let workspace_fingerprint = fingerprint_of(&canonical_workspace);
    Ok(CheckpointStore {
        workspace_root: canonical_workspace,
        checkpoint_root: canonical_root,
        workspace_fingerprint,
        max_preimage_bytes: MAX_PREIMAGE_BYTES,
        max_checkpoints: DEFAULT_MAX_CHECKPOINTS,
    })
}

/// SHA-256 of the canonical workspace path bytes (reference binding).
pub fn fingerprint_of(canonical_workspace: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        sha256_hex(canonical_workspace.as_os_str().as_bytes())
    }
    #[cfg(windows)]
    {
        sha256_hex(
            canonical_workspace.as_os_str().to_string_lossy().as_bytes(),
        )
    }
}

fn is_inside(root: &Path, target: &Path) -> bool {
    target.starts_with(root)
}
/// One validated metadata document plus its exact byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedMetadata {
    /// The validated checkpoint record.
    pub checkpoint: FileCheckpoint,
    /// Exact UTF-8 byte length of the metadata document.
    pub metadata_bytes: u64,
}

/// Parse and fully validate one stored metadata document (exact key
/// set, version, id binding, workspace fingerprint, relative path,
/// enums, operation-state invariant, size and pattern bounds).
pub fn parse_metadata(
    raw: &str,
    id: &str,
    workspace_fingerprint: &str,
    max_preimage_bytes: u64,
) -> Result<LoadedMetadata, CheckpointStoreError> {
    if raw.len() as u64 > MAX_METADATA_BYTES {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata is oversized: {id}."
        )));
    }
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint metadata is not valid JSON: {id}."
            ))
        })?;
    let object = value.as_object().ok_or_else(|| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata is malformed: {id}."
        ))
    })?;
    const KEYS: [&str; 11] = [
        "version",
        "id",
        "workspaceFingerprint",
        "relativePath",
        "operation",
        "toolName",
        "createdAt",
        "state",
        "before",
        "after",
        "preview",
    ];
    if object.len() != KEYS.len()
        || KEYS.iter().any(|key| !object.contains_key(*key))
    {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata carries unexpected fields: {id}."
        )));
    }
    let version = required_u64(object, "version")?;
    if version != CHECKPOINT_VERSION {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata version or id mismatch: {id}."
        )));
    }
    let record_id = required_string(object, "id")?;
    if record_id != id {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata version or id mismatch: {id}."
        )));
    }
    let record_fingerprint = required_string(object, "workspaceFingerprint")?;
    if record_fingerprint != workspace_fingerprint {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint belongs to a different workspace: {id}."
        )));
    }
    let relative_path = required_string(object, "relativePath")?;
    if siralos_core::workspace::path::validate_relative_path(&relative_path)
        .is_err()
    {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint relative path is invalid: {id}."
        )));
    }
    let state = CheckpointState::parse(&required_string(object, "state")?)
        .ok_or_else(|| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint state is invalid: {id}."
            ))
        })?;
    let tool_name = required_string(object, "toolName")?;
    if tool_name.is_empty() || tool_name.len() > MAX_TOOL_NAME_LENGTH {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint tool name is invalid: {id}."
        )));
    }
    let created_at = required_string(object, "createdAt")?;
    if created_at.is_empty() || created_at.len() > MAX_CREATED_AT_LENGTH {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint creation time is invalid: {id}."
        )));
    }
    let operation_value = required_string(object, "operation")?;
    let operation =
        CheckpointOperation::parse(&operation_value).ok_or_else(|| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint operation is invalid: {id}."
            ))
        })?;
    let before = parse_file_state(
        object.get("before"),
        "before",
        id,
        Some(max_preimage_bytes),
    )?;
    let after = parse_file_state(object.get("after"), "after", id, None)?;
    validate_checkpoint_invariant(operation, before.exists, after.exists).map_err(|_| {
        let expected = siralos_core::workspace::checkpoint::OperationState::for_operation(operation);
        CheckpointStoreError::Invalid(format!("Checkpoint operation \"{operation_value}\" requires the before/after existence transition {}->{}, but the record declares {}->{}", expected.before_exists, expected.after_exists, before.exists, after.exists))
    })?;
    let preview_value = object
        .get("preview")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint preview is malformed: {id}."
            ))
        })?;
    const PREVIEW_KEYS: [&str; 2] = ["addedLines", "removedLines"];
    if preview_value.len() != PREVIEW_KEYS.len()
        || PREVIEW_KEYS.iter().any(|key| !preview_value.contains_key(*key))
    {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint preview carries unexpected fields: {id}."
        )));
    }
    let added_lines = required_u64(preview_value, "addedLines")?;
    let removed_lines = required_u64(preview_value, "removedLines")?;
    let checkpoint = FileCheckpoint {
        version: CHECKPOINT_VERSION,
        id: record_id,
        workspace_fingerprint: record_fingerprint,
        relative_path,
        operation,
        tool_name,
        created_at,
        state,
        before,
        after,
        preview: CheckpointPreview { added_lines, removed_lines },
    };
    Ok(LoadedMetadata { checkpoint, metadata_bytes: raw.len() as u64 })
}

fn required_string(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, CheckpointStoreError> {
    object
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint metadata is malformed ({key})."
            ))
        })
}

fn required_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<u64, CheckpointStoreError> {
    object.get(key).and_then(serde_json::Value::as_u64).ok_or_else(|| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata is malformed ({key})."
        ))
    })
}

fn parse_file_state(
    value: Option<&serde_json::Value>,
    label: &str,
    id: &str,
    max_preimage_bytes: Option<u64>,
) -> Result<CheckpointFileState, CheckpointStoreError> {
    let object =
        value.and_then(serde_json::Value::as_object).ok_or_else(|| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint {label} is malformed: {id}."
            ))
        })?;
    const KEYS: [&str; 3] = ["exists", "sha256", "byteLength"];
    if object.len() != KEYS.len()
        || KEYS.iter().any(|key| !object.contains_key(*key))
    {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint {label} carries unexpected fields: {id}."
        )));
    }
    let exists = required_bool(object, "exists")?;
    if !exists {
        let sha256 = object.get("sha256").and_then(serde_json::Value::as_str);
        let byte_length =
            object.get("byteLength").and_then(serde_json::Value::as_u64);
        if sha256.is_some() || byte_length.is_some() {
            return Err(CheckpointStoreError::Invalid(format!(
                "Checkpoint {label} carries a hash or byte length for a state that does not exist: {id}."
            )));
        }
        return Ok(CheckpointFileState {
            exists: false,
            sha256: None,
            byte_length: None,
        });
    }
    let sha256 = required_string(object, "sha256")?;
    if !is_sha256_hex(&sha256) {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint {label} hash is invalid: {id}."
        )));
    }
    let byte_length = required_u64(object, "byteLength")?;
    if let Some(maximum) = max_preimage_bytes {
        if byte_length > maximum {
            return Err(CheckpointStoreError::Invalid(format!(
                "Checkpoint {label} byte length exceeds the configured maximum: {id}."
            )));
        }
    }
    Ok(CheckpointFileState {
        exists: true,
        sha256: Some(sha256),
        byte_length: Some(byte_length),
    })
}

fn required_bool(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<bool, CheckpointStoreError> {
    object.get(key).and_then(serde_json::Value::as_bool).ok_or_else(|| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint metadata is malformed ({key})."
        ))
    })
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
impl CheckpointStore {
    /// The workspace fingerprint this store validates records against.
    pub fn workspace_fingerprint(&self) -> &str {
        &self.workspace_fingerprint
    }

    /// Directory of one checkpoint id (id-validated first).
    pub fn checkpoint_directory(
        &self,
        id: &str,
    ) -> Result<PathBuf, CheckpointStoreError> {
        if !is_valid_checkpoint_id(id) {
            return Err(CheckpointStoreError::InvalidId(id.to_owned()));
        }
        Ok(self.checkpoint_root.join(&self.workspace_fingerprint).join(id))
    }

    /// Load and fully validate one checkpoint's metadata.
    pub fn load_metadata(
        &self,
        id: &str,
    ) -> Result<LoadedMetadata, CheckpointStoreError> {
        let directory = self.checkpoint_directory(id)?;
        let metadata_path = directory.join("metadata.json");
        let metadata = match std::fs::symlink_metadata(&metadata_path) {
            Ok(metadata) => metadata,
            Err(_) => {
                return Err(CheckpointStoreError::Invalid(format!(
                    "Checkpoint metadata is missing or a symbolic link: {id}."
                )));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(CheckpointStoreError::Invalid(format!(
                "Checkpoint metadata is missing or a symbolic link: {id}."
            )));
        }
        if metadata.len() > MAX_METADATA_BYTES {
            return Err(CheckpointStoreError::Invalid(format!(
                "Checkpoint metadata is oversized: {id}."
            )));
        }
        let raw =
            std::fs::read_to_string(&metadata_path).map_err(|error| {
                CheckpointStoreError::Invalid(format!(
                    "Checkpoint metadata cannot be read: {id}: {error}"
                ))
            })?;
        parse_metadata(
            &raw,
            id,
            &self.workspace_fingerprint,
            self.max_preimage_bytes,
        )
    }

    /// Get one checkpoint, or `None` when unknown or invalid (the
    /// reference `get()` returns null for every load failure).
    pub fn get(&self, id: &str) -> Option<FileCheckpoint> {
        self.load_metadata(id).ok().map(|loaded| loaded.checkpoint)
    }

    /// List checkpoints with the reference semantics: directory names
    /// starting with `cp_` are loaded (invalid records skipped), states
    /// filtered, then sorted by `createdAt` descending; the enumeration
    /// is capped and the result bounded by `limit`.
    pub fn list(
        &self,
        states: Option<&[CheckpointState]>,
        limit: Option<usize>,
    ) -> Vec<FileCheckpoint> {
        let fingerprint_directory =
            self.checkpoint_root.join(&self.workspace_fingerprint);
        let mut names: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&fingerprint_directory) {
            for entry in entries.flatten() {
                if names.len() > self.max_checkpoints {
                    break;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("cp_") {
                    names.push(name);
                }
            }
        }
        let mut checkpoints: Vec<FileCheckpoint> = Vec::new();
        for name in names {
            if let Ok(loaded) = self.load_metadata(&name) {
                checkpoints.push(loaded.checkpoint);
            }
        }
        if let Some(states) = states {
            checkpoints
                .retain(|checkpoint| states.contains(&checkpoint.state));
        }
        checkpoints.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        if let Some(limit) = limit {
            checkpoints.truncate(limit);
        }
        checkpoints
    }

    /// Write one checkpoint's metadata atomically (temp file + rename,
    /// restrictive permissions where supported).
    pub fn write_metadata(
        &self,
        checkpoint: &FileCheckpoint,
    ) -> Result<(), CheckpointStoreError> {
        let serialized =
            serde_json::to_string_pretty(&serialize_checkpoint(checkpoint))
                .map_err(|error| {
                    CheckpointStoreError::WriteFailed(format!(
                        "Checkpoint metadata cannot be serialized: {error}"
                    ))
                })?;
        self.write_metadata_serialized(checkpoint, &serialized)
    }

    /// Write exact serialized metadata bytes atomically.
    pub fn write_metadata_serialized(
        &self,
        checkpoint: &FileCheckpoint,
        serialized: &str,
    ) -> Result<(), CheckpointStoreError> {
        let directory = self.checkpoint_directory(&checkpoint.id)?;
        if !directory.is_dir() {
            return Err(CheckpointStoreError::WriteFailed(format!(
                "Checkpoint directory is missing: {}.",
                checkpoint.id
            )));
        }
        let temporary_path =
            directory.join(format!("metadata.json.tmp-{}", checkpoint.id));
        std::fs::write(&temporary_path, serialized).map_err(|error| {
            CheckpointStoreError::WriteFailed(format!(
                "Checkpoint metadata cannot be staged: {error}"
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &temporary_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
        std::fs::rename(&temporary_path, directory.join("metadata.json"))
            .map_err(|error| {
                let _ = std::fs::remove_file(&temporary_path);
                CheckpointStoreError::WriteFailed(format!(
                    "Checkpoint metadata cannot be committed: {error}"
                ))
            })?;
        Ok(())
    }

    /// Transition one checkpoint's state, mirroring the reference
    /// transition table (prepared -> applied/abandoned/conflicted/
    /// uncertain; applied -> uncertain).
    pub fn mark_state(
        &self,
        id: &str,
        state: CheckpointTerminalState,
    ) -> Result<FileCheckpoint, CheckpointStoreError> {
        let loaded = self.load_metadata(id)?;
        let checkpoint = loaded.checkpoint;
        let target = match state {
            CheckpointTerminalState::Abandoned => CheckpointState::Abandoned,
            CheckpointTerminalState::Conflicted => CheckpointState::Conflicted,
            CheckpointTerminalState::Uncertain => CheckpointState::Uncertain,
            CheckpointTerminalState::Applied => CheckpointState::Applied,
        };
        let allowed: &[CheckpointState] = match checkpoint.state {
            CheckpointState::Prepared => &[
                CheckpointState::Applied,
                CheckpointState::Abandoned,
                CheckpointState::Conflicted,
                CheckpointState::Uncertain,
            ],
            CheckpointState::Applied => &[CheckpointState::Uncertain],
            _ => &[],
        };
        if !allowed.contains(&target) {
            return Err(CheckpointStoreError::InvalidTransition {
                id: id.to_owned(),
                from: checkpoint.state,
                to: target,
            });
        }
        let updated = FileCheckpoint { state: target, ..checkpoint };
        self.write_metadata(&updated)?;
        Ok(updated)
    }

    /// Mark a checkpoint undone (prepared or applied).
    pub fn mark_undone(
        &self,
        id: &str,
    ) -> Result<FileCheckpoint, CheckpointStoreError> {
        let loaded = self.load_metadata(id)?;
        let checkpoint = loaded.checkpoint;
        if checkpoint.state != CheckpointState::Prepared
            && checkpoint.state != CheckpointState::Applied
        {
            return Err(CheckpointStoreError::InvalidTransition {
                id: id.to_owned(),
                from: checkpoint.state,
                to: CheckpointState::Undone,
            });
        }
        let updated =
            FileCheckpoint { state: CheckpointState::Undone, ..checkpoint };
        self.write_metadata(&updated)?;
        Ok(updated)
    }
}

/// Reconciliation report counts (reference `ReconciliationReport`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconciliationReport {
    /// Checkpoints inspected and classified.
    pub checked: u64,
    /// Prepared checkpoints whose before-state still holds.
    pub abandoned: u64,
    /// Prepared checkpoints whose after-state now holds.
    pub applied: u64,
    /// Prepared checkpoints where neither state holds.
    pub uncertain: u64,
    /// Applied checkpoints whose file returned to the before-state.
    pub undone_after_restore: u64,
}

/// Startup reconciliation: classify pending checkpoints against the
/// exact current workspace file states and persist the deterministic
/// state transitions (abandoned/applied/uncertain/undone). Never
/// guesses, never mutates the workspace, and a failed transition leaves
/// the checkpoint in its previous state, exactly like the reference.
pub fn reconcile_checkpoints(
    store: &CheckpointStore,
    max_state_bytes: u64,
) -> ReconciliationReport {
    let mut report = ReconciliationReport::default();
    let pending = store.list(
        Some(&[CheckpointState::Prepared, CheckpointState::Applied]),
        None,
    );
    for checkpoint in pending {
        let current = read_workspace_file_state(
            &store.workspace_root,
            &checkpoint.relative_path,
            max_state_bytes,
        );
        match classify_reconciliation(&checkpoint, &current) {
            Some(ReconciliationClass::Abandoned) => {
                report.checked += 1;
                report.abandoned += 1;
                let _ = store.mark_state(
                    &checkpoint.id,
                    CheckpointTerminalState::Abandoned,
                );
            }
            Some(ReconciliationClass::Applied) => {
                report.checked += 1;
                report.applied += 1;
                let _ = store.mark_state(
                    &checkpoint.id,
                    CheckpointTerminalState::Applied,
                );
            }
            Some(ReconciliationClass::Uncertain) => {
                report.checked += 1;
                report.uncertain += 1;
                let _ = store.mark_state(
                    &checkpoint.id,
                    CheckpointTerminalState::Uncertain,
                );
            }
            Some(ReconciliationClass::UndoneAfterRestore) => {
                report.checked += 1;
                report.undone_after_restore += 1;
                let _ = store.mark_undone(&checkpoint.id);
            }
            None => {}
        }
    }
    report
}

/// Read the exact current workspace file state (exists + SHA-256),
/// mirroring `readWorkspaceFileState`: invalid, missing, linked,
/// non-regular, or oversized targets carry `sha256: null`.
pub fn read_workspace_file_state(
    workspace_root: &Path,
    relative_path: &str,
    max_state_bytes: u64,
) -> WorkspaceFileState {
    if siralos_core::workspace::path::validate_relative_path(relative_path)
        .is_err()
    {
        return WorkspaceFileState { exists: true, sha256: None };
    }
    let absolute = workspace_root.join(relative_path);
    let stats = match std::fs::symlink_metadata(&absolute) {
        Ok(stats) => stats,
        Err(_) => return WorkspaceFileState { exists: false, sha256: None },
    };
    if stats.file_type().is_symlink() || !stats.is_file() {
        return WorkspaceFileState { exists: true, sha256: None };
    }
    if stats.len() > max_state_bytes {
        return WorkspaceFileState { exists: true, sha256: None };
    }
    let bytes = match read_file_bounded(&absolute, max_state_bytes as usize) {
        Ok(Some(bytes)) => bytes,
        _ => return WorkspaceFileState { exists: true, sha256: None },
    };
    WorkspaceFileState { exists: true, sha256: Some(sha256_hex(&bytes)) }
}

/// Serialize one checkpoint to the exact stored metadata shape
/// (semantic equality with the reference record; key order is
/// canonicalized by the JSON value).
fn serialize_checkpoint(checkpoint: &FileCheckpoint) -> serde_json::Value {
    let file_state = |state: &CheckpointFileState| {
        serde_json::json!({
            "exists": state.exists,
            "sha256": state.sha256,
            "byteLength": state.byte_length,
        })
    };
    serde_json::json!({
        "version": checkpoint.version,
        "id": checkpoint.id,
        "workspaceFingerprint": checkpoint.workspace_fingerprint,
        "relativePath": checkpoint.relative_path,
        "operation": checkpoint.operation.as_str(),
        "toolName": checkpoint.tool_name,
        "createdAt": checkpoint.created_at,
        "state": checkpoint.state.as_str(),
        "before": file_state(&checkpoint.before),
        "after": file_state(&checkpoint.after),
        "preview": {
            "addedLines": checkpoint.preview.added_lines,
            "removedLines": checkpoint.preview.removed_lines,
        },
    })
}
#[cfg(test)]
mod tests {
    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
    use super::{
        ReconciliationReport, is_valid_checkpoint_id, open_checkpoint_store,
        parse_metadata, read_workspace_file_state, reconcile_checkpoints,
        serialize_checkpoint,
    };
    use siralos_core::workspace::checkpoint::{
        CheckpointFileState, CheckpointOperation, CheckpointPreview,
        CheckpointState, FileCheckpoint, WorkspaceFileState,
    };

    fn scratch(prefix: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    fn state(
        exists: bool,
        sha256: Option<&str>,
        byte_length: Option<u64>,
    ) -> CheckpointFileState {
        CheckpointFileState {
            exists,
            sha256: sha256.map(str::to_owned),
            byte_length,
        }
    }

    fn checkpoint(
        id: &str,
        fingerprint: &str,
        operation: CheckpointOperation,
        checkpoint_state: CheckpointState,
        created_at: &str,
        before: CheckpointFileState,
        after: CheckpointFileState,
    ) -> FileCheckpoint {
        FileCheckpoint {
            version: 1,
            id: id.to_owned(),
            workspace_fingerprint: fingerprint.to_owned(),
            relative_path: "a.txt".to_owned(),
            operation,
            tool_name: "workspace.edit_file".to_owned(),
            created_at: created_at.to_owned(),
            state: checkpoint_state,
            before,
            after,
            preview: CheckpointPreview { added_lines: 1, removed_lines: 1 },
        }
    }

    #[test]
    fn validates_ids_and_metadata_strictly() {
        assert!(is_valid_checkpoint_id("cp_0123456789abcdef"));
        assert!(!is_valid_checkpoint_id("cp_"));
        assert!(!is_valid_checkpoint_id("cp_abc"));
        assert!(!is_valid_checkpoint_id("other"));
        let raw = r#"{"version":1,"id":"cp_x","workspaceFingerprint":"ws","relativePath":"a.txt","operation":"create","toolName":"t","createdAt":"2024-01-01T00:00:00.000Z","state":"prepared","before":{"exists":false,"sha256":null,"byteLength":null},"after":{"exists":true,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","byteLength":4},"preview":{"addedLines":1,"removedLines":0}}"#;
        let loaded = parse_metadata(raw, "cp_x", "ws", 1024)
            .expect("valid metadata parses");
        assert_eq!(loaded.checkpoint.operation, CheckpointOperation::Create);
        // A create checkpoint with a before-state violates the invariant.
        let invalid = raw.replace(
            "\"before\":{\"exists\":false,\"sha256\":null,\"byteLength\":null}",
            "\"before\":{\"exists\":true,\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"byteLength\":4}",
        );
        assert!(parse_metadata(&invalid, "cp_x", "ws", 1024).is_err());
        // A wrong fingerprint fails closed.
        let foreign = raw.replace(
            "\"workspaceFingerprint\":\"ws\"",
            "\"workspaceFingerprint\":\"other\"",
        );
        assert!(parse_metadata(&foreign, "cp_x", "ws", 1024).is_err());
        // Unknown fields are rejected.
        let extra = raw.replace("\"preview\":", "\"extra\":1,\"preview\":");
        assert!(parse_metadata(&extra, "cp_x", "ws", 1024).is_err());
    }

    #[test]
    fn lists_and_gets_with_fail_closed_inspection() {
        let workspace = scratch("siralos-cp-ws");
        let store_root = scratch("siralos-cp-root");
        let store = open_checkpoint_store(&workspace, &store_root)
            .expect("store opens");
        let fingerprint = store.workspace_fingerprint().to_owned();
        let directory = store_root.join(&fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        let first = checkpoint(
            "cp_0000000001",
            &fingerprint,
            CheckpointOperation::Update,
            CheckpointState::Prepared,
            "2024-01-01T00:00:00.000Z",
            state(
                true,
                Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ),
                Some(4),
            ),
            state(
                true,
                Some(
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                ),
                Some(4),
            ),
        );
        let directory = directory.join("cp_0000000001");
        std::fs::create_dir_all(&directory).unwrap();
        let serialized =
            serde_json::to_string_pretty(&serialize_checkpoint(&first))
                .unwrap();
        std::fs::write(directory.join("metadata.json"), serialized).unwrap();
        assert!(store.get("cp_0000000001").is_some());
        assert!(store.get("cp_missing").is_none());
        let listed = store.list(None, None);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "cp_0000000001");
        let _ = std::fs::remove_dir_all(&workspace);
        let _ = std::fs::remove_dir_all(&store_root);
    }

    #[test]
    fn reconciliation_classifies_and_transitions_states() {
        let workspace = scratch("siralos-cp-ws");
        let store_root = scratch("siralos-cp-root");
        let store = open_checkpoint_store(&workspace, &store_root)
            .expect("store opens");
        let fingerprint = store.workspace_fingerprint().to_owned();
        let directory = store_root.join(&fingerprint);
        std::fs::create_dir_all(&directory).unwrap();
        // Workspace file matches the prepared checkpoint's before-state.
        std::fs::write(workspace.join("a.txt"), b"before bytes").unwrap();
        let before_sha = siralos_core::identity::sha256_hex(b"before bytes");
        let after_sha = siralos_core::identity::sha256_hex(b"after bytes");
        let prepared = checkpoint(
            "cp_0000000001",
            &fingerprint,
            CheckpointOperation::Update,
            CheckpointState::Prepared,
            "2024-01-01T00:00:00.000Z",
            state(true, Some(&before_sha), Some(12)),
            state(true, Some(&after_sha), Some(11)),
        );
        let dir = directory.join("cp_0000000001");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("metadata.json"),
            serde_json::to_string_pretty(&serialize_checkpoint(&prepared))
                .unwrap(),
        )
        .unwrap();
        let report = reconcile_checkpoints(&store, 1024 * 1024);
        assert_eq!(
            report,
            ReconciliationReport {
                checked: 1,
                abandoned: 1,
                ..Default::default()
            },
        );
        let after = store.get("cp_0000000001").expect("checkpoint exists");
        assert_eq!(after.state, CheckpointState::Abandoned);
        let _ = std::fs::remove_dir_all(&workspace);
        let _ = std::fs::remove_dir_all(&store_root);
    }

    #[test]
    fn workspace_file_state_reads_are_exact() {
        let workspace = scratch("siralos-cp-ws");
        std::fs::write(workspace.join("a.txt"), b"hello").unwrap();
        let sha = siralos_core::identity::sha256_hex(b"hello");
        assert_eq!(
            read_workspace_file_state(&workspace, "a.txt", 1024),
            WorkspaceFileState { exists: true, sha256: Some(sha) },
        );
        assert_eq!(
            read_workspace_file_state(&workspace, "missing.txt", 1024),
            WorkspaceFileState { exists: false, sha256: None },
        );
        let _ = std::fs::remove_dir_all(&workspace);
    }
}
