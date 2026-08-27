//! Prepared scene/resource mutation orchestration (Stage 3 milestone 10,
//! ADR 0026; R9 parity slice).
//!
//! Mirrors the orchestration half of
//! `packages/adapters/src/godot/scene-mutation/scene-mutation-service.ts`
//! for the frozen R9 boundary: preparation delegates to the core
//! contracts ([`siralos_godot::godot::scene_mutation`]) and application is
//! typed [`GodotMutationApplyOutcome::Unavailable`] before any approval,
//! checkpoint, mirror, or write — Node offers no directory-relative
//! create/replace/delete primitive, so nothing here ever mutates the
//! filesystem or launches a process (`SECURITY.md`).

use siralos_godot::godot::scene_mutation::{
    CreatePreparedGodotMutationInput, GodotMutationPreview, MutationError,
    MutationKind, MutationOperation, PreparedGodotMutation,
    create_prepared_godot_mutation, expected_semantic_effect,
    validate_mutation_operations,
};

/// Truthful reason returned by every apply attempt while the
/// directory-relative write primitive cannot bind execution to the
/// approved bytes.
pub const GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE: &str = "Scene and resource mutation is unavailable on this platform: the prepared bytes cannot be written through a directory-relative, identity-bound primitive, the required checkpoint cannot be created, and post-apply verification could not be guaranteed against a substituted file, because Node offers no openat/renameat-style primitive. Nothing was created, nothing was modified, and no checkpoint was taken.";

/// Inputs for one preparation attempt.
pub struct GodotSceneMutationPrepareRequest {
    /// Target document path (workspace-relative).
    pub target_path: String,
    /// Exact source revision handle (`rev_…`).
    pub source_revision: String,
    /// SHA-256 of the exact source text.
    pub source_sha256: String,
    /// Document kind.
    pub kind: MutationKind,
    /// Validated operation set.
    pub operations: Vec<MutationOperation>,
    /// Complete preview.
    pub preview: GodotMutationPreview,
    /// Deterministic serialized after-text.
    pub serialized_after: String,
    /// Added line count.
    pub added_lines: u64,
    /// Removed line count.
    pub removed_lines: u64,
}

/// Outcome of one apply attempt. Single-variant at this stage: every
/// attempt truthfully reports `unavailable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GodotMutationApplyOutcome {
    /// Execution refuses before any effect.
    Unavailable {
        /// Bounded truthful reason.
        message: String,
    },
}

/// Prepare-only scene/resource mutation orchestration over the core
/// contracts. No constructor state today; the type anchors future
/// session-bound orchestration without widening authority.
#[derive(Debug, Clone, Copy, Default)]
pub struct GodotSceneMutationService;

impl GodotSceneMutationService {
    /// Create the prepare-only orchestration surface.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Validate the operation set and bind the immutable prepared
    /// mutation to the exact source revision. Validation delegates to
    /// the core contract; identical inputs yield identical fingerprints.
    pub fn prepare(
        &self,
        request: &GodotSceneMutationPrepareRequest,
    ) -> Result<PreparedGodotMutation, MutationError> {
        let operations = validate_mutation_operations(&request.operations)?;
        let _ = expected_semantic_effect(&operations);
        create_prepared_godot_mutation(CreatePreparedGodotMutationInput {
            target_path: request.target_path.clone(),
            source_revision: request.source_revision.clone(),
            source_sha256: request.source_sha256.clone(),
            kind: request.kind,
            operations,
            preview: GodotMutationPreview {
                structural_summary: request.preview.structural_summary.clone(),
                diff: request.preview.diff.clone(),
            },
            serialized_after: request.serialized_after.clone(),
            added_lines: request.added_lines,
            removed_lines: request.removed_lines,
        })
    }

    /// Attempt one approved apply. Always typed `unavailable` at this
    /// stage: the prepared artifact is never consumed and nothing is
    /// ever written, checkpointed, or launched.
    pub fn apply(
        &mut self,
        _prepared: &PreparedGodotMutation,
        _approved_digest: &str,
    ) -> GodotMutationApplyOutcome {
        GodotMutationApplyOutcome::Unavailable {
            message: GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE, GodotMutationApplyOutcome,
        GodotSceneMutationPrepareRequest, GodotSceneMutationService,
    };
    use siralos_godot::godot::scene::models::GodotVariantValue;
    use siralos_godot::godot::scene_mutation::{
        MutationKind, MutationOperation, compute_mutation_fingerprint,
    };

    fn request() -> GodotSceneMutationPrepareRequest {
        GodotSceneMutationPrepareRequest {
            target_path: "res://scenes/player.tscn".to_owned(),
            source_revision: format!("rev_{}", "a".repeat(32)),
            source_sha256: "b".repeat(64),
            kind: MutationKind::Scene,
            operations: vec![MutationOperation::SetProperty {
                node_path: Some("Root/Button".to_owned()),
                property: "text".to_owned(),
                value: GodotVariantValue::String("Play".to_owned()),
            }],
            preview: crate::godot::scene_mutation::GodotMutationPreview {
                structural_summary: "set Root/Button.text".to_owned(),
                diff: "--- a\n+++ b\n".to_owned(),
            },
            serialized_after: "[gd_scene]\n".to_owned(),
            added_lines: 1,
            removed_lines: 1,
        }
    }

    #[test]
    fn prepare_delegates_core_validation_verbatim() {
        let mut invalid = request();
        invalid.operations = vec![MutationOperation::RemoveNode {
            node_path: "Root/../Old".to_owned(),
        }];
        let error = GodotSceneMutationService::new()
            .prepare(&invalid)
            .expect_err("traversal rejected");
        assert_eq!(error.message, "Invalid node path: Root/../Old");
    }

    #[test]
    fn prepare_binds_the_core_fingerprint_and_apply_stays_unavailable() {
        let mut service = GodotSceneMutationService::new();
        let request = request();
        let prepared = service.prepare(&request).expect("prepared");
        assert_eq!(
            prepared.fingerprint,
            compute_mutation_fingerprint(
                &request.target_path,
                &request.source_revision,
                &request.source_sha256,
                request.kind,
                &request.operations,
                &request.serialized_after,
            )
        );
        let outcome = service.apply(&prepared, &prepared.fingerprint);
        assert_eq!(
            outcome,
            GodotMutationApplyOutcome::Unavailable {
                message: GODOT_MUTATION_APPLY_UNAVAILABLE_MESSAGE.to_owned()
            }
        );
    }

    #[test]
    fn identical_requests_bind_identical_identities() {
        let service = GodotSceneMutationService::new();
        let first = service.prepare(&request()).expect("prepared");
        let second = service.prepare(&request()).expect("prepared");
        assert_eq!(first.fingerprint, second.fingerprint);
    }
}
