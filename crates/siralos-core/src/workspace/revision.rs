//! Workspace revision handles and the session-scoped registry (R4,
//! ADR 0016).
//!
//! An opaque model-facing revision handle identifies one exact
//! cryptographic file state: `rev_<32 hex>`. The handle is ergonomic
//! identity, never authority: possession grants no read/write access,
//! approval, path access, or sandbox bypass. The authoritative identity
//! is the whole-file SHA-256, and every future mutation revalidates the
//! current file against the SHA-256 the handle resolves to. Handles are
//! workspace-scoped: the same relative path in a different workspace
//! never resolves. The registry is session-scoped, in-memory, bounded,
//! and deterministic (no durable storage, no randomness, no clock).

use crate::identity::{CanonicalValue, canonicalize, sha256_hex};

use std::collections::BTreeMap;

/// Opaque revision handle prefix (`rev_`).
pub const WORKSPACE_REVISION_HANDLE_PREFIX: &str = "rev_";

/// Default registry entry limit (reference default).
pub const DEFAULT_REVISION_REGISTRY_LIMIT: usize = 1024;

/// Read mode recorded by an observed read (protocol compatibility;
/// structural/summary modes are R5-owned language surfaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedReadMode {
    /// Authoritative exact source read.
    Exact,
    /// Structural read (R5 language intelligence; mode value only).
    Structural,
    /// Summary read (R5 language intelligence; mode value only).
    Summary,
}

impl ObservedReadMode {
    /// The canonical protocol string for this mode.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Structural => "structural",
            Self::Summary => "summary",
        }
    }

    /// Parse a protocol mode string (unknown values are rejected).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "exact" => Some(Self::Exact),
            "structural" => Some(Self::Structural),
            "summary" => Some(Self::Summary),
            _ => None,
        }
    }
}

/// The full identity bound to one revision handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRevisionIdentity {
    /// Fingerprint of the owning workspace.
    pub workspace_fingerprint: String,
    /// Workspace-relative path of the file.
    pub path: String,
    /// Exact whole-file SHA-256 of the observed state.
    pub sha256: String,
}

/// One recorded observed read (bounded session-local evidence only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedWorkspaceRead {
    /// Workspace-relative path that was read.
    pub path: String,
    /// The revision handle observed.
    pub revision: String,
    /// The read mode.
    pub mode: ObservedReadMode,
    /// Deterministic observation sequence number (host counter, never a
    /// wall clock).
    pub at_ms: u64,
}

/// Deterministic handle for one exact file state in one workspace,
/// byte-identical to the reference `computeWorkspaceRevisionHandle`:
/// SHA-256 of the canonical JSON identity tuple, first 128 bits.
pub fn compute_workspace_revision_handle(
    workspace_fingerprint: &str,
    path: &str,
    sha256: &str,
) -> String {
    let identity = CanonicalValue::Object(BTreeMap::from([
        ("path".to_owned(), CanonicalValue::Str(path.to_owned())),
        ("sha256".to_owned(), CanonicalValue::Str(sha256.to_owned())),
        (
            "workspace".to_owned(),
            CanonicalValue::Str(workspace_fingerprint.to_owned()),
        ),
    ]));
    let digest = sha256_hex(canonicalize(&identity).as_bytes());
    format!("{WORKSPACE_REVISION_HANDLE_PREFIX}{}", &digest[..32])
}

/// Options for constructing a revision registry.
#[derive(Debug, Clone)]
pub struct WorkspaceRevisionRegistryOptions {
    /// Owning workspace fingerprint (workspace-scoped handles).
    pub workspace_fingerprint: String,
    /// Maximum tracked identities (FIFO eviction).
    pub max_entries: Option<usize>,
}

/// Session-scoped, bounded, in-memory workspace revision registry.
///
/// One clear owner: the host session constructs one registry per
/// workspace. Identities stay resolvable as historical evidence after
/// `invalidate_path`; only the current-path binding is dropped. FIFO
/// eviction preserves deterministic behavior under the entry bound.
#[derive(Debug)]
pub struct WorkspaceRevisionRegistry {
    workspace_fingerprint: String,
    max_entries: usize,
    /// path -> current valid handle.
    by_path: BTreeMap<String, String>,
    /// handle -> full identity (insertion-ordered for deterministic
    /// `revision_for_state` scans).
    identities: Vec<(String, WorkspaceRevisionIdentity)>,
    /// FIFO eviction order (handles).
    order: Vec<String>,
    observed: Vec<ObservedWorkspaceRead>,
    clock: u64,
}

impl WorkspaceRevisionRegistry {
    /// Construct a registry; a positive entry limit is required.
    pub fn new(
        options: WorkspaceRevisionRegistryOptions,
    ) -> Result<Self, String> {
        let max_entries =
            options.max_entries.unwrap_or(DEFAULT_REVISION_REGISTRY_LIMIT);
        if max_entries < 1 {
            return Err(
                "The revision registry requires a positive entry limit."
                    .to_owned(),
            );
        }
        Ok(Self {
            workspace_fingerprint: options.workspace_fingerprint,
            max_entries,
            by_path: BTreeMap::new(),
            identities: Vec::new(),
            order: Vec::new(),
            observed: Vec::new(),
            clock: 0,
        })
    }

    fn identity_for_handle(
        &self,
        handle: &str,
    ) -> Option<&WorkspaceRevisionIdentity> {
        self.identities.iter().find_map(|(candidate, identity)| {
            (candidate == handle).then_some(identity)
        })
    }

    /// Issue (or return the existing) handle for one exact file state.
    pub fn issue(&mut self, path: &str, sha256: &str) -> String {
        if let Some(existing) = self.by_path.get(path) {
            if let Some(identity) = self.identity_for_handle(existing) {
                if identity.sha256 == sha256 {
                    return existing.clone();
                }
            }
        }
        let handle = compute_workspace_revision_handle(
            &self.workspace_fingerprint,
            path,
            sha256,
        );
        let identity = WorkspaceRevisionIdentity {
            workspace_fingerprint: self.workspace_fingerprint.clone(),
            path: path.to_owned(),
            sha256: sha256.to_owned(),
        };
        self.identities.push((handle.clone(), identity));
        self.order.push(handle.clone());
        self.by_path.insert(path.to_owned(), handle.clone());
        self.evict_if_needed();
        handle
    }

    /// Resolve a handle to its full identity; `None` when unknown or
    /// foreign (wrong prefix or not in this registry).
    pub fn resolve(&self, handle: &str) -> Option<&WorkspaceRevisionIdentity> {
        if !handle.starts_with(WORKSPACE_REVISION_HANDLE_PREFIX) {
            return None;
        }
        self.identity_for_handle(handle)
    }

    /// The last issued valid handle for a path, or `None`.
    pub fn current_revision(&self, path: &str) -> Option<&str> {
        self.by_path.get(path).map(String::as_str)
    }

    /// A handle for an exact (path, sha256) state, or `None`.
    pub fn revision_for_state(
        &self,
        path: &str,
        sha256: &str,
    ) -> Option<&str> {
        self.identities.iter().find_map(|(handle, identity)| {
            (identity.path == path && identity.sha256 == sha256)
                .then_some(handle.as_str())
        })
    }

    /// Invalidate a path: the current binding is dropped while the
    /// identity stays resolvable as historical evidence.
    pub fn invalidate_path(&mut self, path: &str) {
        self.by_path.remove(path);
    }

    /// Record that a read observed a specific revision (bounded
    /// session-local groundwork; never authoritative).
    pub fn observe_read(
        &mut self,
        path: &str,
        revision: &str,
        mode: ObservedReadMode,
    ) {
        self.clock += 1;
        self.observed.push(ObservedWorkspaceRead {
            path: path.to_owned(),
            revision: revision.to_owned(),
            mode,
            at_ms: self.clock,
        });
        if self.observed.len() > 64 {
            self.observed.remove(0);
        }
    }

    /// The bounded session-local observed-read window.
    pub fn observed_reads(&self) -> &[ObservedWorkspaceRead] {
        &self.observed
    }

    /// Number of tracked identities.
    pub fn size(&self) -> usize {
        self.identities.len()
    }

    /// Clear every binding, identity, and observation.
    pub fn clear(&mut self) {
        self.by_path.clear();
        self.identities.clear();
        self.order.clear();
        self.observed.clear();
        self.clock = 0;
    }

    fn evict_if_needed(&mut self) {
        while self.identities.len() > self.max_entries {
            let Some(oldest) = self.order.first().cloned() else {
                break;
            };
            self.order.remove(0);
            let position = self
                .identities
                .iter()
                .position(|(handle, _)| *handle == oldest);
            let Some(position) = position else {
                continue;
            };
            let (_, identity) = self.identities.remove(position);
            if self
                .by_path
                .get(&identity.path)
                .is_some_and(|current| *current == oldest)
            {
                self.by_path.remove(&identity.path);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{
        ObservedReadMode, WorkspaceRevisionRegistry,
        WorkspaceRevisionRegistryOptions, compute_workspace_revision_handle,
    };

    fn sha256(text: &str) -> String {
        // Deterministic 64-hex test identity (the registry treats SHA-256
        // as an opaque string; only the format matters for fixtures).
        let mut value: u32 = 0;
        for character in text.chars() {
            value = value.wrapping_mul(31).wrapping_add(character as u32);
        }
        format!("{value:064x}")
    }

    fn registry(fingerprint: &str) -> WorkspaceRevisionRegistry {
        WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
            workspace_fingerprint: fingerprint.to_owned(),
            max_entries: None,
        })
        .expect("registry construction succeeds")
    }

    #[test]
    fn issues_opaque_handles_backed_by_the_identity_tuple() {
        let mut registry = registry("ws-A");
        let handle = registry.issue("player.gd", &sha256("a"));
        assert!(handle.starts_with("rev_"));
        assert_eq!(handle.len(), 36);
        assert!(handle[4..].bytes().all(|byte| byte.is_ascii_hexdigit()));
        let identity = registry.resolve(&handle).expect("handle resolves");
        assert_eq!(identity.workspace_fingerprint, "ws-A");
        assert_eq!(identity.path, "player.gd");
        assert_eq!(identity.sha256, sha256("a"));
    }

    #[test]
    fn deduplicates_identical_states_and_distinguishes_changed_states() {
        let mut registry = registry("ws-A");
        let first = registry.issue("player.gd", &sha256("a"));
        assert_eq!(registry.issue("player.gd", &sha256("a")), first);
        let second = registry.issue("player.gd", &sha256("b"));
        assert_ne!(second, first);
        assert_eq!(
            registry.current_revision("player.gd"),
            Some(second.as_str())
        );
    }

    #[test]
    fn never_resolves_across_workspaces() {
        let mut registry_a = registry("ws-A");
        let registry_b = registry("ws-B");
        let handle = registry_a.issue("player.gd", &sha256("a"));
        assert!(registry_b.resolve(&handle).is_none());
        assert_ne!(
            compute_workspace_revision_handle(
                "ws-A",
                "player.gd",
                &sha256("a")
            ),
            compute_workspace_revision_handle(
                "ws-B",
                "player.gd",
                &sha256("a")
            ),
        );
    }

    #[test]
    fn does_not_self_evict_with_a_limit_of_one_entry() {
        let mut registry =
            WorkspaceRevisionRegistry::new(WorkspaceRevisionRegistryOptions {
                workspace_fingerprint: "ws-A".to_owned(),
                max_entries: Some(1),
            })
            .expect("registry construction succeeds");
        let handle = registry.issue("player.gd", &sha256("a"));
        assert!(registry.resolve(&handle).is_some());
        let second = registry.issue("health.gd", &sha256("b"));
        assert!(registry.resolve(&second).is_some());
        assert!(registry.resolve(&handle).is_none());
    }

    #[test]
    fn invalidates_the_current_binding_while_keeping_the_identity_historical()
    {
        let mut registry = registry("ws-A");
        let handle = registry.issue("player.gd", &sha256("a"));
        registry.invalidate_path("player.gd");
        assert!(registry.current_revision("player.gd").is_none());
        assert!(registry.resolve(&handle).is_some());
    }

    #[test]
    fn tracks_observed_reads_with_a_bounded_session_local_window() {
        let mut registry = registry("ws-A");
        let handle = registry.issue("player.gd", &sha256("a"));
        registry.observe_read("player.gd", &handle, ObservedReadMode::Exact);
        let reads = registry.observed_reads();
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].path, "player.gd");
        assert_eq!(reads[0].revision, handle);
        assert_eq!(reads[0].mode, ObservedReadMode::Exact);
        assert_eq!(reads[0].at_ms, 1);
    }

    #[test]
    fn observed_window_is_bounded_at_64_with_fifo_order() {
        let mut registry = registry("ws-A");
        let handle = registry.issue("player.gd", &sha256("a"));
        for _ in 0..70 {
            registry.observe_read(
                "player.gd",
                &handle,
                ObservedReadMode::Exact,
            );
        }
        let reads = registry.observed_reads();
        assert_eq!(reads.len(), 64);
        assert_eq!(reads[0].at_ms, 7);
        assert_eq!(reads[63].at_ms, 70);
    }
}
