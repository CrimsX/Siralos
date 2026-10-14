//! One-time digest-bound per-action approvals (ADR 0038 D4).
//!
//! The permission gate ([`PermissionRule::Ask`]) declares that a
//! capability needs approval; this module is the reviewable
//! preparation protocol the application gate consumes. An
//! [`ActionApproval`] binds one exact tool call: the tool name, the
//! canonical input bytes, and the task revision, hashed into
//! [`action_digest`]. The host-owned [`ActionApprovals`] ledger holds
//! unconsumed approvals; the executor removes the matching record
//! before execution, so every approval authorizes at most one call.
//! A mutated input, a stale task revision, or a replayed call finds
//! no record and is denied without execution.
//!
//! The ledger is host-owned state shared with the per-round executor
//! through [`SharedApprovals`]. The round runner owns its executor by
//! value across pull steps, so a borrow cannot span the round; the
//! single-threaded pull machine shares the ledger instead. No thread,
//! lock, or cross-thread synchronization is introduced.
//!
//! Core never decides what deserves approval: records enter the
//! ledger only through explicit host [`ActionApprovals::approve`]
//! calls. An empty ledger denies everything, exactly like the
//! previous ask path.
//!
//! [`PermissionRule::Ask`]: crate::tool::permission::PermissionRule

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde_json::Value;

use crate::identity::sha256_hex;

/// One validated 64-character hexadecimal SHA-256 action digest.
///
/// Validity is established once, in [`ActionDigest::parse`]: holders
/// of this type never re-check length or hex shape at call sites.
/// The digest binds one exact tool call (tool name, canonical input,
/// task revision); see [`action_digest`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ActionDigest(String);

impl ActionDigest {
    /// Validate one digest at the host boundary.
    ///
    /// Anything that is not 64 hexadecimal characters is a host
    /// programming defect and fails here, never at execution time.
    pub fn parse(value: &str) -> Result<Self, ApprovalError> {
        if value.len() != 64
            || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ApprovalError::InvalidDigest);
        }
        Ok(Self(value.to_owned()))
    }

    /// The canonical digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One host-issued per-action approval.
///
/// The record binds the exact tool name, the exact action digest
/// ([`action_digest`] over the canonical input and task revision),
/// and the task revision it was issued for. Records are opaque to
/// the model: the provider never sees them and cannot forge one,
/// because matching requires the host-owned ledger entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionApproval {
    /// Exact tool name this approval authorizes.
    tool_name: String,
    /// Expected [`action_digest`] for the approved call.
    action_digest: ActionDigest,
    /// Task revision the approval was issued for.
    task_revision: Option<u64>,
}

impl ActionApproval {
    /// Build one validated approval record.
    ///
    /// The tool name must be non-empty; anything else is a host
    /// programming defect and fails here, never at execution time.
    /// The digest arrives already validated as an [`ActionDigest`].
    pub fn new(
        tool_name: &str,
        action_digest: ActionDigest,
        task_revision: Option<u64>,
    ) -> Result<Self, ApprovalError> {
        if tool_name.is_empty() {
            return Err(ApprovalError::EmptyTool);
        }
        Ok(Self {
            tool_name: tool_name.to_owned(),
            action_digest,
            task_revision,
        })
    }

    /// The exact tool name this approval authorizes.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// The expected action digest for the approved call.
    pub fn action_digest(&self) -> &ActionDigest {
        &self.action_digest
    }

    /// The task revision the approval was issued for.
    pub fn task_revision(&self) -> Option<u64> {
        self.task_revision
    }
}

/// Why a host-issued approval record was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalError {
    /// The approval tool name is empty.
    EmptyTool,
    /// The approval digest is not 64 hexadecimal characters.
    InvalidDigest,
}

impl ApprovalError {
    /// The exact externally observable rejection message.
    pub fn message(self) -> &'static str {
        match self {
            Self::EmptyTool => "approval tool name is empty",
            Self::InvalidDigest => {
                "approval digest is not a 64-character hexadecimal \
                 SHA-256"
            }
        }
    }
}

impl std::fmt::Display for ApprovalError {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ApprovalError {}

/// Compute the digest one approval binds.
///
/// The preimage is the exact tool name, the task revision
/// (`none` when the machine carries none), and the canonical
/// input JSON ([`canonical_input_json`]), newline-joined. Any
/// byte difference in the model input or any revision change
/// produces a different digest, so the approval cannot authorize
/// changed content (RUST_STYLE anti-absolutism: approval cannot
/// authorize changed content).
pub fn action_digest(
    tool_name: &str,
    canonical_input: &str,
    task_revision: Option<u64>,
) -> ActionDigest {
    let revision = match task_revision {
        Some(revision) => revision.to_string(),
        None => "none".to_owned(),
    };
    let hex = sha256_hex(
        format!("{tool_name}\n{revision}\n{canonical_input}").as_bytes(),
    );
    ActionDigest::parse(&hex)
        .expect("sha256_hex always yields 64 lowercase hex characters")
}

/// Canonical JSON text for approval binding.
///
/// `serde_json` maps order keys deterministically (no
/// `preserve_order` in this workspace), so serializing the
/// detached input value yields byte-stable text for equal
/// inputs regardless of provider key order.
pub fn canonical_input_json(input: &Value) -> String {
    serde_json::to_string(input)
        .expect("serde_json::Value is always serializable")
}

/// Host-owned ledger of unconsumed per-action approvals.
///
/// Keyed by action digest; a later approval for the same digest
/// replaces the earlier one. Consumption removes the record, so
/// replaying an approved call finds nothing and is denied.
#[derive(Debug, Clone, Default)]
pub struct ActionApprovals {
    approvals: BTreeMap<ActionDigest, ActionApproval>,
}

impl ActionApprovals {
    /// An empty ledger (denies every approval-gated call).
    pub fn new() -> Self {
        Self::default()
    }

    /// Share this ledger with per-round executors.
    ///
    /// Clones of the shared handle observe the same records: a
    /// consumption through any clone is visible to all holders.
    pub fn into_shared(self) -> SharedApprovals {
        SharedApprovals(Rc::new(RefCell::new(self)))
    }

    /// Record one validated approval.
    pub fn approve(&mut self, approval: ActionApproval) {
        self.approvals.insert(approval.action_digest.clone(), approval);
    }

    /// Remove and return the approval matching one exact call.
    ///
    /// Returns `None` (leaving the ledger untouched) when no
    /// record carries the digest, the tool name differs, or the
    /// task revision differs. A returned record is already
    /// removed: the same approval can never authorize twice.
    pub fn take(
        &mut self,
        tool_name: &str,
        action_digest: &ActionDigest,
        task_revision: Option<u64>,
    ) -> Option<ActionApproval> {
        let approval = self.approvals.get(action_digest)?;
        if approval.tool_name != tool_name
            || approval.task_revision != task_revision
        {
            return None;
        }
        self.approvals.remove(action_digest)
    }

    /// The number of unconsumed approvals.
    pub fn len(&self) -> usize {
        self.approvals.len()
    }

    /// Whether no unconsumed approval remains.
    pub fn is_empty(&self) -> bool {
        self.approvals.is_empty()
    }
}

/// Ledger handle shared between the application and its
/// per-round executors (see the module documentation for why
/// the handle is shared rather than borrowed).
#[derive(Debug, Clone)]
pub struct SharedApprovals(Rc<RefCell<ActionApprovals>>);

impl SharedApprovals {
    /// An empty shared ledger.
    pub fn new() -> Self {
        ActionApprovals::new().into_shared()
    }

    /// Record one validated approval through the shared handle.
    pub fn approve(&self, approval: ActionApproval) {
        self.0.borrow_mut().approve(approval);
    }

    /// Remove and return the approval matching one exact call
    /// (see [`ActionApprovals::take`]).
    pub fn take(
        &self,
        tool_name: &str,
        action_digest: &ActionDigest,
        task_revision: Option<u64>,
    ) -> Option<ActionApproval> {
        self.0.borrow_mut().take(tool_name, action_digest, task_revision)
    }

    /// The number of unconsumed approvals.
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    /// Whether no unconsumed approval remains.
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
}

impl Default for SharedApprovals {
    fn default() -> Self {
        Self::new()
    }
}

/// Reviewable preview of one proposed action, built read-only by
/// the Tool itself.
///
/// The preview is advisory, never a promise: the tool re-verifies
/// everything at execution, so content that shifts between preview
/// and execution refuses there. Preview lines may carry
/// model-proposed text, so frontends sanitize them at the single
/// output boundary before display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPreview {
    /// One-line framing (tool name and target path).
    pub title: String,
    /// Detail lines (digests, sizes, diffs, match counts), bounded
    /// by [`MAX_APPROVAL_PREVIEW_LINES`] at every frontend render.
    pub lines: Vec<String>,
}

/// Maximum detail lines one approval render shows, before the
/// truncation marker. Bounds the terminal output of a mistaken or
/// hostile preview; fits the TUI modal line cap with room for its
/// framing.
pub const MAX_APPROVAL_PREVIEW_LINES: usize = 24;

/// Maximum characters in one approval preview line, before the
/// tool's truncation marker. One shared bound so the mutation and
/// execution previews cannot drift apart; each tool keeps its own
/// unit-accurate marker wording (lines vs characters).
pub const MAX_APPROVAL_PREVIEW_LINE_CHARS: usize = 160;

/// One synchronous approval request for a proposed action.
///
/// The executor computes the action digest itself and never hands
/// it out: the frontend sees the human-readable preview and returns
/// a verdict, and only an approve verdict mints the ledger approval
/// for the exact computed digest. The frontend can therefore neither
/// forge nor broaden an approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// Exact tool name of the proposed call.
    pub tool_name: String,
    /// Task revision the machine carries.
    pub task_revision: Option<u64>,
    /// Read-only preview built by the Tool.
    pub preview: ApprovalPreview,
}

/// Verdict on one approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalVerdict {
    /// Record a one-time approval for the exact requested action
    /// and proceed with this dispatch.
    Approve,
    /// Deny without execution.
    Deny,
}

/// Shared resolver behavior: prompt synchronously for one request,
/// return its verdict.
type ResolverFn = dyn FnMut(&ApprovalRequest) -> ApprovalVerdict;

/// Host-owned synchronous approval resolver shared with per-round
/// executors.
///
/// The resolver runs inside the dispatch it authorizes, so the same
/// call proceeds under a freshly recorded approval with no second
/// prompt and no re-proposal. Sharing follows the ledger's
/// justification: the round runner owns its executor by value
/// across pull steps, so a borrow cannot span the round; the handle
/// is single-threaded and never crosses threads.
#[derive(Clone)]
pub struct SharedApprovalResolver(Rc<RefCell<ResolverFn>>);

impl SharedApprovalResolver {
    /// Build one resolver from the frontend's prompt behavior.
    pub fn new(
        resolve: impl FnMut(&ApprovalRequest) -> ApprovalVerdict + 'static,
    ) -> Self {
        Self(Rc::new(RefCell::new(resolve)))
    }

    /// Resolve one request (panics only on reentrant resolution,
    /// which the single-threaded pull machine never performs: the
    /// resolver never runs while itself is running).
    pub fn resolve(&self, request: &ApprovalRequest) -> ApprovalVerdict {
        (self.0.borrow_mut())(request)
    }
}

impl std::fmt::Debug for SharedApprovalResolver {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str("SharedApprovalResolver(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActionApproval, ActionApprovals, ActionDigest, action_digest,
        canonical_input_json,
    };
    use serde_json::json;

    fn digest_for(tool: &str, input: &serde_json::Value) -> ActionDigest {
        action_digest(tool, &canonical_input_json(input), Some(7))
    }

    fn parsed(value: &str) -> ActionDigest {
        ActionDigest::parse(value).expect("test digest is valid")
    }

    #[test]
    fn digest_parse_accepts_only_64_hex_characters() {
        let valid = "a".repeat(64);
        assert_eq!(parsed(&valid).as_str(), valid.as_str());
        for digest in ["", "abc", &"g".repeat(64), &"a".repeat(63)] {
            assert_eq!(
                ActionDigest::parse(digest).unwrap_err(),
                super::ApprovalError::InvalidDigest,
                "{digest}"
            );
        }
        // Acceptance is unchanged by the newtype: uppercase hex
        // validates (it simply never matches a computed digest).
        ActionDigest::parse(&"A".repeat(64)).expect("uppercase hex valid");
    }

    #[test]
    fn rejects_empty_tool_names() {
        assert_eq!(
            ActionApproval::new("", parsed(&"a".repeat(64)), None)
                .unwrap_err(),
            super::ApprovalError::EmptyTool
        );
    }

    #[test]
    fn digest_is_stable_and_binds_tool_input_and_revision() {
        let input = json!({"path": "a.txt", "content": "x"});
        let reordered = serde_json::from_str::<serde_json::Value>(
            r#"{"content":"x","path":"a.txt"}"#,
        )
        .unwrap();
        assert_eq!(
            canonical_input_json(&input),
            canonical_input_json(&reordered)
        );
        let digest = digest_for("write", &input);
        assert_eq!(digest, digest_for("write", &reordered));
        assert_ne!(
            digest,
            action_digest("write", &canonical_input_json(&input), Some(8))
        );
        assert_ne!(
            digest,
            action_digest("edit", &canonical_input_json(&input), Some(7))
        );
        assert_ne!(
            digest,
            digest_for("write", &json!({"path": "a.txt", "content": "y"}))
        );
    }

    #[test]
    fn take_consumes_exactly_once_and_checks_tool_and_revision() {
        let input = json!({"path": "a.txt"});
        let digest = digest_for("write", &input);
        let mut ledger = ActionApprovals::new();
        ledger.approve(
            ActionApproval::new("write", digest.clone(), Some(7)).unwrap(),
        );
        assert_eq!(ledger.len(), 1);
        assert!(ledger.take("edit", &digest, Some(7)).is_none());
        assert!(ledger.take("write", &digest, Some(8)).is_none());
        assert!(ledger.take("write", &digest, None).is_none());
        assert_eq!(ledger.len(), 1);
        let approval = ledger.take("write", &digest, Some(7)).expect("match");
        assert_eq!(approval.tool_name(), "write");
        assert!(ledger.is_empty());
        assert!(ledger.take("write", &digest, Some(7)).is_none());
    }

    #[test]
    fn unknown_digests_leave_the_ledger_untouched() {
        let mut ledger = ActionApprovals::new();
        let digest = digest_for("write", &json!({}));
        ledger.approve(ActionApproval::new("write", digest, Some(7)).unwrap());
        assert!(
            ledger.take("write", &parsed(&"b".repeat(64)), Some(7)).is_none()
        );
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn shared_handles_observe_consumption_across_clones() {
        let input = json!({"path": "a.txt"});
        let digest = digest_for("write", &input);
        let shared = ActionApprovals::new().into_shared();
        shared.approve(
            ActionApproval::new("write", digest.clone(), Some(7)).unwrap(),
        );
        let clone = shared.clone();
        assert!(clone.take("write", &digest, Some(7)).is_some());
        assert!(shared.is_empty());
        assert!(shared.take("write", &digest, Some(7)).is_none());
    }
}
