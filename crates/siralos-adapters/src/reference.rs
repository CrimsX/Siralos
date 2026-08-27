//! Reference resolvers and materializer (Stage 3 milestone 5; R13.3).
//!
//! Mirrors `packages/adapters/src/reference/{resolver,materializer}*`.
//!
//! The local-directory resolver maps a declared source to a canonical
//! path plus a bounded manifest fingerprint: every regular file beneath
//! the root is enumerated (symlinks are never traversed; special files
//! make the manifest non-fingerprintable — fail closed with a precise
//! reason) and hashed with SHA-256 under an explicit per-file cap, then
//! the fingerprint is the SHA-256 of the canonical JSON manifest of
//! sorted relative paths + hashes.
//!
//! The REAL production repository backend always reports the source
//! unavailable: repository resolution requires sandboxed git execution,
//! which is unavailable at this stage — nothing is ever spawned or
//! fetched. The fake backend is the deterministic, network-free stand-in
//! for tests and the behavior harness.

use serde_json::json;
use siralos_core::identity::Sha256;
use siralos_core::identity::{canonicalize_json, sha256_hex_str};
use siralos_core::reference::{
    MaterializationOutcome, MaterializationStatus, ReferenceLimits,
    ReferenceMaterializerPort, ReferenceResolutionOutcome,
    ReferenceResolverPort, ReferenceSource, RepositoryRef,
    ResolvedReferenceIdentity,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// REPOSITORY_RESOLUTION_UNAVAILABLE_MESSAGE.
pub const REPOSITORY_RESOLUTION_UNAVAILABLE_MESSAGE: &str = "repository resolution requires sandboxed git execution, which is unavailable at this stage";

/// The REAL production repository backend: always reports the source
/// unavailable. No sandboxed git execution exists at this stage — nothing
/// is spawned and nothing is fetched.
pub struct UnavailableRepositoryBackend;

impl UnavailableRepositoryBackend {
    /// Resolve nothing; report the typed unavailable boundary.
    pub fn resolve_commit(
        &self,
        _origin: &str,
        _ref: &RepositoryRef,
        _allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        ReferenceResolutionOutcome::Unavailable {
            reason: REPOSITORY_RESOLUTION_UNAVAILABLE_MESSAGE.to_owned(),
        }
    }
}

/// MUTABLE_REF_REFUSAL_MESSAGE.
pub const MUTABLE_REF_REFUSAL_MESSAGE: &str =
    "mutable repository ref requires an explicit pinned commit/tag";

/// One fixture entry of the fake repository backend.
#[derive(Debug, Clone, Default)]
pub struct FakeRepositoryFixtureEntry {
    /// Known commit ids (full or abbreviated, exactly as declared).
    pub commits: BTreeSet<String>,
    /// Tag -> commit mapping.
    pub tags: BTreeMap<String, String>,
    /// Branch -> commit mapping.
    pub branches: BTreeMap<String, String>,
}

/// Deterministic, network-free repository backend for tests and the
/// behavior harness: `origin -> { commits, tags, branches }`. Mutable
/// refs (branches) are refused unless `allowMutableRefs` is set,
/// mirroring the registry policy as defense in depth.
#[derive(Default)]
pub struct FakeRepositoryBackend {
    fixture: BTreeMap<String, FakeRepositoryFixtureEntry>,
}

impl FakeRepositoryBackend {
    #[must_use]
    /// new.
    pub fn new(fixture: BTreeMap<String, FakeRepositoryFixtureEntry>) -> Self {
        Self { fixture }
    }

    /// Resolve one declared ref against the fixture with exact reasons.
    pub fn resolve_commit(
        &self,
        origin: &str,
        r#ref: &RepositoryRef,
        allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        let Some(repository) = self.fixture.get(origin) else {
            return ReferenceResolutionOutcome::Failed {
                reason: format!("Unknown repository origin \"{origin}\"."),
            };
        };
        match r#ref {
            RepositoryRef::Commit { commit } => {
                if !is_commit_sha(commit) {
                    return ReferenceResolutionOutcome::Failed {
                        reason: format!("Malformed commit \"{commit}\"."),
                    };
                }
                if !repository.commits.contains(commit) {
                    return ReferenceResolutionOutcome::Failed {
                        reason: format!("Unknown commit \"{commit}\"."),
                    };
                }
                ReferenceResolutionOutcome::Resolved {
                    identity: ResolvedReferenceIdentity::Repository {
                        origin: origin.to_owned(),
                        commit: commit.clone(),
                        requested_ref: r#ref.clone(),
                    },
                }
            }
            RepositoryRef::Tag { tag } => match repository.tags.get(tag) {
                None => ReferenceResolutionOutcome::Failed {
                    reason: format!("Unknown tag \"{tag}\"."),
                },
                Some(commit) => ReferenceResolutionOutcome::Resolved {
                    identity: ResolvedReferenceIdentity::Repository {
                        origin: origin.to_owned(),
                        commit: commit.clone(),
                        requested_ref: r#ref.clone(),
                    },
                },
            },
            RepositoryRef::Branch { branch } => {
                if !allow_mutable_refs {
                    return ReferenceResolutionOutcome::Refused {
                        reason: MUTABLE_REF_REFUSAL_MESSAGE.to_owned(),
                    };
                }
                match repository.branches.get(branch) {
                    None => ReferenceResolutionOutcome::Failed {
                        reason: format!("Unknown branch \"{branch}\"."),
                    },
                    Some(commit) => ReferenceResolutionOutcome::Resolved {
                        identity: ResolvedReferenceIdentity::Repository {
                            origin: origin.to_owned(),
                            commit: commit.clone(),
                            requested_ref: r#ref.clone(),
                        },
                    },
                }
            }
        }
    }
}

fn is_commit_sha(commit: &str) -> bool {
    (7..=64).contains(&commit.len())
        && commit.bytes().all(|b| {
            b.is_ascii_digit()
                || (b'a'..=b'f').contains(&b)
                || (b'A'..=b'F').contains(&b)
        })
}

/// Resolver that routes repository sources through a revision backend.
pub struct RepositoryResolver<B> {
    /// backend.
    pub backend: B,
}

impl<B> ReferenceResolverPort for RepositoryResolver<B>
where
    B: Fn(&str, &RepositoryRef, bool) -> ReferenceResolutionOutcome
        + Send
        + Sync,
{
    fn resolve_identity(
        &self,
        source: &ReferenceSource,
        allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        let ReferenceSource::Repository { repository, r#ref } = source else {
            return ReferenceResolutionOutcome::Unavailable {
                reason: "This resolver only handles repository sources."
                    .to_owned(),
            };
        };
        let outcome = (self.backend)(repository, r#ref, allow_mutable_refs);
        match outcome {
            ReferenceResolutionOutcome::Resolved {
                identity:
                    ResolvedReferenceIdentity::Repository {
                        origin,
                        commit,
                        ..
                    },
            } => ReferenceResolutionOutcome::Resolved {
                identity:
                    ResolvedReferenceIdentity::Repository {
                        origin,
                        commit,
                        requested_ref: r#ref.clone(),
                    },
            },
            ReferenceResolutionOutcome::Resolved { .. } => ReferenceResolutionOutcome::Failed {
                reason: "Repository backend returned a non-repository identity.".to_owned(),
            },
            other => other,
        }
    }
}

/// Manifest entry budget factor for non-file entries.
const MAX_DIRECTORY_ENTRIES_FACTOR: usize = 2;
const MAX_MANIFEST_DEPTH: usize = 64;

enum ManifestOutcome {
    /// Successful manifest enumeration.
    Ok {
        /// Sorted (relativePath, sha256) pairs.
        files: Vec<(String, String)>,
    },
    /// Failed outcome.
    Failed {
        /// Exact reference-failure reason.
        reason: String,
    },
}

/// Bounded, deterministic manifest enumeration. Every entry is inspected
/// with symlink metadata (symlinks skipped, never traversed; special
/// files fail the manifest); regular files are size-checked and SHA-256
/// hashed through a bounded read loop (a short read is never treated as
/// EOF, and a file grown past the cap after inspection fails the
/// manifest).
fn build_manifest(root: &Path, limits: &ReferenceLimits) -> ManifestOutcome {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut total_bytes: usize = 0;
    let mut directories_visited: usize = 0;
    let mut entries_examined: usize = 0;
    let mut pending: Vec<(std::path::PathBuf, String)> =
        vec![(root.to_path_buf(), String::new())];
    while let Some((directory, relative)) = pending.pop() {
        directories_visited += 1;
        if directories_visited > limits.max_manifest_entries {
            return ManifestOutcome::Failed {
                reason: format!(
                    "Reference manifest is too large: more than {} directories.",
                    limits.max_manifest_entries
                ),
            };
        }
        if relative.split('/').count() > MAX_MANIFEST_DEPTH {
            return ManifestOutcome::Failed {
                reason: format!(
                    "Reference manifest is too large: directory depth exceeds {MAX_MANIFEST_DEPTH}."
                ),
            };
        }
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                return ManifestOutcome::Failed {
                    reason: format!(
                        "Cannot enumerate reference directory: {}",
                        describe_fs_error(&error)
                    ),
                };
            }
        };
        let mut names: BTreeSet<String> = BTreeSet::new();
        for entry in entries.flatten() {
            entries_examined += 1;
            if entries_examined
                > limits.max_manifest_entries * MAX_DIRECTORY_ENTRIES_FACTOR
            {
                return ManifestOutcome::Failed {
                    reason: "Reference manifest is too large: entry budget exceeded.".to_owned(),
                };
            }
            names.insert(entry.file_name().to_string_lossy().into_owned());
        }
        for name in names {
            let absolute = directory.join(&name);
            let relative_path = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let metadata = match std::fs::symlink_metadata(&absolute) {
                Ok(metadata) => metadata,
                Err(error) => {
                    return ManifestOutcome::Failed {
                        reason: format!(
                            "Cannot inspect reference entry {relative_path}: {}",
                            describe_fs_error(&error)
                        ),
                    };
                }
            };
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                // Symlinks are never traversed and never enter the manifest.
                continue;
            }
            if file_type.is_dir() {
                pending.push((absolute, relative_path));
                continue;
            }
            if !file_type.is_file() {
                return ManifestOutcome::Failed {
                    reason: format!(
                        "Reference manifest is not fingerprintable: special file at {relative_path}."
                    ),
                };
            }
            if metadata.len() as usize > limits.max_file_sha256_bytes {
                return ManifestOutcome::Failed {
                    reason: format!(
                        "Reference manifest is not fingerprintable: file at {relative_path} is {} bytes (limit {}).",
                        metadata.len(),
                        limits.max_file_sha256_bytes
                    ),
                };
            }
            let hash = match hash_file_bounded(
                &absolute,
                limits.max_file_sha256_bytes,
            ) {
                Ok(hash) => hash,
                Err(reason) => return ManifestOutcome::Failed { reason },
            };
            files.push((relative_path, hash));
            total_bytes += metadata.len() as usize;
            if files.len() > limits.max_manifest_entries {
                return ManifestOutcome::Failed {
                    reason: format!(
                        "Reference manifest is too large: more than {} files.",
                        limits.max_manifest_entries
                    ),
                };
            }
            if total_bytes > limits.max_manifest_bytes {
                return ManifestOutcome::Failed {
                    reason: format!(
                        "Reference manifest is too large: more than {} bytes.",
                        limits.max_manifest_bytes
                    ),
                };
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    ManifestOutcome::Ok { files }
}

/// Bounded SHA-256 of one regular file (explicit-offset read loop).
fn hash_file_bounded(
    absolute: &Path,
    max_bytes: usize,
) -> Result<String, String> {
    let mut file = std::fs::File::open(absolute).map_err(|error| {
        format!("Cannot read reference file: {}", describe_fs_error(&error))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total: usize = 0;
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            format!(
                "Cannot hash reference file: {}",
                describe_fs_error(&error)
            )
        })?;
        if read == 0 {
            break;
        }
        total += read;
        if total > max_bytes {
            return Err(format!(
                "Reference manifest is not fingerprintable: file exceeds {max_bytes} bytes."
            ));
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finish())
}

fn describe_fs_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_owned(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        _ => error.to_string(),
    }
}

/// Local-directory resolver: canonicalize the declared path, require a
/// directory, and fingerprint its bounded manifest.
pub struct LocalDirectoryResolver {
    /// Effective limits.
    pub limits: ReferenceLimits,
}

impl ReferenceResolverPort for LocalDirectoryResolver {
    fn resolve_identity(
        &self,
        source: &ReferenceSource,
        _allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        let ReferenceSource::LocalDirectory { path } = source else {
            return ReferenceResolutionOutcome::Unavailable {
                reason: "This resolver only handles local-directory sources."
                    .to_owned(),
            };
        };
        let canonical = match std::fs::canonicalize(path) {
            Ok(canonical) => canonical,
            Err(error) => {
                return ReferenceResolutionOutcome::Unavailable {
                    reason: format!(
                        "Reference path cannot be resolved: {}",
                        describe_fs_error(&error)
                    ),
                };
            }
        };
        let metadata = match std::fs::metadata(&canonical) {
            Ok(metadata) => metadata,
            Err(error) => {
                return ReferenceResolutionOutcome::Unavailable {
                    reason: format!(
                        "Reference path is not accessible: {}",
                        describe_fs_error(&error)
                    ),
                };
            }
        };
        if !metadata.is_dir() {
            return ReferenceResolutionOutcome::Failed {
                reason: "Reference path is not a directory.".to_owned(),
            };
        }
        let manifest = build_manifest(&canonical, &self.limits);
        let files = match manifest {
            ManifestOutcome::Ok { files } => files,
            ManifestOutcome::Failed { reason } => {
                return ReferenceResolutionOutcome::Failed { reason };
            }
        };
        let entries: Vec<serde_json::Value> = files
            .iter()
            .map(|(relative_path, hash)| json!({ "relativePath": relative_path, "sha256": hash }))
            .collect();
        let fingerprint =
            sha256_hex_str(&canonicalize_json(&json!({ "files": entries })));
        ReferenceResolutionOutcome::Resolved {
            identity: ResolvedReferenceIdentity::LocalDirectory {
                canonical_path: canonical.to_string_lossy().into_owned(),
                fingerprint,
            },
        }
    }
}

/// Dispatch resolver: routes by source kind; a missing side fails closed
/// as unavailable.
#[derive(Default)]
pub struct ReferenceResolverDispatch {
    /// local.
    pub local: Option<Arc<dyn ReferenceResolverPort>>,
    /// repository.
    pub repository: Option<Arc<dyn ReferenceResolverPort>>,
}

impl ReferenceResolverPort for ReferenceResolverDispatch {
    fn resolve_identity(
        &self,
        source: &ReferenceSource,
        allow_mutable_refs: bool,
    ) -> ReferenceResolutionOutcome {
        match source {
            ReferenceSource::LocalDirectory { .. } => match &self.local {
                Some(local) => {
                    local.resolve_identity(source, allow_mutable_refs)
                }
                None => ReferenceResolutionOutcome::Unavailable {
                    reason: "Local-directory resolution is not configured."
                        .to_owned(),
                },
            },
            ReferenceSource::Repository { .. } => match &self.repository {
                Some(repository) => {
                    repository.resolve_identity(source, allow_mutable_refs)
                }
                None => ReferenceResolutionOutcome::Unavailable {
                    reason: "Repository resolution is not configured."
                        .to_owned(),
                },
            },
        }
    }
}

/// REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE.
pub const REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE: &str = "repository materialization requires sandboxed git execution, which is unavailable at this stage";

#[derive(Default)]
struct MaterializerState {
    statuses: HashMap<String, MaterializationStatus>,
}

/// The REAL production materializer: local-directory references need no
/// copy — the directory IS the reference (`materialize` returns the
/// canonical path with zero filesystem operations and status
/// `not-required`); repository references fail closed as `unavailable` —
/// materialization requires sandboxed git execution, which is unavailable
/// at this stage, so nothing is ever created or fetched.
#[derive(Default)]
pub struct ReferenceMaterializer {
    state: std::sync::Mutex<MaterializerState>,
}

impl ReferenceMaterializer {
    #[must_use]
    /// new.
    pub fn new() -> Self {
        Self::default()
    }
}

impl ReferenceMaterializerPort for ReferenceMaterializer {
    /// materialize.
    fn materialize(
        &self,
        reference_id: &str,
        identity: &siralos_core::reference::ResolvedReferenceIdentity,
    ) -> MaterializationOutcome {
        match identity {
            ResolvedReferenceIdentity::LocalDirectory {
                canonical_path,
                ..
            } => {
                // No copy: the directory IS the reference. Zero filesystem
                // operations are performed.
                self.state
                    .lock()
                    .expect("materializer state")
                    .statuses
                    .insert(
                        reference_id.to_owned(),
                        MaterializationStatus::NotRequired,
                    );
                MaterializationOutcome::Materialized {
                    root: canonical_path.clone(),
                }
            }
            ResolvedReferenceIdentity::Repository { .. } => {
                self.state
                    .lock()
                    .expect("materializer state")
                    .statuses
                    .insert(
                        reference_id.to_owned(),
                        MaterializationStatus::Unavailable,
                    );
                MaterializationOutcome::Unavailable {
                    reason: REPOSITORY_MATERIALIZATION_UNAVAILABLE_MESSAGE
                        .to_owned(),
                }
            }
        }
    }

    /// Lifecycle status.
    fn status(&self, reference_id: &str) -> MaterializationStatus {
        self.state
            .lock()
            .expect("materializer state")
            .statuses
            .get(reference_id)
            .copied()
            .unwrap_or(MaterializationStatus::NotMaterialized)
    }
}
