//! Read-only Git inspection contracts (R4, ADR 0006).
//!
//! Git is optional integration, never transactional authority:
//! non-Git workspaces stay fully valid, Git never broadens the
//! workspace root, and no Git mutation surface exists (add, commit,
//! reset, restore, checkout, clean, stash, branch, worktree, remote,
//! hooks, config mutation are all absent). Git processes may only
//! execute inside an enforcing sandbox boundary; until the Rust
//! candidate has that boundary, inspection reports typed unavailable
//! and Git is never spawned. Core carries only the typed result/error
//! contract; process execution belongs to adapters behind the
//! enforcing boundary.

/// Canonical Git inspection error codes (reference `GitError` codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitErrorCode {
    /// Git inspection is unavailable on this runtime/boundary.
    GitUnavailable,
    /// The workspace is not inside a Git repository.
    GitNotRepository,
    /// The repository root differs from the Siralos workspace root.
    GitRootMismatch,
    /// `git status` failed.
    GitStatusFailed,
    /// `git diff` failed.
    GitDiffFailed,
    /// The inspection was cancelled.
    GitCancelled,
    /// The inspection exceeded its deadline.
    GitTimeout,
    /// Machine-oriented Git output could not be parsed.
    GitParseFailed,
}

impl GitErrorCode {
    /// The canonical protocol string for this code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitUnavailable => "git_unavailable",
            Self::GitNotRepository => "git_not_repository",
            Self::GitRootMismatch => "git_root_mismatch",
            Self::GitStatusFailed => "git_status_failed",
            Self::GitDiffFailed => "git_diff_failed",
            Self::GitCancelled => "git_cancelled",
            Self::GitTimeout => "git_timeout",
            Self::GitParseFailed => "git_parse_failed",
        }
    }
}

/// Typed disposition of Git inspection on this runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitInspectionDisposition {
    /// Inspection is unavailable; the stable code says why class.
    Unavailable {
        /// The canonical unavailable code.
        code: GitErrorCode,
        /// Stable machine-branchable reason class.
        reason: &'static str,
    },
}

impl GitInspectionDisposition {
    /// True when inspection is unavailable (fail-closed).
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
}
#[cfg(test)]
mod tests {
    use super::{GitErrorCode, GitInspectionDisposition};

    #[test]
    fn unavailable_disposition_is_typed_and_fail_closed() {
        let disposition = GitInspectionDisposition::Unavailable {
            code: GitErrorCode::GitUnavailable,
            reason: "no enforcing process boundary",
        };
        assert!(disposition.is_unavailable());
        assert_eq!(disposition, disposition.clone());
    }

    #[test]
    fn codes_round_trip_through_canonical_strings() {
        assert_eq!(GitErrorCode::GitUnavailable.as_str(), "git_unavailable");
        assert_eq!(
            GitErrorCode::GitNotRepository.as_str(),
            "git_not_repository"
        );
        assert_eq!(
            GitErrorCode::GitRootMismatch.as_str(),
            "git_root_mismatch"
        );
        assert_eq!(
            GitErrorCode::GitStatusFailed.as_str(),
            "git_status_failed"
        );
        assert_eq!(GitErrorCode::GitDiffFailed.as_str(), "git_diff_failed");
        assert_eq!(GitErrorCode::GitCancelled.as_str(), "git_cancelled");
        assert_eq!(GitErrorCode::GitTimeout.as_str(), "git_timeout");
        assert_eq!(GitErrorCode::GitParseFailed.as_str(), "git_parse_failed");
    }
}
