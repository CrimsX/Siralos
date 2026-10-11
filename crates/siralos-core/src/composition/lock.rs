//! The machine-generated workspace lock (Stage 5.4, decision 50):
//! resolved portable identities per ADR 0036 §8-9.
//!
//! The lock is derived data: it binds digests of declarations the
//! workspace already holds — the profile identity (5.1) and the module
//! records (decision 38/39) — and grants nothing. Regeneration is
//! deterministic and idempotent (same declarations → the same lock
//! digest), so verification compares recomputed digests, never file
//! bytes alone. No network, no spawn, no wall clock.

use std::collections::BTreeMap;

use super::ProfileValidationError;
use crate::identity::CanonicalValue;
use crate::identity::compute_artifact_digest;

/// Maximum number of module identities in one lock.
pub(crate) const MAX_LOCK_MODULE_IDENTITIES: usize = 16;
/// Maximum lock identity id length in UTF-8 bytes.
pub(crate) const MAX_LOCK_ID_BYTES: usize = 64;
/// Maximum lock identity path length in UTF-8 bytes.
pub(crate) const MAX_LOCK_PATH_BYTES: usize = 256;

fn is_hex64(digest: &str) -> bool {
    digest.len() == 64
        && digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// One resolved module identity bound into the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockModuleIdentity {
    /// The module record id.
    pub id: String,
    /// The declared workspace-relative path.
    pub path: String,
    /// The declared content digest (64 lowercase hex characters).
    pub digest: String,
}

impl LockModuleIdentity {
    /// Validate one identity at the lock boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileValidationError`] for empty/oversize ids,
    /// oversize paths, and non-hex digests.
    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        let fail = |message: String| ProfileValidationError { message };
        if self.id.is_empty()
            || self.id.len() > MAX_LOCK_ID_BYTES
            || self.id.chars().any(char::is_control)
        {
            return Err(fail(format!(
                "A lock module id must be 1..={MAX_LOCK_ID_BYTES} bytes without controls."
            )));
        }
        if self.path.is_empty()
            || self.path.len() > MAX_LOCK_PATH_BYTES
            || crate::workspace::path::validate_relative_path(&self.path)
                .is_err()
        {
            return Err(fail(format!(
                "A lock module path must be 1..={MAX_LOCK_PATH_BYTES} bytes and workspace-relative."
            )));
        }
        if !is_hex64(&self.digest) {
            return Err(fail(
                "A lock module digest must be 64 lowercase hex characters."
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// The resolved workspace lock: sorted module identities plus the
/// profile identity, bound by one lock digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLock {
    /// The profile evidence digest when a profile resolves.
    pub profile_digest: Option<String>,
    /// Sorted module identities.
    pub modules: Vec<LockModuleIdentity>,
    /// The lock digest binding every identity.
    pub lock_digest: String,
}

/// Create a workspace lock from the declared identities. Idempotent:
/// the same declarations always produce the same lock digest.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the profile digest is not
/// 64 lowercase hex characters, any module identity fails validation,
/// module ids are duplicated, or the count exceeds
/// [`MAX_LOCK_MODULE_IDENTITIES`].
pub fn create_workspace_lock(
    profile_digest: Option<&str>,
    modules: &[LockModuleIdentity],
) -> Result<WorkspaceLock, ProfileValidationError> {
    let fail = |message: String| ProfileValidationError { message };
    if let Some(digest) = profile_digest {
        if !is_hex64(digest) {
            return Err(fail(
                "A lock profile digest must be 64 lowercase hex characters."
                    .to_owned(),
            ));
        }
    }
    if modules.len() > MAX_LOCK_MODULE_IDENTITIES {
        return Err(fail(format!(
            "The lock exceeds the {MAX_LOCK_MODULE_IDENTITIES}-module bound."
        )));
    }
    let mut sorted: Vec<LockModuleIdentity> = modules.to_vec();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));
    let mut seen = BTreeMap::new();
    for identity in &sorted {
        identity.validate()?;
        if seen.contains_key(identity.id.as_str()) {
            return Err(fail(format!(
                "The lock declares module id {} more than once.",
                identity.id
            )));
        }
        seen.insert(identity.id.as_str(), ());
    }
    let mut module_map = BTreeMap::new();
    for identity in &sorted {
        module_map.insert(
            identity.id.as_str().to_owned(),
            CanonicalValue::Object(BTreeMap::from([
                (
                    "digest".to_owned(),
                    CanonicalValue::Str(identity.digest.clone()),
                ),
                ("id".to_owned(), CanonicalValue::Str(identity.id.clone())),
                (
                    "path".to_owned(),
                    CanonicalValue::Str(identity.path.clone()),
                ),
            ])),
        );
    }
    let payload = CanonicalValue::Object(BTreeMap::from([
        ("modules".to_owned(), CanonicalValue::Object(module_map)),
        (
            "profileDigest".to_owned(),
            profile_digest.map_or(CanonicalValue::Null, |digest| {
                CanonicalValue::Str(digest.to_owned())
            }),
        ),
    ]));
    let lock_digest = compute_artifact_digest("WorkspaceLock", 1, &payload)
        .map_err(|error| ProfileValidationError { message: error.message })?
        .value;
    Ok(WorkspaceLock {
        profile_digest: profile_digest.map(str::to_owned),
        modules: sorted,
        lock_digest,
    })
}
