//! The machine-generated workspace lock (Stage 5.4, decision 50):
//! resolved portable identities per ADR 0036 §8-9.
//!
//! The lock is derived data: it binds digests of declarations the
//! workspace already holds — the profile identity (5.1) and the plugin
//! records (decision 38/39) — and grants nothing. Regeneration is
//! deterministic and idempotent (same declarations → the same lock
//! digest), so verification compares recomputed digests, never file
//! bytes alone. No network, no spawn, no wall clock.

use std::collections::BTreeMap;

use super::ProfileValidationError;
use crate::identity::CanonicalValue;
use crate::identity::compute_artifact_digest;

/// Maximum number of plugin identities in one lock.
pub const MAX_LOCK_PLUGIN_IDENTITIES: usize = 16;
/// Maximum lock identity id length in UTF-8 bytes.
pub const MAX_LOCK_ID_BYTES: usize = 64;
/// Maximum lock identity path length in UTF-8 bytes.
pub const MAX_LOCK_PATH_BYTES: usize = 256;

fn is_hex64(digest: &str) -> bool {
    digest.len() == 64
        && digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// One resolved plugin identity bound into the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockPluginIdentity {
    /// The plugin record id.
    pub id: String,
    /// The declared workspace-relative path.
    pub path: String,
    /// The declared content digest (64 lowercase hex characters).
    pub digest: String,
}

impl LockPluginIdentity {
    /// Validate one identity at the lock boundary.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileValidationError`] for empty/oversize ids,
    /// oversize paths, and non-hex digests.
    pub fn validate(&self) -> Result<(), ProfileValidationError> {
        let fail = |message: String| ProfileValidationError { message };
        if self.id.is_empty() || self.id.len() > MAX_LOCK_ID_BYTES {
            return Err(fail(format!(
                "A lock plugin id must be 1..={MAX_LOCK_ID_BYTES} bytes."
            )));
        }
        if self.path.is_empty() || self.path.len() > MAX_LOCK_PATH_BYTES {
            return Err(fail(format!(
                "A lock plugin path must be 1..={MAX_LOCK_PATH_BYTES} bytes."
            )));
        }
        if !is_hex64(&self.digest) {
            return Err(fail(
                "A lock plugin digest must be 64 lowercase hex characters."
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// The resolved workspace lock: sorted plugin identities plus the
/// profile identity, bound by one lock digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLock {
    /// The profile evidence digest when a profile resolves.
    pub profile_digest: Option<String>,
    /// Sorted plugin identities.
    pub plugins: Vec<LockPluginIdentity>,
    /// The lock digest binding every identity.
    pub lock_digest: String,
}

/// Create a workspace lock from the declared identities. Idempotent:
/// the same declarations always produce the same lock digest.
///
/// # Errors
///
/// Returns [`ProfileValidationError`] when the profile digest is not
/// 64 lowercase hex characters, any plugin identity fails validation,
/// plugin ids are duplicated, or the count exceeds
/// [`MAX_LOCK_PLUGIN_IDENTITIES`].
pub fn create_workspace_lock(
    profile_digest: Option<&str>,
    plugins: &[LockPluginIdentity],
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
    if plugins.len() > MAX_LOCK_PLUGIN_IDENTITIES {
        return Err(fail(format!(
            "The lock exceeds the {MAX_LOCK_PLUGIN_IDENTITIES}-plugin bound."
        )));
    }
    let mut sorted: Vec<LockPluginIdentity> = plugins.to_vec();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));
    let mut seen = BTreeMap::new();
    for identity in &sorted {
        identity.validate()?;
        if seen.contains_key(identity.id.as_str()) {
            return Err(fail(format!(
                "The lock declares plugin id {} more than once.",
                identity.id
            )));
        }
        seen.insert(identity.id.as_str(), ());
    }
    let mut plugin_map = BTreeMap::new();
    for identity in &sorted {
        plugin_map.insert(
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
        ("plugins".to_owned(), CanonicalValue::Object(plugin_map)),
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
        plugins: sorted,
        lock_digest,
    })
}
