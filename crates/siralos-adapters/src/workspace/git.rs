//! Typed unavailable Git inspection (R4, ADR 0006).
//!
//! Git inspection may only execute through an enforcing sandbox
//! boundary (host-read allowlist, read-only workspace, network
//! denial, process-tree confinement). The Rust candidate has no such
//! boundary yet, so inspection reports typed unavailable and Git is
//! never spawned; the security boundary is never weakened to make
//! Git work. Git is optional integration, never transaction
//! authority, and it never broadens the workspace root.

use siralos_core::workspace::git::{GitErrorCode, GitInspectionDisposition};

/// The typed Git inspection disposition of the Rust candidate at R4.
/// Always `Unavailable` until an enforcing process boundary exists;
/// the stable reason class is machine-branchable and the message is
/// truthful about the boundary, never a claim of availability.
pub fn git_inspection_disposition() -> GitInspectionDisposition {
    GitInspectionDisposition::Unavailable {
        code: GitErrorCode::GitUnavailable,
        reason: "no enforcing process boundary in the Rust candidate",
    }
}
#[cfg(test)]
mod tests {
    use super::git_inspection_disposition;
    use siralos_core::workspace::git::{
        GitErrorCode, GitInspectionDisposition,
    };

    #[test]
    fn git_inspection_is_unavailable_and_typed() {
        let disposition = git_inspection_disposition();
        assert!(disposition.is_unavailable());
        assert_eq!(
            disposition,
            GitInspectionDisposition::Unavailable {
                code: GitErrorCode::GitUnavailable,
                reason: "no enforcing process boundary in the Rust candidate",
            },
        );
    }
}
