//! User-facing checkpoint undo (ADR 0038): list restorable
//! checkpoints recorded by the default `write`/`edit` tools and
//! restore their preimages byte-exactly behind an explicit
//! confirmation.
//!
//! Undo is a USER operation over the user's own approved writes:
//! the model gets no undo tool and its mutation authority is
//! unchanged. Every restore re-applies the same gates as the
//! tools (containment, protected paths, atomic writer, observed
//! revision safety) and refuses typed instead of guessing.
//!
//! The store is opened at the edges ([`open_undo_store`]) and the
//! logic below is store-bound, so tests run against temporary
//! stores while production opens the default store.
//! Rendering helpers below return raw text; every dispatch site
//! sanitizes before display.

use std::path::Path;

use siralos_adapters::workspace::checkpoint::{
    MAX_PREIMAGE_BYTES, is_valid_checkpoint_id,
};
use siralos_adapters::workspace::checkpoint_lifecycle::{
    UndoApplyOutcome, apply_undo,
};
use siralos_core::workspace::checkpoint::{CheckpointState, FileCheckpoint};

/// Tools whose checkpoints undo covers: the default mutation
/// tools only. Records from any other tool are never listed and
/// refuse by id.
const UNDO_TOOL_NAMES: [&str; 2] = ["write", "edit"];

// Re-export the store types undo callers name so dispatch sites
// need only this module.
pub use siralos_adapters::workspace::checkpoint::CheckpointStore;

/// Machine-branchable class of an undo failure. Messages stay
/// stable for display and the differential harness; control flow
/// and tests match on `kind`, never prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoErrorKind {
    /// The checkpoint store cannot be opened (unexpected store
    /// content blocks here; nothing is repaired or deleted).
    StoreUnavailable,
    /// No checkpoint carries the addressed id, or its metadata
    /// cannot be verified.
    UnknownCheckpoint,
    /// The record exists but cannot restore: wrong lifecycle
    /// state, foreign tool, missing preimage identity, or an
    /// undo that would delete a created file (D5).
    NotRestorable,
    /// The target is protected from model-facing mutation.
    ProtectedTarget,
    /// The recorded target escapes the workspace.
    UnresolvableTarget,
    /// The restore itself failed (tampered preimage, lost race):
    /// the message carries the store error.
    RestoreFailed,
}

/// One undo failure: a branchable kind plus the stable
/// human-readable reason.
#[derive(Debug, PartialEq, Eq)]
pub struct UndoError {
    /// Machine-branchable failure class.
    pub kind: UndoErrorKind,
    /// Deterministic, human-readable reason.
    pub message: String,
}

fn error(kind: UndoErrorKind, message: impl Into<String>) -> UndoError {
    UndoError { kind, message: message.into() }
}

/// The recorded confirmation behind one restore. A dedicated type
/// (never `bool`) so call sites name the decision and a bare
/// boolean can never be misread as confirmed. The harness corpus
/// keeps the boolean `confirmed` wire field and maps it to this
/// type at the boundary; the display text is unchanged either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoConfirmation {
    /// The user approved the exact restore the prompt recorded.
    Confirmed,
    /// The user declined (or the prompt never ran): refuse before
    /// touching the store.
    Unconfirmed,
}

impl UndoConfirmation {
    /// Whether the restore may proceed.
    #[must_use]
    pub fn is_confirmed(self) -> bool {
        matches!(self, Self::Confirmed)
    }
}

/// One restorable checkpoint resolved for listing or restore.
#[derive(Debug, PartialEq, Eq)]
pub struct UndoPlan {
    /// Addressed checkpoint id.
    pub id: String,
    /// Workspace-relative restore target.
    pub target: String,
    /// Exact preimage byte length.
    pub bytes: u64,
    /// Exact preimage SHA-256.
    pub digest: String,
    /// Record creation time, for display.
    pub created_at: String,
}

/// Open the workspace checkpoint store: the same store the
/// default tools record into (outside the workspace under the
/// caller state directory). Crate-visible: dispatch sites open
/// the store, then call the store-bound logic below.
pub(crate) fn open_undo_store(
    root: &Path,
) -> Result<CheckpointStore, UndoError> {
    siralos_adapters::tool::WriteTool::default_store(root).map_err(|store| {
        error(
            UndoErrorKind::StoreUnavailable,
            format!("undo unavailable: {store}"),
        )
    })
}

/// Resolve one checkpoint id to a restore plan, refusing every
/// non-restorable case typed. Re-resolution at execute time
/// re-runs the same gates on a fresh read; the prompt and the
/// restore are not digest-bound to each other (the binding is
/// `apply_undo`'s own compare-and-swap plus post-restore
/// verification).
pub fn plan_undo_restore(
    store: &CheckpointStore,
    id: &str,
) -> Result<UndoPlan, UndoError> {
    if !is_valid_checkpoint_id(id) {
        return Err(error(
            UndoErrorKind::UnknownCheckpoint,
            format!("no restorable checkpoint {id:?}"),
        ));
    }
    let Some(record) = store.get(id) else {
        return Err(error(
            UndoErrorKind::UnknownCheckpoint,
            format!("no restorable checkpoint {id:?}"),
        ));
    };
    refuse_unrestorable(&record)?;
    Ok(UndoPlan {
        id: record.id.clone(),
        target: record.relative_path.clone(),
        bytes: record.before.byte_length.unwrap_or(0),
        digest: record.before.sha256.clone().unwrap_or_default(),
        created_at: record.created_at.clone(),
    })
}

/// Refuse a loaded record that cannot restore, naming the reason.
fn refuse_unrestorable(record: &FileCheckpoint) -> Result<(), UndoError> {
    if record.state != CheckpointState::Applied {
        return Err(error(
            UndoErrorKind::NotRestorable,
            format!(
                "checkpoint {} is {}, only applied checkpoints restore",
                record.id,
                record.state.as_str()
            ),
        ));
    }
    if !UNDO_TOOL_NAMES.contains(&record.tool_name.as_str()) {
        return Err(error(
            UndoErrorKind::NotRestorable,
            format!(
                "checkpoint {} was not recorded by write or edit",
                record.id
            ),
        ));
    }
    if siralos_adapters::workspace::fs::is_model_protected_workspace_path(
        &record.relative_path,
    ) {
        return Err(error(
            UndoErrorKind::ProtectedTarget,
            format!(
                "checkpoint {} targets protected configuration; refusing restore",
                record.id
            ),
        ));
    }
    if siralos_core::workspace::path::validate_relative_path(
        &record.relative_path,
    )
    .is_err()
    {
        return Err(error(
            UndoErrorKind::UnresolvableTarget,
            format!(
                "checkpoint {} target escapes the workspace; refusing restore",
                record.id
            ),
        ));
    }
    if !record.before.exists
        || record.before.sha256.is_none()
        || record.before.byte_length.is_none()
    {
        return Err(error(
            UndoErrorKind::NotRestorable,
            format!(
                "checkpoint {} cannot be undone: it has no restorable before-state",
                record.id
            ),
        ));
    }
    Ok(())
}

/// Render the restore confirmation prompt: the exact restore the
/// user authorizes (target, bytes, digest). Raw text; dispatch
/// sanitizes.
#[must_use]
pub fn render_undo_prompt(plan: &UndoPlan) -> String {
    format!(
        "Restore {} to {} bytes (sha256:{})?\nCheckpoint {} ({}). This overwrites the current content. (y/N)\n",
        plan.target, plan.bytes, plan.digest, plan.id, plan.created_at
    )
}

/// Resolve the most recent restorable checkpoint: records list
/// newest-first, so the first applicable record wins. Restorability
/// is exactly [`refuse_unrestorable`] succeeding, so the list,
/// `--last` resolution, and the plan gate can never disagree on
/// what is applicable.
pub fn resolve_last_restorable(
    store: &CheckpointStore,
) -> Result<String, UndoError> {
    for record in store.list(None, None) {
        if refuse_unrestorable(&record).is_ok() {
            return Ok(record.id.clone());
        }
    }
    Err(error(
        UndoErrorKind::UnknownCheckpoint,
        "no restorable checkpoints".to_owned(),
    ))
}

/// Render the restorable-checkpoint list (id, target, operation,
/// bytes, timestamp). Raw text; dispatch sanitizes.
#[must_use]
pub fn render_undo_list(store: &CheckpointStore) -> String {
    let restorable: Vec<FileCheckpoint> = store
        .list(None, None)
        .into_iter()
        .filter(|record| refuse_unrestorable(record).is_ok())
        .collect();
    if restorable.is_empty() {
        return "No restorable checkpoints.\n".to_owned();
    }
    let mut out = String::from("Restorable checkpoints:\n");
    for record in &restorable {
        out.push_str(&format!(
            "  {} - {} ({}, {} bytes, {})\n",
            record.id,
            record.relative_path,
            record.operation.as_str(),
            record.before.byte_length.unwrap_or(0),
            record.created_at
        ));
    }
    out
}

/// Execute one restore behind a recorded confirmation. Every
/// outcome maps to display text; [`UndoConfirmation::Unconfirmed`]
/// refuses before touching the store, so no restore happens
/// without the confirmation the prompt recorded. The plan gates
/// re-run here (fresh read), then `apply_undo` performs its own
/// compare-and-swap plus post-restore verification. Raw text;
/// dispatch sanitizes.
#[must_use]
pub fn execute_undo_restore(
    store: &CheckpointStore,
    id: &str,
    confirmation: UndoConfirmation,
) -> String {
    if !confirmation.is_confirmed() {
        return format!("undo of {id} cancelled — no changes made\n");
    }
    let outcome = (|| -> Result<UndoApplyOutcome, UndoError> {
        let record = store.get(id).ok_or_else(|| {
            error(
                UndoErrorKind::UnknownCheckpoint,
                format!("no restorable checkpoint {id:?}"),
            )
        })?;
        refuse_unrestorable(&record)?;
        apply_undo(store, id, MAX_PREIMAGE_BYTES).map_err(|store_error| {
            // Messages carry no trailing newline here; the final
            // display format below adds exactly one.
            match store_error {
                siralos_adapters::workspace::checkpoint::CheckpointStoreError::Invalid(
                    detail,
                ) => error(
                    UndoErrorKind::RestoreFailed,
                    detail.to_string(),
                ),
                other => error(
                    UndoErrorKind::RestoreFailed,
                    format!("undo of {id} failed: {other}"),
                ),
            }
        })
    })();
    match outcome {
        // The adapter outcome strings are the stable vocabulary
        // (Restored, Recreated, typed DeleteUnavailable/Conflict
        // refusals); unexpected store content never repairs.
        Ok(result) => format!("{}\n", result.message()),
        Err(error) => format!("{}\n", error.message),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        UndoConfirmation, UndoErrorKind, execute_undo_restore,
        plan_undo_restore, render_undo_list, resolve_last_restorable,
    };
    use siralos_adapters::workspace::checkpoint::open_checkpoint_store;
    use siralos_adapters::workspace::checkpoint_lifecycle::{
        CheckpointCreation, create_checkpoint,
    };
    use siralos_core::identity::sha256_hex;
    use siralos_core::workspace::checkpoint::{
        CheckpointFileState, CheckpointOperation, CheckpointPreview,
        CheckpointState, CheckpointTerminalState,
    };

    struct UndoFixture {
        root: std::path::PathBuf,
        state: std::path::PathBuf,
        store: siralos_adapters::workspace::checkpoint::CheckpointStore,
    }

    fn fixture(label: &str) -> UndoFixture {
        // The checkpoint state lives BESIDE the workspace, like the
        // production state directory: a store rooted inside its own
        // workspace is refused.
        let base = std::env::temp_dir()
            .join(format!("siralos-undo-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("ws");
        let state = base.join("state");
        std::fs::create_dir_all(&root).expect("workspace dir");
        std::fs::create_dir_all(&state).expect("state dir");
        let store = open_checkpoint_store(&root, &state).expect("open store");
        UndoFixture { root, state, store }
    }

    fn file_state(bytes: &[u8]) -> CheckpointFileState {
        CheckpointFileState {
            exists: true,
            sha256: Some(sha256_hex(bytes)),
            byte_length: Some(bytes.len() as u64),
        }
    }

    fn absent_state() -> CheckpointFileState {
        CheckpointFileState { exists: false, sha256: None, byte_length: None }
    }

    /// Seed one applied `write` record: `name` holds `after`, the
    /// record pins `before` → `after` at `created_at`.
    fn seed_applied(
        fixture: &UndoFixture,
        name: &str,
        before: &[u8],
        after: &[u8],
        created_at: &str,
    ) -> String {
        std::fs::write(fixture.root.join(name), after).expect("seed file");
        let record = create_checkpoint(
            &fixture.store,
            &CheckpointCreation {
                relative_path: name.to_owned(),
                operation: CheckpointOperation::Update,
                tool_name: "write".to_owned(),
                created_at: created_at.to_owned(),
                before: file_state(before),
                before_bytes: Some(before.to_vec()),
                after: file_state(after),
                preview: CheckpointPreview {
                    added_lines: 1,
                    removed_lines: 0,
                },
            },
        )
        .expect("seed record");
        fixture
            .store
            .mark_state(&record.id, CheckpointTerminalState::Applied)
            .expect("mark applied");
        record.id
    }

    fn teardown(fixture: &UndoFixture) {
        let _ = std::fs::remove_dir_all(fixture.root.parent().expect("base"));
    }

    #[test]
    fn undo_restores_byte_exactly_and_marks_undone() {
        let fixture = fixture("restore");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        let text = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Confirmed,
        );
        assert_eq!(
            text,
            format!(
                "Checkpoint {id} was undone: the before-state was restored.\n"
            )
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"before bytes\n"
        );
        assert_eq!(
            fixture.store.get(&id).expect("record").state,
            CheckpointState::Undone
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_second_restore_conflicts() {
        let fixture = fixture("double");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        let _ = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Confirmed,
        );
        let text = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Confirmed,
        );
        assert_eq!(
            text,
            format!(
                "checkpoint {id} is undone, only applied checkpoints restore\n"
            )
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"before bytes\n"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_mutated_since_conflicts_without_overwriting() {
        let fixture = fixture("stale");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        std::fs::write(fixture.root.join("a.txt"), b"newer work\n")
            .expect("external edit");
        let text = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Confirmed,
        );
        assert!(
            text.contains("no longer matches"),
            "stale restore must conflict, got: {text:?}"
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"newer work\n",
            "newer work is never overwritten"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_without_confirmation_cancels_without_touching_store() {
        let fixture = fixture("unconfirmed");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        let text = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Unconfirmed,
        );
        assert_eq!(
            text,
            format!("undo of {id} cancelled — no changes made\n")
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"after bytes\n"
        );
        assert_eq!(
            fixture.store.get(&id).expect("record").state,
            CheckpointState::Applied,
            "unconfirmed restore touches nothing"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_unknown_id_refuses_typed() {
        let fixture = fixture("unknown");
        let error = plan_undo_restore(&fixture.store, "cp_0000000000")
            .expect_err("unknown id must refuse");
        assert!(
            matches!(error.kind, UndoErrorKind::UnknownCheckpoint),
            "got: {error:?}"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_lists_only_restorable_and_last_resolves_newest() {
        let fixture = fixture("list");
        let old = seed_applied(
            &fixture,
            "old.txt",
            b"old before\n",
            b"old after\n",
            "2026-01-01T00:00:00Z",
        );
        let new = seed_applied(
            &fixture,
            "new.txt",
            b"new before\n",
            b"new after\n",
            "2026-01-01T00:00:01Z",
        );
        // Prepared (never applied) records do not list.
        std::fs::write(fixture.root.join("pending.txt"), b"pending\n")
            .expect("seed file");
        let pending = create_checkpoint(
            &fixture.store,
            &CheckpointCreation {
                relative_path: "pending.txt".to_owned(),
                operation: CheckpointOperation::Update,
                tool_name: "write".to_owned(),
                created_at: "2026-01-01T00:00:02Z".to_owned(),
                before: file_state(b"pending\n"),
                before_bytes: Some(b"pending\n".to_vec()),
                after: file_state(b"pending\n"),
                preview: CheckpointPreview {
                    added_lines: 0,
                    removed_lines: 0,
                },
            },
        )
        .expect("seed pending");
        // Foreign tools never list.
        let foreign = create_checkpoint(
            &fixture.store,
            &CheckpointCreation {
                relative_path: "old.txt".to_owned(),
                operation: CheckpointOperation::Update,
                tool_name: "test-tool".to_owned(),
                created_at: "2026-01-01T00:00:03Z".to_owned(),
                before: file_state(b"old before\n"),
                before_bytes: Some(b"old before\n".to_vec()),
                after: file_state(b"old after\n"),
                preview: CheckpointPreview {
                    added_lines: 0,
                    removed_lines: 0,
                },
            },
        )
        .expect("seed foreign");
        fixture
            .store
            .mark_state(&foreign.id, CheckpointTerminalState::Applied)
            .expect("mark foreign");
        let listed = render_undo_list(&fixture.store);
        assert!(listed.contains(&old), "{listed:?}");
        assert!(listed.contains(&new), "{listed:?}");
        assert!(!listed.contains(&pending.id), "{listed:?}");
        assert!(!listed.contains(&foreign.id), "{listed:?}");
        assert_eq!(
            resolve_last_restorable(&fixture.store).expect("last"),
            new,
            "newest restorable wins"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_of_create_refuses_without_deleting() {
        let fixture = fixture("create");
        std::fs::write(fixture.root.join("made.txt"), b"created\n")
            .expect("seed file");
        let record = create_checkpoint(
            &fixture.store,
            &CheckpointCreation {
                relative_path: "made.txt".to_owned(),
                operation: CheckpointOperation::Create,
                tool_name: "write".to_owned(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                before: absent_state(),
                before_bytes: None,
                after: file_state(b"created\n"),
                preview: CheckpointPreview {
                    added_lines: 1,
                    removed_lines: 0,
                },
            },
        )
        .expect("seed create");
        fixture
            .store
            .mark_state(&record.id, CheckpointTerminalState::Applied)
            .expect("mark applied");
        // Not listed: undo would delete the created file (D5).
        assert!(
            !render_undo_list(&fixture.store).contains(&record.id),
            "create-records never list"
        );
        let text = execute_undo_restore(
            &fixture.store,
            &record.id,
            UndoConfirmation::Confirmed,
        );
        assert!(
            text.contains("cannot be undone"),
            "undo-of-create refuses typed, got: {text:?}"
        );
        assert!(
            fixture.root.join("made.txt").exists(),
            "the created file is never deleted"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_protected_target_refuses() {
        let fixture = fixture("protected");
        std::fs::create_dir_all(fixture.root.join(".siralos")).expect("dir");
        std::fs::write(
            fixture.root.join(".siralos").join("evil.toml"),
            b"planted\n",
        )
        .expect("seed file");
        let record = create_checkpoint(
            &fixture.store,
            &CheckpointCreation {
                relative_path: ".siralos/evil.toml".to_owned(),
                operation: CheckpointOperation::Update,
                tool_name: "write".to_owned(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                before: file_state(b"original\n"),
                before_bytes: Some(b"original\n".to_vec()),
                after: file_state(b"planted\n"),
                preview: CheckpointPreview {
                    added_lines: 1,
                    removed_lines: 0,
                },
            },
        )
        .expect("seed record");
        fixture
            .store
            .mark_state(&record.id, CheckpointTerminalState::Applied)
            .expect("mark applied");
        let error = plan_undo_restore(&fixture.store, &record.id)
            .expect_err("protected target must refuse");
        assert!(
            matches!(error.kind, UndoErrorKind::ProtectedTarget),
            "got: {error:?}"
        );
        assert!(
            !render_undo_list(&fixture.store).contains(&record.id),
            "protected records never list"
        );
        assert_eq!(
            std::fs::read(fixture.root.join(".siralos").join("evil.toml"))
                .expect("read back"),
            b"planted\n",
            "protected content untouched"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_tampered_preimage_refuses() {
        let fixture = fixture("tamper");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        // Corrupt the stored preimage: same length, different digest.
        let preimage = find_record_file(&fixture, &id, "preimage.bin");
        std::fs::write(&preimage, b"BEFORE BYTES\n").expect("tamper");
        let text = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Confirmed,
        );
        assert_eq!(
            text,
            format!("Checkpoint preimage cannot be verified: {id}.\n")
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"after bytes\n",
            "tampered restore writes nothing"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_poisoned_record_refuses_without_repair() {
        let fixture = fixture("poison");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        // Corrupt the record metadata itself.
        let metadata = find_record_file(&fixture, &id, "metadata.json");
        std::fs::write(&metadata, b"{oops").expect("poison");
        let error = plan_undo_restore(&fixture.store, &id)
            .expect_err("poisoned record must refuse");
        assert!(
            matches!(error.kind, UndoErrorKind::UnknownCheckpoint),
            "got: {error:?}"
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"after bytes\n"
        );
        assert_eq!(
            std::fs::read(&metadata).expect("poison intact"),
            b"{oops",
            "poison is never repaired or deleted"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_unrelated_store_poison_does_not_block_valid_restore() {
        let fixture = fixture("intruder");
        let id = seed_applied(
            &fixture,
            "a.txt",
            b"before bytes\n",
            b"after bytes\n",
            "2026-01-01T00:00:00Z",
        );
        // Poison beside the valid record: the restore still runs
        // and the poison is left exactly alone.
        let fingerprint = fixture.store.workspace_fingerprint().to_owned();
        let intruder = fixture.state.join(&fingerprint).join("intruder.txt");
        std::fs::write(&intruder, b"junk").expect("intruder");
        let text = execute_undo_restore(
            &fixture.store,
            &id,
            UndoConfirmation::Confirmed,
        );
        assert!(
            text.contains("was undone"),
            "valid restore proceeds, got: {text:?}"
        );
        assert_eq!(
            std::fs::read(fixture.root.join("a.txt")).expect("read back"),
            b"before bytes\n"
        );
        assert_eq!(
            std::fs::read(&intruder).expect("intruder intact"),
            b"junk",
            "intruder is never cleaned up"
        );
        teardown(&fixture);
    }

    #[test]
    fn undo_traversal_target_refuses_typed() {
        // Defense in depth: the store loader rejects escaping
        // targets first, so this gate fires only for a record that
        // reaches planning with a hostile path. Fabricate it
        // directly to pin the refusal.
        let record = siralos_core::workspace::checkpoint::FileCheckpoint {
            version: 1,
            id: "cp_aaaa000000".to_owned(),
            workspace_fingerprint: "fp".to_owned(),
            relative_path: "../evil.toml".to_owned(),
            operation:
                siralos_core::workspace::checkpoint::CheckpointOperation::Update,
            tool_name: "write".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            state: siralos_core::workspace::checkpoint::CheckpointState::Applied,
            before: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(b"x")),
                byte_length: Some(1),
            },
            after: CheckpointFileState {
                exists: true,
                sha256: Some(sha256_hex(b"y")),
                byte_length: Some(1),
            },
            preview: CheckpointPreview {
                added_lines: 1,
                removed_lines: 0,
            },
        };
        let error = super::refuse_unrestorable(&record)
            .expect_err("escaping target must refuse");
        assert!(
            matches!(error.kind, UndoErrorKind::UnresolvableTarget),
            "got: {error:?}"
        );
    }

    /// Locate one record file under the temp store by walking the
    /// fingerprint directory for the record id.
    fn find_record_file(
        fixture: &UndoFixture,
        id: &str,
        leaf: &str,
    ) -> std::path::PathBuf {
        let fingerprint = fixture.store.workspace_fingerprint().to_owned();
        fixture.state.join(fingerprint).join(id).join(leaf)
    }
}
