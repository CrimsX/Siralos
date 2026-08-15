//! Prepared workspace effects (R4, ADR 0005).
//!
//! The generic prepared-effect contract without model authority:
//! inspect exact revision -> prepare exact proposed change -> complete
//! deterministic preview -> bind proposal identity -> revalidate any
//! future precondition before commit. A prepared effect is typed,
//! immutable after creation, bound to exact pre-state identity and
//! exact proposed post-state content, and single-use. Prepared !=
//! approved and prepared != applied; the model proposes, the Host
//! disposes. The reference currently reports every provider-accessible
//! mutation effect as `unavailable` (no identity-bound commit
//! primitive), so the executable boundary below carries the same
//! fail-closed dispositions and performs no filesystem mutation.

/// The typed prepared-mutation kind (protocol vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedMutationKind {
    /// Create proposal.
    Create,
    /// Edit proposal.
    Edit,
    /// Delete proposal.
    Delete,
}

impl PreparedMutationKind {
    /// The canonical protocol string for this kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Edit => "edit",
            Self::Delete => "delete",
        }
    }
}

/// The generic mutation operation of a prepared effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOperation {
    /// Create one UTF-8 text file (absent -> present).
    Create,
    /// Exact-replacement edit of one UTF-8 text file (present -> present).
    Edit,
    /// Delete one UTF-8 text file (present -> absent).
    Delete,
}

impl MutationOperation {
    /// The canonical protocol string for this operation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Edit => "edit",
            Self::Delete => "delete",
        }
    }
}

/// One exact sequential text replacement (edit contract: each old text
/// must match exactly once; no regex, no replace-all, no fuzzy edit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextReplacement {
    /// Exact text to find (must occur exactly once at application).
    pub old_text: String,
    /// Exact replacement text.
    pub new_text: String,
}

/// Prepared create effect (typed, immutable proposal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCreateFile {
    /// Workspace-relative target path.
    pub path: String,
    /// Complete proposed UTF-8 text content (bounded).
    pub content: String,
}

/// Prepared exact-replacement edit effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEditFile {
    /// Workspace-relative target path.
    pub path: String,
    /// Exact pre-state SHA-256 (stale-write precondition).
    pub expected_sha256: String,
    /// Ordered exact replacements (bounded count and bytes).
    pub replacements: Vec<TextReplacement>,
}

/// Prepared delete effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDeleteFile {
    /// Workspace-relative target path.
    pub path: String,
    /// Exact pre-state SHA-256 (stale-write precondition).
    pub expected_sha256: String,
}

/// The typed prepared mutation proposal (opaque single-use handle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMutation {
    /// The proposed operation kind.
    pub kind: PreparedMutationKind,
    /// Workspace-relative target path.
    pub path: String,
    /// Exact pre-state content identity (whole-file SHA-256) when the
    /// operation requires an existing file.
    pub expected_sha256: Option<String>,
    /// Exact proposed post-state content identity.
    pub after_sha256: Option<String>,
    /// The immutable proposal; none of these fields grant authority.
    pub payload: PreparedMutationPayload,
}

/// The immutable proposal payload of a prepared mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedMutationPayload {
    /// Create proposal content.
    Create(PreparedCreateFile),
    /// Edit proposal replacements.
    Edit(PreparedEditFile),
    /// Delete proposal (no payload).
    Delete(PreparedDeleteFile),
}

/// The prepared-effect lifecycle disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedMutationState {
    /// Prepared: the immutable proposal exists (never approval, never
    /// application).
    Prepared,
    /// Applied once (a future identity-bound commit primitive).
    Applied,
    /// Expired or consumed without application.
    Expired,
}

#[cfg(test)]
mod tests {
    use super::{
        MutationOperation, PreparedCreateFile, PreparedDeleteFile,
        PreparedEditFile, PreparedMutation, PreparedMutationKind,
        PreparedMutationPayload, TextReplacement,
    };

    #[test]
    fn prepared_mutation_binds_exact_before_and_after_identity() {
        let create = PreparedMutation {
            kind: PreparedMutationKind::Create,
            path: "src/new.gd".to_owned(),
            expected_sha256: None,
            after_sha256: Some("ab12".to_owned()),
            payload: PreparedMutationPayload::Create(PreparedCreateFile {
                path: "src/new.gd".to_owned(),
                content: "extends Node\n".to_owned(),
            }),
        };
        assert_eq!(create.kind.as_str(), "create");
        assert!(create.expected_sha256.is_none());
    }

    #[test]
    fn edit_proposal_keeps_ordered_exact_replacements() {
        let edit = PreparedEditFile {
            path: "src/main.gd".to_owned(),
            expected_sha256: "before-hash".to_owned(),
            replacements: vec![TextReplacement {
                old_text: "move_and_slide()".to_owned(),
                new_text: "move_and_slide(Vector2.UP)".to_owned(),
            }],
        };
        assert_eq!(edit.replacements.len(), 1);
        assert_eq!(MutationOperation::Edit.as_str(), "edit");
        let delete = PreparedDeleteFile {
            path: "src/old.gd".to_owned(),
            expected_sha256: "before-hash".to_owned(),
        };
        assert_eq!(delete.expected_sha256, "before-hash");
    }
}
