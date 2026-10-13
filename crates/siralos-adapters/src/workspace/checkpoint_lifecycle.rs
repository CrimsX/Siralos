//! Bounded checkpoint creation, capacity, pruning, and undo
//! execution (ADR 0038 §4).
//!
//! Inspection stays in [`super::checkpoint`]; this module owns the
//! lifecycle that makes "safe undo" real: [`create_checkpoint`]
//! records the before-state (with the exact before-bytes as
//! `preimage.bin`) before a mutation, [`checkpoint_capacity`]
//! accounts exact metadata plus preimage bytes, [`prune_checkpoints`]
//! automatically evicts evictable records oldest-first, and
//! [`apply_undo`] restores the before-state through the pure
//! [`plan_undo`] decision.
//!
//! Fail-closed rules (never weakened):
//!
//! - Unverifiable or unexpected store content blocks capacity checks
//!   and creation; it is never repaired or deleted automatically.
//! - `prepared` records (pending intent) are never auto-evicted; when
//!   only they exceed the bounds, creation refuses.
//! - Undoing a file creation would delete the file, so it stays typed
//!   unavailable (ADR 0038 D5: no deletion); anything else that is
//!   not exactly restorable is a conflict, never an overwrite.
//!
//! [`plan_undo`]: siralos_core::workspace::checkpoint::plan_undo

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use siralos_core::identity::sha256_hex;
use siralos_core::workspace::checkpoint::{
    CheckpointFileState, CheckpointOperation, CheckpointPreview,
    CheckpointState, FileCheckpoint, UndoPlanDecision, plan_undo,
    validate_checkpoint_invariant,
};
use siralos_core::workspace::path::validate_relative_path;

use super::checkpoint::{
    CHECKPOINT_VERSION, CheckpointStore, CheckpointStoreError,
    MAX_CREATED_AT_LENGTH, MAX_TOOL_NAME_LENGTH, is_valid_checkpoint_id,
    serialize_checkpoint,
};
use super::fs::{BoundedFileRead, read_complete_file_bounded};
use crate::atomic::stage_atomic;
use crate::workspace::resolve::{PathRejection, resolve_workspace_path};
/// Maximum attempts to mint a fresh checkpoint id.
const MAX_ID_ATTEMPTS: u32 = 8;
/// File mode for staged checkpoint bytes where supported.
const CHECKPOINT_FILE_MODE: u32 = 0o600;

/// One host-requested checkpoint record (before a mutation).
#[derive(Debug, Clone)]
pub struct CheckpointCreation {
    /// Workspace-relative target path.
    pub relative_path: String,
    /// The mutation operation (create/update; never delete: the
    /// mutation tools cannot delete, so no delete record is born).
    pub operation: CheckpointOperation,
    /// Tool that records the checkpoint (`write` / `edit`).
    pub tool_name: String,
    /// Host-recorded creation time (fixtures control it).
    pub created_at: String,
    /// Exact before-state of the target.
    pub before: CheckpointFileState,
    /// Exact before-bytes when the target exists.
    pub before_bytes: Option<Vec<u8>>,
    /// Intended after-state of the mutation.
    pub after: CheckpointFileState,
    /// Mutation preview line counts.
    pub preview: CheckpointPreview,
}

/// Exact logical bytes retained by one workspace checkpoint store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityUsage {
    /// Valid records retained.
    pub checkpoints: usize,
    /// Exact metadata plus preimage bytes retained.
    pub bytes: u64,
}

/// Records evicted by one automatic pruning pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneReport {
    /// Evicted checkpoint ids in eviction order.
    pub evicted: Vec<String>,
    /// Records retained after pruning.
    pub checkpoints: usize,
    /// Logical bytes retained after pruning.
    pub bytes: u64,
}

/// Outcome of one undo execution attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoApplyOutcome {
    /// The before-content was restored byte-exactly.
    Restored {
        /// Checkpoint the restore was planned from.
        id: String,
    },
    /// The deleted file was recreated from the preimage.
    Recreated {
        /// Checkpoint the recreation was planned from.
        id: String,
    },
    /// Undo would delete the created file; deletion is unavailable.
    DeleteUnavailable {
        /// Checkpoint that cannot be undone by deletion.
        id: String,
    },
    /// Current state diverged from the recorded after-state.
    Conflict {
        /// Checkpoint that no longer applies cleanly.
        id: String,
    },
}

impl UndoApplyOutcome {
    /// The checkpoint this outcome was decided for.
    pub fn id(&self) -> &str {
        match self {
            Self::Restored { id }
            | Self::Recreated { id }
            | Self::DeleteUnavailable { id }
            | Self::Conflict { id } => id,
        }
    }

    /// The exact externally observable message.
    pub fn message(&self) -> String {
        match self {
            Self::Restored { id } => {
                format!(
                    "Checkpoint {id} was undone: the before-state was restored."
                )
            }
            Self::Recreated { id } => {
                format!("Checkpoint {id} was undone: the file was recreated.")
            }
            Self::DeleteUnavailable { id } => {
                format!(
                    "Checkpoint {id} cannot be undone: undo would delete the created file and file deletion is unavailable."
                )
            }
            Self::Conflict { id } => {
                format!(
                    "Checkpoint {id} cannot be undone: the current file state no longer matches the recorded after-state."
                )
            }
        }
    }
}

/// Mint one fresh checkpoint id (`cp_` + 32 hex chars).
fn mint_checkpoint_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let preimage = format!("{nanos}-{counter}-{}", std::process::id());
    let digest = sha256_hex(preimage.as_bytes());
    format!("cp_{}", &digest[..32])
}

/// Create one `prepared` checkpoint record before a mutation.
///
/// Validates the creation against the operation-state invariant and
/// the before-bytes identity, enforces capacity (pruning evictable
/// records first), writes the preimage when the target exists, and
/// stores metadata through the validating writer (the stored record
/// re-parses, or creation fails). A partial record is removed before
/// returning an error, so failures leave no unexpected content.
pub fn create_checkpoint(
    store: &CheckpointStore,
    creation: &CheckpointCreation,
) -> Result<FileCheckpoint, CheckpointStoreError> {
    validate_creation(store, creation)?;
    for _ in 0..MAX_ID_ATTEMPTS {
        let id = mint_checkpoint_id();
        if !is_valid_checkpoint_id(&id) {
            continue;
        }
        let record = prepared_record(store, creation, &id);
        ensure_capacity_for(store, &record, creation.preimage_len())?;
        match try_create_record_directory(store, &id) {
            DirectoryOutcome::Collision => continue,
            DirectoryOutcome::Failed(error) => return Err(error),
            DirectoryOutcome::Created => {}
        }
        let outcome = write_prepared(store, &record, creation);
        if outcome.is_err() {
            remove_record_directory(store, &id);
        }
        return outcome;
    }
    Err(CheckpointStoreError::WriteFailed(
        "Checkpoint id could not be minted".to_owned(),
    ))
}

fn prepared_record(
    store: &CheckpointStore,
    creation: &CheckpointCreation,
    id: &str,
) -> FileCheckpoint {
    FileCheckpoint {
        version: CHECKPOINT_VERSION,
        id: id.to_owned(),
        workspace_fingerprint: store.workspace_fingerprint().to_owned(),
        relative_path: creation.relative_path.clone(),
        operation: creation.operation,
        tool_name: creation.tool_name.clone(),
        created_at: creation.created_at.clone(),
        state: CheckpointState::Prepared,
        before: creation.before.clone(),
        after: creation.after.clone(),
        preview: creation.preview,
    }
}

fn validate_creation(
    store: &CheckpointStore,
    creation: &CheckpointCreation,
) -> Result<(), CheckpointStoreError> {
    if validate_relative_path(&creation.relative_path).is_err() {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint relative path is invalid: {}.",
            creation.relative_path
        )));
    }
    if creation.tool_name.is_empty()
        || creation.tool_name.len() > MAX_TOOL_NAME_LENGTH
    {
        return Err(CheckpointStoreError::Invalid(
            "Checkpoint tool name is invalid.".to_owned(),
        ));
    }
    if creation.created_at.is_empty()
        || creation.created_at.len() > MAX_CREATED_AT_LENGTH
    {
        return Err(CheckpointStoreError::Invalid(
            "Checkpoint creation time is invalid.".to_owned(),
        ));
    }
    validate_checkpoint_invariant(
        creation.operation,
        creation.before.exists,
        creation.after.exists,
    )
    .map_err(|_| {
        CheckpointStoreError::Invalid(
            "Checkpoint operation disagrees with the before/after existence transition.".to_owned(),
        )
    })?;
    match (&creation.before_bytes, &creation.before) {
        (Some(bytes), before) if before.exists => {
            let length = bytes.len() as u64;
            if Some(length) != before.byte_length
                || before.sha256.as_deref() != Some(sha256_hex(bytes).as_str())
            {
                return Err(CheckpointStoreError::Invalid(
                    "Checkpoint before-bytes do not match the recorded before-state.".to_owned(),
                ));
            }
            if length > store.max_preimage_bytes_limit() {
                return Err(CheckpointStoreError::Invalid(
                    "Checkpoint before-state exceeds the preimage bound."
                        .to_owned(),
                ));
            }
        }
        (None, before) if !before.exists => {}
        _ => {
            return Err(CheckpointStoreError::Invalid(
                "Checkpoint before-bytes disagree with the recorded before-state.".to_owned(),
            ));
        }
    }
    if let Some(sha256) = creation.after.sha256.as_deref() {
        if creation.after.exists
            && (sha256.len() != 64
                || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(CheckpointStoreError::Invalid(
                "Checkpoint after-state hash is invalid.".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Whether one checkpoint directory could be claimed.
enum DirectoryOutcome {
    /// The directory was created for this record.
    Created,
    /// The id directory already exists: a mint collision (or
    /// foreign content) that is never reused.
    Collision,
    /// The directory could not be created.
    Failed(CheckpointStoreError),
}

fn try_create_record_directory(
    store: &CheckpointStore,
    id: &str,
) -> DirectoryOutcome {
    let fingerprint = store.fingerprint_directory();
    if std::fs::create_dir_all(&fingerprint).is_err() {
        return DirectoryOutcome::Failed(CheckpointStoreError::WriteFailed(
            "Checkpoint directory cannot be created.".to_owned(),
        ));
    }
    let directory = match store.checkpoint_directory(id) {
        Ok(directory) => directory,
        Err(error) => return DirectoryOutcome::Failed(error),
    };
    // `create_dir` (not `create_dir_all`) keeps id minting
    // first-writer: an existing id directory is a mint collision
    // (or foreign content) and is never reused.
    match std::fs::create_dir(&directory) {
        Ok(()) => DirectoryOutcome::Created,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            DirectoryOutcome::Collision
        }
        Err(_) => DirectoryOutcome::Failed(CheckpointStoreError::WriteFailed(
            "Checkpoint directory cannot be created.".to_owned(),
        )),
    }
}

/// The preimage byte length of one creation (0 when absent).
impl CheckpointCreation {
    /// Exact preimage bytes this creation will store.
    fn preimage_len(&self) -> u64 {
        self.before_bytes.as_ref().map_or(0, |bytes| bytes.len() as u64)
    }
}

fn write_prepared(
    store: &CheckpointStore,
    record: &FileCheckpoint,
    creation: &CheckpointCreation,
) -> Result<FileCheckpoint, CheckpointStoreError> {
    let directory = store.checkpoint_directory(&record.id)?;
    if let Some(bytes) = creation.before_bytes.as_deref() {
        let staged = stage_atomic(
            &directory,
            "preimage.bin",
            "preimage.bin.tmp",
            bytes,
            Some(CHECKPOINT_FILE_MODE),
        )
        .map_err(|_| {
            CheckpointStoreError::WriteFailed(
                "Checkpoint preimage cannot be staged".to_owned(),
            )
        })?;
        staged.commit().map_err(|_| {
            CheckpointStoreError::WriteFailed(
                "Checkpoint preimage cannot be committed".to_owned(),
            )
        })?;
        verify_preimage(&directory, bytes)?;
    }
    store.write_metadata(record)?;
    Ok(record.clone())
}

fn verify_preimage(
    directory: &Path,
    expected: &[u8],
) -> Result<(), CheckpointStoreError> {
    let path = directory.join("preimage.bin");
    let stats = std::fs::symlink_metadata(&path).map_err(|_| {
        CheckpointStoreError::WriteFailed(
            "Checkpoint preimage is missing after commit".to_owned(),
        )
    })?;
    if stats.file_type().is_symlink()
        || !stats.is_file()
        || stats.len() != expected.len() as u64
    {
        return Err(CheckpointStoreError::WriteFailed(
            "Checkpoint preimage is not the committed file".to_owned(),
        ));
    }
    match read_complete_file_bounded(&path, expected.len()) {
        BoundedFileRead::Complete(bytes) if bytes == expected => Ok(()),
        _ => Err(CheckpointStoreError::WriteFailed(
            "Checkpoint preimage does not match the before-bytes".to_owned(),
        )),
    }
}

fn remove_record_directory(store: &CheckpointStore, id: &str) {
    if let Ok(directory) = store.checkpoint_directory(id) {
        let fingerprint = store.fingerprint_directory();
        if directory.starts_with(&fingerprint)
            && directory != fingerprint
            && is_valid_checkpoint_id(id)
        {
            let _ = std::fs::remove_dir_all(&directory);
        }
    }
}

/// Account the exact logical bytes retained by one store.
///
/// Every `cp_*` record must load validly and its preimage must be
/// exactly the recorded before-state (present with matching length
/// and hash when the before-state exists, absent otherwise); any
/// unexpected directory entry blocks. Unverifiable content fails
/// the whole check: capacity is unknown, so creation cannot prove
/// it stays within bounds.
pub fn checkpoint_capacity(
    store: &CheckpointStore,
) -> Result<CapacityUsage, CheckpointStoreError> {
    let fingerprint = store.fingerprint_directory();
    let mut checkpoints = 0usize;
    let mut bytes = 0u64;
    let entries = match std::fs::read_dir(&fingerprint) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CapacityUsage { checkpoints: 0, bytes: 0 });
        }
        Err(_) => {
            return Err(CheckpointStoreError::Invalid(
                "Checkpoint store cannot be enumerated.".to_owned(),
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|_| {
            CheckpointStoreError::Invalid(
                "Checkpoint store cannot be enumerated.".to_owned(),
            )
        })?;
        let Ok(name) = entry.file_name().into_string() else {
            return Err(CheckpointStoreError::Invalid(
                "Checkpoint store carries an unreadable entry.".to_owned(),
            ));
        };
        if !name.starts_with("cp_") {
            return Err(CheckpointStoreError::Invalid(format!(
                "Checkpoint store carries unexpected content: {name}."
            )));
        }
        let loaded = store.load_metadata(&name).map_err(|_| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint cannot be verified: {name}."
            ))
        })?;
        verify_record_layout(store, &name, loaded.checkpoint.before.exists)?;
        bytes = bytes.saturating_add(loaded.metadata_bytes);
        bytes = bytes.saturating_add(preimage_len_for(
            store,
            &name,
            &loaded.checkpoint.before,
        )?);
        checkpoints += 1;
    }
    Ok(CapacityUsage { checkpoints, bytes })
}

fn verify_record_layout(
    store: &CheckpointStore,
    id: &str,
    before_exists: bool,
) -> Result<(), CheckpointStoreError> {
    let directory = store.checkpoint_directory(id)?;
    let entries = std::fs::read_dir(&directory).map_err(|_| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint cannot be verified: {id}."
        ))
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint cannot be verified: {id}."
            ))
        })?;
        let Ok(name) = entry.file_name().into_string() else {
            return Err(CheckpointStoreError::Invalid(format!(
                "Checkpoint cannot be verified: {id}."
            )));
        };
        names.push(name);
    }
    names.sort();
    let expected: &[&str] = if before_exists {
        &["metadata.json", "preimage.bin"]
    } else {
        &["metadata.json"]
    };
    if names.iter().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint carries unexpected content: {id}."
        )));
    }
    Ok(())
}

fn preimage_len_for(
    store: &CheckpointStore,
    id: &str,
    before: &CheckpointFileState,
) -> Result<u64, CheckpointStoreError> {
    let failed = || {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint preimage cannot be verified: {id}."
        ))
    };
    let directory = store.checkpoint_directory(id).map_err(|_| failed())?;
    let path = directory.join("preimage.bin");
    if !before.exists {
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(failed());
        }
        return Ok(0);
    }
    let stats = std::fs::symlink_metadata(&path).map_err(|_| failed())?;
    if stats.file_type().is_symlink() || !stats.is_file() {
        return Err(failed());
    }
    let expected = before.byte_length.ok_or_else(failed)?;
    if stats.len() != expected {
        return Err(failed());
    }
    let limit = store.max_preimage_bytes_limit() as usize;
    let bytes = match read_complete_file_bounded(&path, limit) {
        BoundedFileRead::Complete(bytes) => bytes,
        _ => return Err(failed()),
    };
    if sha256_hex(&bytes) != before.sha256.as_deref().unwrap_or("") {
        return Err(failed());
    }
    Ok(stats.len())
}

fn ensure_capacity_for(
    store: &CheckpointStore,
    record: &FileCheckpoint,
    preimage_len: u64,
) -> Result<(), CheckpointStoreError> {
    // The record is not stored yet; measure its exact serialized
    // bytes through the single canonical shape the writer persists,
    // so estimates never drift from stored bytes. Serialization of
    // the canonical shape is infallible in practice (fixed-depth
    // value); the refusal below is the fail-closed guard for a
    // measurement that cannot be taken, never an estimate.
    let record_bytes =
        serde_json::to_string_pretty(&serialize_checkpoint(record))
            .map(|text| text.len() as u64 + preimage_len)
            .map_err(|_| CheckpointStoreError::MeasurementFailed)?;
    let usage = checkpoint_capacity(store)?;
    if usage.checkpoints < store.max_checkpoints_limit()
        && usage.bytes.saturating_add(record_bytes) <= store.max_store_bytes()
    {
        return Ok(());
    }
    prune_checkpoints(store)?;
    let usage = checkpoint_capacity(store)?;
    if usage.checkpoints < store.max_checkpoints_limit()
        && usage.bytes.saturating_add(record_bytes) <= store.max_store_bytes()
    {
        return Ok(());
    }
    Err(CheckpointStoreError::CapacityExceeded(format!(
        "Checkpoint store is at capacity ({} records, {} bytes).",
        usage.checkpoints, usage.bytes
    )))
}

/// Automatically evict evictable records oldest-first until the
/// store is within its count and byte bounds.
///
/// Eviction order is deterministic: terminal non-restorable states
/// (`undone`, `abandoned`, `conflicted`, `uncertain`) before
/// `applied` (whose undo window closes), each oldest-first by
/// creation time then id. `prepared` records are pending intent
/// and are never evicted. Only records that verify are evicted;
/// unverifiable content blocks instead of being deleted.
pub fn prune_checkpoints(
    store: &CheckpointStore,
) -> Result<PruneReport, CheckpointStoreError> {
    let mut evicted = Vec::new();
    loop {
        let usage = checkpoint_capacity(store)?;
        if usage.checkpoints < store.max_checkpoints_limit()
            && usage.bytes <= store.max_store_bytes()
        {
            return Ok(PruneReport {
                evicted,
                checkpoints: usage.checkpoints,
                bytes: usage.bytes,
            });
        }
        let Some(next) = oldest_evictable(store)? else {
            return Ok(PruneReport {
                evicted,
                checkpoints: usage.checkpoints,
                bytes: usage.bytes,
            });
        };
        remove_record_directory(store, &next);
        // A vanished directory still counts as progress: the usage
        // recount observes it (or blocks on unexpected content).
        evicted.push(next);
    }
}

fn oldest_evictable(
    store: &CheckpointStore,
) -> Result<Option<String>, CheckpointStoreError> {
    let fingerprint = store.fingerprint_directory();
    let entries = match std::fs::read_dir(&fingerprint) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(_) => {
            return Err(CheckpointStoreError::Invalid(
                "Checkpoint store cannot be enumerated.".to_owned(),
            ));
        }
    };
    let mut candidates: Vec<(u8, String, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| {
            CheckpointStoreError::Invalid(
                "Checkpoint store cannot be enumerated.".to_owned(),
            )
        })?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if !name.starts_with("cp_") {
            continue;
        }
        let loaded = store.load_metadata(&name).map_err(|_| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint cannot be verified: {name}."
            ))
        })?;
        let rank = match loaded.checkpoint.state {
            CheckpointState::Undone
            | CheckpointState::Abandoned
            | CheckpointState::Conflicted
            | CheckpointState::Uncertain => 0,
            CheckpointState::Applied => 1,
            CheckpointState::Prepared => continue,
        };
        candidates.push((rank, loaded.checkpoint.created_at.clone(), name));
    }
    candidates.sort();
    Ok(candidates.into_iter().next().map(|(_, _, id)| id))
}

/// Execute the undo planned by [`plan_undo`] for one checkpoint.
///
/// The current file state must equal the recorded after-state
/// exactly; anything else is a [`UndoApplyOutcome::Conflict`] and
/// never an overwrite. Restores write the content-verified preimage
/// through an atomic compare-and-swap writer; a lost race is a
/// conflict, not a retry. Undoing a creation would delete the file
/// and stays [`UndoApplyOutcome::DeleteUnavailable`] (D5).
///
/// [`plan_undo`]: siralos_core::workspace::checkpoint::plan_undo
pub fn apply_undo(
    store: &CheckpointStore,
    id: &str,
    max_state_bytes: u64,
) -> Result<UndoApplyOutcome, CheckpointStoreError> {
    let loaded = store.load_metadata(id)?;
    let checkpoint = loaded.checkpoint;
    let current = super::checkpoint::read_workspace_file_state(
        store.workspace_root_path(),
        &checkpoint.relative_path,
        max_state_bytes,
    );
    match plan_undo(&checkpoint, &current) {
        UndoPlanDecision::ReadyRestore => {
            restore_before(store, &checkpoint, max_state_bytes)
        }
        UndoPlanDecision::ReadyCreate => recreate_before(store, &checkpoint),
        UndoPlanDecision::ReadyDelete => {
            Ok(UndoApplyOutcome::DeleteUnavailable {
                id: checkpoint.id.clone(),
            })
        }
        UndoPlanDecision::Conflict => {
            Ok(UndoApplyOutcome::Conflict { id: checkpoint.id.clone() })
        }
    }
}

fn restore_before(
    store: &CheckpointStore,
    checkpoint: &FileCheckpoint,
    max_state_bytes: u64,
) -> Result<UndoApplyOutcome, CheckpointStoreError> {
    let before_bytes = verified_preimage(store, checkpoint)?;
    let resolved = resolve_workspace_path(
        store.workspace_root_path(),
        &checkpoint.relative_path,
    )
    .map_err(|_| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint target cannot be resolved: {}.",
            checkpoint.id
        ))
    })?;
    let expected_after = checkpoint.after.sha256.clone().ok_or_else(|| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint after-state carries no hash: {}.",
            checkpoint.id
        ))
    })?;
    write_target_cas(&resolved.absolute_path, &before_bytes, &expected_after)
        .map_err(|_| {
            CheckpointStoreError::Invalid(format!(
                "Checkpoint target changed during undo: {}.",
                checkpoint.id
            ))
        })?;
    let current = super::checkpoint::read_workspace_file_state(
        store.workspace_root_path(),
        &checkpoint.relative_path,
        max_state_bytes,
    );
    if current.sha256.as_deref() != checkpoint.before.sha256.as_deref()
        || !current.exists
    {
        return Err(CheckpointStoreError::Invalid(format!(
            "Checkpoint restore could not be verified: {}.",
            checkpoint.id
        )));
    }
    store.mark_undone(&checkpoint.id)?;
    Ok(UndoApplyOutcome::Restored { id: checkpoint.id.clone() })
}

fn recreate_before(
    store: &CheckpointStore,
    checkpoint: &FileCheckpoint,
) -> Result<UndoApplyOutcome, CheckpointStoreError> {
    let before_bytes = verified_preimage(store, checkpoint)?;
    let target = resolve_create_target(
        store.workspace_root_path(),
        &checkpoint.relative_path,
    )
    .map_err(|_| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint target cannot be resolved: {}.",
            checkpoint.id
        ))
    })?;
    // The plan proved absence; `commit_if_absent` narrows the
    // first-writer window without closing it (advisory, like every
    // pathname commit in this repository).
    let staged = stage_atomic(
        &target.parent,
        &target.leaf,
        super::fs::MUTATION_TEMP_PREFIX,
        &before_bytes,
        Some(CHECKPOINT_FILE_MODE),
    )
    .map_err(|_| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint target cannot be recreated: {}.",
            checkpoint.id
        ))
    })?;
    staged.commit_if_absent().map_err(|_| {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint target changed during undo: {}.",
            checkpoint.id
        ))
    })?;
    store.mark_undone(&checkpoint.id)?;
    Ok(UndoApplyOutcome::Recreated { id: checkpoint.id.clone() })
}

/// One resolved not-yet-existing workspace target: the canonical
/// parent directory plus the validated leaf name.
pub(crate) struct PendingTarget {
    /// Canonical absolute parent directory (inside the workspace).
    pub(crate) parent: std::path::PathBuf,
    /// Validated leaf file name.
    pub(crate) leaf: String,
}

/// Resolve a target that must NOT exist yet (creation path).
///
/// The parent directory must resolve inside the canonical workspace
/// root; the leaf is one validated file-name component (no
/// separators, `.`/`..`, NUL, or drive syntax). The target itself
/// is never canonicalized (it does not exist yet), so link escape
/// through the leaf is impossible and parent escapes fail closed.
/// Lexical mistakes report their exact [`PathRejection`]; only the
/// parent contributes resolution failures.
pub(crate) fn resolve_create_target(
    root: &Path,
    requested: &str,
) -> Result<PendingTarget, PathRejection> {
    if requested.contains('\0') {
        return Err(PathRejection::NullByte);
    }
    if requested.is_empty() {
        return Err(PathRejection::Empty);
    }
    if siralos_core::workspace::path::is_absolute_pattern(requested) {
        return Err(PathRejection::Absolute);
    }
    let (parent, leaf) = match requested.rfind('/') {
        Some(index) => (&requested[..index], &requested[index + 1..]),
        None => (".", requested),
    };
    if leaf.is_empty()
        || leaf == "."
        || leaf == ".."
        || leaf.contains('\\')
        || leaf.contains(':')
        || std::path::Path::new(leaf).components().count() != 1
    {
        return Err(PathRejection::InvalidCharacter);
    }
    if requested.split('/').any(|component| component.contains(':')) {
        return Err(PathRejection::InvalidCharacter);
    }
    let resolved_parent = if parent == "." || parent.is_empty() {
        resolve_workspace_path(root, ".")?
    } else {
        resolve_workspace_path(root, parent)?
    };
    Ok(PendingTarget {
        parent: resolved_parent.absolute_path,
        leaf: leaf.to_owned(),
    })
}

fn verified_preimage(
    store: &CheckpointStore,
    checkpoint: &FileCheckpoint,
) -> Result<Vec<u8>, CheckpointStoreError> {
    let failed = || {
        CheckpointStoreError::Invalid(format!(
            "Checkpoint preimage cannot be verified: {}.",
            checkpoint.id
        ))
    };
    if !checkpoint.before.exists {
        return Err(failed());
    }
    let expected_len =
        checkpoint.before.byte_length.ok_or_else(failed)? as usize;
    if expected_len as u64 > store.max_preimage_bytes_limit() {
        return Err(failed());
    }
    let directory =
        store.checkpoint_directory(&checkpoint.id).map_err(|_| failed())?;
    let bytes = match read_complete_file_bounded(
        &directory.join("preimage.bin"),
        expected_len,
    ) {
        BoundedFileRead::Complete(bytes) => bytes,
        _ => return Err(failed()),
    };
    if bytes.len() != expected_len
        || sha256_hex(&bytes)
            != checkpoint.before.sha256.as_deref().unwrap_or("")
    {
        return Err(failed());
    }
    Ok(bytes)
}

fn write_target_cas(
    target: &Path,
    bytes: &[u8],
    expected_sha256: &str,
) -> Result<(), CheckpointStoreError> {
    let parent = target.parent().ok_or_else(|| {
        CheckpointStoreError::Invalid(
            "Checkpoint target has no parent.".to_owned(),
        )
    })?;
    let leaf = target.file_name().and_then(|name| name.to_str()).ok_or_else(
        || {
            CheckpointStoreError::Invalid(
                "Checkpoint target name is not UTF-8.".to_owned(),
            )
        },
    )?;
    let staged = stage_atomic(
        parent,
        leaf,
        super::fs::MUTATION_TEMP_PREFIX,
        bytes,
        Some(CHECKPOINT_FILE_MODE),
    )
    .map_err(|_| {
        CheckpointStoreError::Invalid(
            "Checkpoint target cannot be staged.".to_owned(),
        )
    })?;
    staged.commit_if_digest(expected_sha256).map_err(|_| {
        CheckpointStoreError::Invalid(
            "Checkpoint target changed during undo.".to_owned(),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::checkpoint::open_checkpoint_store;
    use super::{
        CheckpointCreation, UndoApplyOutcome, apply_undo, checkpoint_capacity,
        create_checkpoint, is_valid_checkpoint_id, mint_checkpoint_id,
        prune_checkpoints,
    };
    use crate::test_support::unique;
    use siralos_core::identity::sha256_hex;
    use siralos_core::workspace::checkpoint::{
        CheckpointFileState, CheckpointOperation, CheckpointPreview,
        CheckpointState, CheckpointTerminalState,
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

    fn setup()
    -> (super::super::checkpoint::CheckpointStore, std::path::PathBuf) {
        let workspace = scratch("siralos-lifecycle-ws");
        let root = scratch("siralos-lifecycle-store");
        let store = open_checkpoint_store(&workspace, &root).unwrap();
        (store, workspace)
    }

    fn present(bytes: &[u8]) -> (CheckpointFileState, Option<Vec<u8>>) {
        (
            CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(bytes)),
                byte_length: Some(bytes.len() as u64),
            },
            Some(bytes.to_vec()),
        )
    }

    fn absent() -> CheckpointFileState {
        CheckpointFileState { exists: false, sha256: None, byte_length: None }
    }

    fn update_creation(
        tag: &str,
        before: &[u8],
        after: &[u8],
    ) -> CheckpointCreation {
        let (before_state, before_bytes) = present(before);
        CheckpointCreation {
            relative_path: "a.txt".to_owned(),
            operation: CheckpointOperation::Update,
            tool_name: "edit".to_owned(),
            created_at: tag.to_owned(),
            before: before_state,
            before_bytes,
            after: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(after)),
                byte_length: Some(after.len() as u64),
            },
            preview: CheckpointPreview { added_lines: 1, removed_lines: 1 },
        }
    }

    #[test]
    fn minted_ids_are_valid_and_unique() {
        let first = mint_checkpoint_id();
        let second = mint_checkpoint_id();
        assert!(is_valid_checkpoint_id(&first), "{first}");
        assert!(is_valid_checkpoint_id(&second), "{second}");
        assert_ne!(first, second);
    }

    #[test]
    fn update_creation_round_trips_with_verified_preimage() {
        let (store, workspace) = setup();
        std::fs::write(workspace.join("a.txt"), b"old").unwrap();
        let record = create_checkpoint(
            &store,
            &update_creation("t-001", b"old", b"new"),
        )
        .unwrap();
        assert_eq!(record.state, CheckpointState::Prepared);
        assert_eq!(record.operation, CheckpointOperation::Update);
        assert_eq!(store.get(&record.id), Some(record.clone()));
        let preimage = std::fs::read(
            store
                .checkpoint_directory(&record.id)
                .unwrap()
                .join("preimage.bin"),
        )
        .unwrap();
        assert_eq!(preimage, b"old");
        let usage = checkpoint_capacity(&store).unwrap();
        assert_eq!(usage.checkpoints, 1);
        assert!(usage.bytes > 0);
    }

    #[test]
    fn create_creation_stores_no_preimage() {
        let (store, _) = setup();
        let creation = CheckpointCreation {
            relative_path: "new.txt".to_owned(),
            operation: CheckpointOperation::Create,
            tool_name: "write".to_owned(),
            created_at: "t-001".to_owned(),
            before: absent(),
            before_bytes: None,
            after: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(b"hello")),
                byte_length: Some(5),
            },
            preview: CheckpointPreview { added_lines: 1, removed_lines: 0 },
        };
        let record = create_checkpoint(&store, &creation).unwrap();
        let mut names: Vec<String> =
            std::fs::read_dir(store.checkpoint_directory(&record.id).unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
        names.sort();
        assert_eq!(names, vec!["metadata.json".to_owned()]);
    }

    #[test]
    fn creation_rejects_mismatched_bytes_and_bad_transitions() {
        let (store, _) = setup();
        let (mut before, _) = present(b"old");
        before.sha256 = Some("0".repeat(64));
        let mismatched = CheckpointCreation {
            relative_path: "a.txt".to_owned(),
            operation: CheckpointOperation::Update,
            tool_name: "edit".to_owned(),
            created_at: "t-001".to_owned(),
            before,
            before_bytes: Some(b"old".to_vec()),
            after: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(b"new")),
                byte_length: Some(3),
            },
            preview: CheckpointPreview { added_lines: 1, removed_lines: 1 },
        };
        assert!(matches!(
            create_checkpoint(&store, &mismatched),
            Err(super::super::checkpoint::CheckpointStoreError::Invalid(_))
        ));
        let (before_state, before_bytes) = present(b"old");
        let bad_transition = CheckpointCreation {
            relative_path: "a.txt".to_owned(),
            operation: CheckpointOperation::Create,
            tool_name: "write".to_owned(),
            created_at: "t-001".to_owned(),
            before: before_state,
            before_bytes,
            after: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(b"new")),
                byte_length: Some(3),
            },
            preview: CheckpointPreview { added_lines: 1, removed_lines: 0 },
        };
        assert!(matches!(
            create_checkpoint(&store, &bad_transition),
            Err(super::super::checkpoint::CheckpointStoreError::Invalid(_))
        ));
        assert_eq!(checkpoint_capacity(&store).unwrap().checkpoints, 0);
    }

    #[test]
    fn unexpected_store_content_blocks_capacity_and_creation() {
        let (store, _) = setup();
        let record =
            create_checkpoint(&store, &update_creation("t-001", b"a", b"b"))
                .unwrap();
        std::fs::write(
            store.fingerprint_directory().join("intruder.txt"),
            b"nope",
        )
        .unwrap();
        assert!(checkpoint_capacity(&store).is_err());
        assert!(
            create_checkpoint(&store, &update_creation("t-002", b"a", b"b"))
                .is_err()
        );
        // Inspection still skips nothing valid: the good record loads.
        assert!(store.get(&record.id).is_some());
        std::fs::remove_file(
            store.fingerprint_directory().join("intruder.txt"),
        )
        .unwrap();
        create_checkpoint(&store, &update_creation("t-003", b"a", b"b"))
            .unwrap();
    }

    #[test]
    fn prune_evicts_terminal_oldest_first_and_never_prepared() {
        let (store, _) = setup();
        let mut ids = Vec::new();
        for index in 0..60 {
            let record = create_checkpoint(
                &store,
                &update_creation(&format!("t-{index:05}"), b"a", b"b"),
            )
            .unwrap();
            ids.push(record.id);
        }
        for id in ids.iter().take(30) {
            store.mark_state(id, CheckpointTerminalState::Abandoned).unwrap();
        }
        for id in ids.iter().skip(30).take(10) {
            store.mark_state(id, CheckpointTerminalState::Applied).unwrap();
        }
        // Fill to the count bound; further creations prune
        // automatically, one oldest evictable record per creation.
        for index in 60..100 {
            create_checkpoint(
                &store,
                &update_creation(&format!("t-{index:05}"), b"a", b"b"),
            )
            .unwrap();
        }
        assert_eq!(checkpoint_capacity(&store).unwrap().checkpoints, 100);
        // A direct prune at exactly the bound evicts the single
        // oldest terminal record and stops.
        let report = prune_checkpoints(&store).unwrap();
        assert_eq!(report.evicted, vec![ids[0].clone()]);
        assert!(store.get(&ids[0]).is_none());
        // Thirty-one more creations evict the remaining abandoned
        // records oldest-first, then the oldest applied record
        // (the first creation after the direct prune needs no
        // eviction: the prune left the count below the bound).
        for index in 100..131 {
            create_checkpoint(
                &store,
                &update_creation(&format!("t-{index:05}"), b"a", b"b"),
            )
            .unwrap();
        }
        let usage = checkpoint_capacity(&store).unwrap();
        assert!(usage.checkpoints <= 100, "{usage:?}");
        for id in ids.iter().take(30) {
            assert!(store.get(id).is_none(), "abandoned {id} pruned");
        }
        assert!(
            store.get(&ids[30]).is_none(),
            "oldest applied pruned after all abandoned"
        );
        for id in ids.iter().skip(31).take(9) {
            assert!(store.get(id).is_some(), "newer applied {id} retained");
        }
        for id in ids.iter().skip(40) {
            assert!(store.get(id).is_some(), "prepared {id} retained");
        }
    }

    #[test]
    fn prepared_only_overflow_refuses_creation() {
        let (store, _) = setup();
        for index in 0..100 {
            create_checkpoint(
                &store,
                &update_creation(&format!("t-{index:05}"), b"a", b"b"),
            )
            .unwrap();
        }
        assert!(matches!(
            create_checkpoint(&store, &update_creation("t-00100", b"a", b"b")),
            Err(super::super::checkpoint::CheckpointStoreError::CapacityExceeded(_))
        ));
    }

    #[test]
    fn undo_restores_exact_bytes_and_marks_undone() {
        let (store, workspace) = setup();
        std::fs::write(workspace.join("a.txt"), b"old").unwrap();
        let record = create_checkpoint(
            &store,
            &update_creation("t-001", b"old", b"new"),
        )
        .unwrap();
        std::fs::write(workspace.join("a.txt"), b"new").unwrap();
        let outcome = apply_undo(&store, &record.id, 1024 * 1024).unwrap();
        assert_eq!(
            outcome,
            UndoApplyOutcome::Restored { id: record.id.clone() }
        );
        assert_eq!(std::fs::read(workspace.join("a.txt")).unwrap(), b"old");
        assert_eq!(
            store.get(&record.id).map(|record| record.state),
            Some(CheckpointState::Undone)
        );
        // The undone record no longer matches: conflict, never a
        // second restore.
        let outcome = apply_undo(&store, &record.id, 1024 * 1024).unwrap();
        assert_eq!(
            outcome,
            UndoApplyOutcome::Conflict { id: record.id.clone() }
        );
        assert_eq!(std::fs::read(workspace.join("a.txt")).unwrap(), b"old");
    }

    #[test]
    fn undo_of_create_is_delete_unavailable_and_leaves_bytes() {
        let (store, workspace) = setup();
        let creation = CheckpointCreation {
            relative_path: "new.txt".to_owned(),
            operation: CheckpointOperation::Create,
            tool_name: "write".to_owned(),
            created_at: "t-001".to_owned(),
            before: absent(),
            before_bytes: None,
            after: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(b"hello")),
                byte_length: Some(5),
            },
            preview: CheckpointPreview { added_lines: 1, removed_lines: 0 },
        };
        let record = create_checkpoint(&store, &creation).unwrap();
        std::fs::write(workspace.join("new.txt"), b"hello").unwrap();
        let outcome = apply_undo(&store, &record.id, 1024 * 1024).unwrap();
        assert_eq!(
            outcome,
            UndoApplyOutcome::DeleteUnavailable { id: record.id.clone() }
        );
        assert_eq!(
            std::fs::read(workspace.join("new.txt")).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn undo_conflict_never_overwrites_newer_work() {
        let (store, workspace) = setup();
        std::fs::write(workspace.join("a.txt"), b"old").unwrap();
        let record = create_checkpoint(
            &store,
            &update_creation("t-001", b"old", b"new"),
        )
        .unwrap();
        std::fs::write(workspace.join("a.txt"), b"new").unwrap();
        std::fs::write(workspace.join("a.txt"), b"third-party").unwrap();
        let outcome = apply_undo(&store, &record.id, 1024 * 1024).unwrap();
        assert_eq!(
            outcome,
            UndoApplyOutcome::Conflict { id: record.id.clone() }
        );
        assert_eq!(
            std::fs::read(workspace.join("a.txt")).unwrap(),
            b"third-party"
        );
    }

    #[test]
    fn undo_recreates_a_deleted_file_from_the_preimage() {
        let (store, workspace) = setup();
        std::fs::write(workspace.join("gone.txt"), b"kept").unwrap();
        let (before, before_bytes) = present(b"kept");
        let creation = CheckpointCreation {
            relative_path: "gone.txt".to_owned(),
            operation: CheckpointOperation::Delete,
            tool_name: "test".to_owned(),
            created_at: "t-001".to_owned(),
            before,
            before_bytes,
            after: absent(),
            preview: CheckpointPreview { added_lines: 0, removed_lines: 1 },
        };
        let record = create_checkpoint(&store, &creation).unwrap();
        std::fs::remove_file(workspace.join("gone.txt")).unwrap();
        let outcome = apply_undo(&store, &record.id, 1024 * 1024).unwrap();
        assert_eq!(
            outcome,
            UndoApplyOutcome::Recreated { id: record.id.clone() }
        );
        assert_eq!(
            std::fs::read(workspace.join("gone.txt")).unwrap(),
            b"kept"
        );
    }
}
