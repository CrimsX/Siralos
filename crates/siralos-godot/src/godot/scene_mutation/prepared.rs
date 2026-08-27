//! Prepared mutation artifact (Stage 3 milestone 10, ADR 0026; R9
//! parity slice).
//!
//! Mirrors `packages/core/src/godot/scene-mutation/prepared.ts`. Every
//! native mutation becomes an immutable prepared artifact binding: the
//! exact target revision, the exact operation set, the complete preview,
//! the expected semantic effect, and a deterministic fingerprint.
//! Approval binds the fingerprint; changing the target, revision,
//! operations, or prepared output produces a new identity and
//! invalidates any old approval. A prepared mutation is NOT approval and
//! NOT an apply â€” at this stage apply stays typed `unavailable`
//! (`SECURITY.md`).

use crate::godot::scene_mutation::{
    MutationError, MutationOperation, expected_semantic_effect,
};
use serde_json::json;
use siralos_core::identity::{canonicalize_json, sha256_hex_str};

/// Complete reviewable preview of one prepared mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GodotMutationPreview {
    /// Bounded structural summary of the operations (reviewable text).
    pub structural_summary: String,
    /// Complete unified diff of the serialized before/after document.
    pub diff: String,
}

/// Target document kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// `.tscn` scene document.
    Scene,
    /// `.tres` resource document.
    Resource,
}

impl MutationKind {
    /// Canonical protocol string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Resource => "resource",
        }
    }
}

/// Immutable prepared mutation binding target, revision, operations,
/// expectations, preview, and deterministic fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedGodotMutation {
    /// Target document path (workspace-relative).
    pub target_path: String,
    /// Exact source revision handle (`rev_â€¦`).
    pub source_revision: String,
    /// SHA-256 of the exact source text (apply precondition).
    pub source_sha256: String,
    /// Document kind.
    pub kind: MutationKind,
    /// Validated operation set.
    pub operations: Vec<MutationOperation>,
    /// Expected post-apply semantic effect.
    pub expected_semantic_effect: Vec<super::SemanticExpectation>,
    /// Complete preview.
    pub preview: GodotMutationPreview,
    /// Deterministic identity; approval binds this exact value.
    pub fingerprint: String,
    /// The deterministic serialized after-text (apply content).
    pub serialized_after: String,
    /// Preview line count added (checkpoint metadata).
    pub added_lines: u64,
    /// Preview line count removed (checkpoint metadata).
    pub removed_lines: u64,
}

/// Inputs for [`create_prepared_godot_mutation`].
pub struct CreatePreparedGodotMutationInput {
    /// Target document path.
    pub target_path: String,
    /// Exact source revision handle.
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

/// Deterministic identity over the exact prepared mutation content.
/// Mirrors the oracle's `computeMutationFingerprint`: canonical JSON
/// with sorted keys over the fixed field tuple, SHA-256 hex.
#[must_use]
pub fn compute_mutation_fingerprint(
    target_path: &str,
    source_revision: &str,
    source_sha256: &str,
    kind: MutationKind,
    operations: &[MutationOperation],
    serialized_after: &str,
) -> String {
    let canonical = canonicalize_json(&json!({
        "targetPath": target_path,
        "sourceRevision": source_revision,
        "sourceSha256": source_sha256,
        "kind": kind.as_str(),
        "operations": operations
            .iter()
            .map(MutationOperation::to_canonical_json)
            .collect::<Vec<_>>(),
        "serializedAfter": serialized_after,
    }));
    sha256_hex_str(&canonical)
}

fn require_bounded(
    text: &str,
    max_bytes: usize,
    field: &str,
) -> Result<String, MutationError> {
    let value = text.trim();
    if value.is_empty() {
        return Err(MutationError {
            message: format!("{field} must not be empty."),
        });
    }
    if value.len() > max_bytes {
        return Err(MutationError {
            message: format!("{field} exceeds {max_bytes} UTF-8 bytes."),
        });
    }
    Ok(value.to_owned())
}

fn is_revision_handle(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && value.starts_with("rev_")
        && bytes[4..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_sha256_hex(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Create the immutable prepared mutation. Host-owned identity: the
/// fingerprint is computed over the exact content so any material change
/// (revision, operations, target, serialized output) yields a new
/// identity that old approvals cannot satisfy.
pub fn create_prepared_godot_mutation(
    input: CreatePreparedGodotMutationInput,
) -> Result<PreparedGodotMutation, MutationError> {
    let target_path =
        require_bounded(&input.target_path, 1024, "A target path")?;
    if !is_revision_handle(&input.source_revision) {
        return Err(MutationError {
            message:
                "A prepared mutation requires an exact source revision handle."
                    .to_owned(),
        });
    }
    if !is_sha256_hex(&input.source_sha256) {
        return Err(MutationError {
            message: "A prepared mutation requires a 64-hex source SHA-256."
                .to_owned(),
        });
    }
    if input.operations.is_empty() {
        return Err(MutationError {
            message: "A prepared mutation requires at least one operation."
                .to_owned(),
        });
    }
    let structural_summary = require_bounded(
        &input.preview.structural_summary,
        8 * 1024,
        "A preview summary",
    )?;
    let diff =
        require_bounded(&input.preview.diff, 64 * 1024, "A preview diff")?;
    let fingerprint = compute_mutation_fingerprint(
        &target_path,
        &input.source_revision,
        &input.source_sha256,
        input.kind,
        &input.operations,
        &input.serialized_after,
    );
    let expected_semantic_effect = expected_semantic_effect(&input.operations);
    Ok(PreparedGodotMutation {
        target_path,
        source_revision: input.source_revision,
        source_sha256: input.source_sha256,
        kind: input.kind,
        operations: input.operations,
        expected_semantic_effect,
        preview: GodotMutationPreview { structural_summary, diff },
        fingerprint,
        serialized_after: input.serialized_after,
        added_lines: input.added_lines,
        removed_lines: input.removed_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CreatePreparedGodotMutationInput, GodotMutationPreview, MutationError,
        MutationKind, compute_mutation_fingerprint,
        create_prepared_godot_mutation,
    };
    use crate::godot::scene::models::GodotVariantValue;
    use crate::godot::scene_mutation::MutationOperation;

    fn revision() -> String {
        format!("rev_{}", "a".repeat(32))
    }

    fn sha() -> String {
        "b".repeat(64)
    }

    fn operation() -> MutationOperation {
        MutationOperation::SetProperty {
            node_path: Some("Root/Button".to_owned()),
            property: "text".to_owned(),
            value: GodotVariantValue::String("Play".to_owned()),
        }
    }

    fn input() -> CreatePreparedGodotMutationInput {
        CreatePreparedGodotMutationInput {
            target_path: "res://scenes/player.tscn".to_owned(),
            source_revision: revision(),
            source_sha256: sha(),
            kind: MutationKind::Scene,
            operations: vec![operation()],
            preview: GodotMutationPreview {
                structural_summary: "set Root/Button.text".to_owned(),
                diff: "--- a\n+++ b\n".to_owned(),
            },
            serialized_after: "[gd_scene]\n".to_owned(),
            added_lines: 1,
            removed_lines: 1,
        }
    }

    #[test]
    fn fingerprint_is_deterministic_and_content_sensitive() {
        let prepared =
            create_prepared_godot_mutation(input()).expect("prepared");
        let again = create_prepared_godot_mutation(input()).expect("prepared");
        assert_eq!(prepared.fingerprint, again.fingerprint);
        assert_eq!(prepared.expected_semantic_effect.len(), 1);

        let mut changed = input();
        changed.serialized_after = "[gd_scene load_steps=2]\n".to_owned();
        let other = create_prepared_godot_mutation(changed).expect("prepared");
        assert_ne!(prepared.fingerprint, other.fingerprint);
    }

    #[test]
    fn identity_fields_are_validated_with_oracle_messages() {
        let mut bad = input();
        bad.source_revision = "rev_zzz".to_owned();
        assert_eq!(
            create_prepared_godot_mutation(bad),
            Err(MutationError {
                message: "A prepared mutation requires an exact source revision handle."
                    .to_owned()
            })
        );
        let mut bad = input();
        bad.source_sha256 = "not-a-digest".to_owned();
        assert_eq!(
            create_prepared_godot_mutation(bad),
            Err(MutationError {
                message:
                    "A prepared mutation requires a 64-hex source SHA-256."
                        .to_owned()
            })
        );
        let mut bad = input();
        bad.operations = Vec::new();
        assert_eq!(
            create_prepared_godot_mutation(bad),
            Err(MutationError {
                message:
                    "A prepared mutation requires at least one operation."
                        .to_owned()
            })
        );
        let mut bad = input();
        bad.preview.diff = String::new();
        assert_eq!(
            create_prepared_godot_mutation(bad),
            Err(MutationError {
                message: "A preview diff must not be empty.".to_owned()
            })
        );
    }

    #[test]
    fn fingerprint_changes_when_operations_change() {
        let mut changed = input();
        changed.operations = vec![MutationOperation::SetProperty {
            node_path: Some("Root/Button".to_owned()),
            property: "text".to_owned(),
            value: GodotVariantValue::String("Pause".to_owned()),
        }];
        let base = create_prepared_godot_mutation(input()).expect("prepared");
        let other = create_prepared_godot_mutation(changed).expect("prepared");
        assert_ne!(base.fingerprint, other.fingerprint);
        let recomputed = compute_mutation_fingerprint(
            &other.target_path,
            &other.source_revision,
            &other.source_sha256,
            other.kind,
            &other.operations,
            &other.serialized_after,
        );
        assert_eq!(recomputed, other.fingerprint);
    }
}
