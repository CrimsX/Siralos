//! Bounded profile-document parsing (Stage 5.1, decision 47).
//!
//! Owns the portable `siralos.toml` profile declaration shape for this
//! slice: a `[profile]` table with a bounded `name` and a bounded
//! `[profile.permissions]` table of capability → rule strings. Parsing is
//! pure, bounded, and deterministic: the document is size-capped, unknown
//! keys are rejected, every capability id and rule string is validated at
//! the boundary, and the output feeds
//! `siralos_core::composition::resolve_profile_overlay` unchanged. No
//! filesystem, network, or process access happens here — callers hand in
//! the document bytes they already hold.

use crate::domain::manifest::{
    MAX_SIRALOS_TOML_BYTES, SIRALOS_TOML_FILE_NAME,
};
use crate::workspace::fs::{BoundedFileRead, read_complete_file_bounded};
use siralos_core::composition::{
    MAX_PROFILE_NAME_BYTES, MAX_PROFILE_OVERLAY_ENTRIES, ProfileOverlayEntry,
    ProfileRecord,
};
use siralos_core::context::ContextPolicy;
use siralos_core::tool::capability::CapabilityId;
use siralos_core::tool::permission::PermissionRule;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Filesystem identity evidence captured with one profile revision.
///
/// Content hashes cannot distinguish an A→B→A sequence when a filesystem
/// restores the same bytes. The portable evidence below is deliberately
/// fail-closed: when the platform cannot provide it, the snapshot is not
/// bindable and cannot authorize a write. Unix adds the stable device/inode
/// pair and change times; other targets use the strongest stable std metadata
/// available without enabling an unstable platform API.
#[derive(Clone, PartialEq, Eq)]
struct ProfileFileIdentity {
    length: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    modified_seconds: i64,
    #[cfg(unix)]
    modified_nanoseconds: i64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
    #[cfg(not(unix))]
    modified: Option<std::time::SystemTime>,
    #[cfg(not(unix))]
    created: Option<std::time::SystemTime>,
}

fn profile_file_identity(
    metadata: &std::fs::Metadata,
) -> Option<ProfileFileIdentity> {
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(ProfileFileIdentity {
            length: metadata.len(),
            device: metadata.dev(),
            inode: metadata.ino(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        })
    }
    #[cfg(not(unix))]
    {
        let modified = metadata.modified().ok();
        let created = metadata.created().ok();
        if modified.is_none() && created.is_none() {
            return None;
        }
        Some(ProfileFileIdentity { length: metadata.len(), modified, created })
    }
}

/// Maximum complete profile-document size in UTF-8 bytes.
pub const MAX_PROFILE_DOCUMENT_BYTES: usize =
    crate::domain::manifest::MAX_SIRALOS_TOML_BYTES;
/// Maximum number of skill names selected by one profile.
pub const MAX_PROFILE_SKILL_ENTRIES: usize = 128;
/// Maximum UTF-8 bytes in one selected skill name.
pub const MAX_PROFILE_SKILL_NAME_BYTES: usize = 128;
/// Maximum number of selected plugin ids in one profile.
pub const MAX_PROFILE_PLUGIN_ENTRIES: usize = 16;
/// Maximum bytes in one selected plugin id.
pub const MAX_PROFILE_PLUGIN_ID_BYTES: usize = 64;

/// A typed profile-document parse failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDocumentError {
    /// Deterministic, human-readable reason.
    pub message: String,
}

fn error(message: impl Into<String>) -> ProfileDocumentError {
    ProfileDocumentError { message: message.into() }
}

/// The bounded workspace-profile load outcome (Stage 5.2, decision 48):
/// the profile record when a valid `[profile]` document is present, the
/// typed invalid state otherwise. Per decision 48 C3 an invalid state
/// never blocks session composition - it is simply not applied, with a
/// truthful diagnostic - so there is no error variant to propagate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum WorkspaceProfileLoad {
    /// No `siralos.toml` profile document in the workspace.
    Absent,
    /// A valid profile record was loaded.
    Record(ProfileRecord),
    /// The document exists but the profile was not loaded; the
    /// diagnostic records why (unreadable, oversize, non-UTF-8, syntax,
    /// or shape violation).
    Invalid {
        /// Truthful reason the profile was not loaded.
        diagnostic: String,
    },
}

/// One exact profile-document read and its parse outcome. The raw bytes stay
/// private so diagnostics cannot accidentally persist a credential; callers
/// receive only the digest/length and detached parsed load state.
#[derive(Clone)]
pub struct WorkspaceProfileSnapshot {
    load: WorkspaceProfileLoad,
    raw_sha256: String,
    raw_len: usize,
    // Whether the target existed when this snapshot was observed. This is
    // separate from `WorkspaceProfileLoad::Absent`, because an empty present
    // file has no profile but must not authorize an absent-target commit.
    present: bool,
    // Filesystem identity evidence for a present target. It is deliberately
    // private and is carried only into a one-shot write token.
    file_identity: Option<ProfileFileIdentity>,
    // A snapshot may be produced by a failed lstat/read. Such a snapshot must
    // not become a write expectation: an absent-file digest would otherwise
    // look like permission to create a file after an unreadable target.
    bindable: bool,
    // All tokens minted from this observation share this gate. Keeping the
    // gate on the snapshot prevents two independent `write_token()` calls
    // from replaying the same observed revision.
    write_authority: Arc<AtomicBool>,
}

impl PartialEq for WorkspaceProfileSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.load == other.load
            && self.raw_sha256 == other.raw_sha256
            && self.raw_len == other.raw_len
            && self.present == other.present
            && self.file_identity == other.file_identity
            && self.bindable == other.bindable
    }
}

impl Eq for WorkspaceProfileSnapshot {}

/// A secret-free, one-shot identity for one observed profile-document
/// revision.
///
/// This is the only profile write input that may cross the frontend/worker
/// boundary. It contains no parsed profile fields, raw bytes, credential, or
/// endpoint. A writer must compare the current bounded bytes and filesystem
/// identity with this token before it parses or replaces them. Clones share
/// the one-shot gate: an attempted mutation consumes the authority, so a
/// retry must reload and observe a new revision.
#[derive(Clone)]
pub struct WorkspaceProfileWriteToken {
    raw_sha256: String,
    raw_len: usize,
    present: bool,
    file_identity: Option<ProfileFileIdentity>,
    consumed: Arc<AtomicBool>,
}

impl PartialEq for WorkspaceProfileWriteToken {
    fn eq(&self, other: &Self) -> bool {
        self.raw_sha256 == other.raw_sha256
            && self.raw_len == other.raw_len
            && self.present == other.present
            && self.file_identity == other.file_identity
            && self.consumed.load(Ordering::Acquire)
                == other.consumed.load(Ordering::Acquire)
    }
}

impl Eq for WorkspaceProfileWriteToken {}

impl std::fmt::Debug for WorkspaceProfileWriteToken {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceProfileWriteToken")
            .field("raw_sha256", &self.raw_sha256)
            .field("raw_len", &self.raw_len)
            .field("present", &self.present)
            .field("identity_present", &self.file_identity.is_some())
            .field("consumed", &self.consumed.load(Ordering::Acquire))
            .finish()
    }
}

impl WorkspaceProfileWriteToken {
    /// Return whether `bytes` are the exact content represented by this token.
    /// `None` means the target was absent when it was observed; an empty file
    /// is therefore not equivalent to an absent file.
    #[must_use]
    pub fn matches_bytes(&self, bytes: Option<&[u8]>) -> bool {
        match bytes {
            None => !self.present,
            Some(bytes) => {
                self.present
                    && self.raw_len == bytes.len()
                    && self.raw_sha256
                        == siralos_core::identity::sha256_hex(bytes)
            }
        }
    }

    /// Return whether the path and bytes still represent the exact observed
    /// filesystem revision. This catches a normal A→B→A replacement even
    /// when the final bytes hash back to the original content. The atomic
    /// commit primitive separately documents its residual pathname race; this
    /// check is the immediately preceding identity gate, not a cryptographic
    /// claim about a hostile process racing `rename`.
    #[must_use]
    pub fn matches_path(&self, path: &Path, bytes: Option<&[u8]>) -> bool {
        if !self.matches_bytes(bytes) {
            return false;
        }
        match (&self.file_identity, bytes) {
            (None, None) => matches!(
                std::fs::symlink_metadata(path),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            ),
            (Some(expected), Some(_)) => {
                let Ok(metadata) = std::fs::symlink_metadata(path) else {
                    return false;
                };
                !metadata.file_type().is_symlink()
                    && profile_file_identity(&metadata).as_ref()
                        == Some(expected)
            }
            _ => false,
        }
    }

    /// Consume this one-shot write authority.
    ///
    /// A token is deliberately not renewable: a failed or stale attempt must
    /// be followed by a fresh snapshot, rather than replaying an old approval
    /// after an intervening edit.
    pub fn consume(&self) -> Result<(), String> {
        if self.consumed.swap(true, Ordering::AcqRel) {
            Err("profile write authority is one-shot; reload before retrying"
                .to_owned())
        } else {
            Ok(())
        }
    }

    /// Whether this token has already been consumed by a mutation attempt.
    #[must_use]
    pub fn is_consumed(&self) -> bool {
        self.consumed.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for WorkspaceProfileSnapshot {
    fn fmt(
        &self,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        let load_kind = match &self.load {
            WorkspaceProfileLoad::Absent => "absent",
            WorkspaceProfileLoad::Record(_) => "record",
            WorkspaceProfileLoad::Invalid { .. } => "invalid",
        };
        formatter
            .debug_struct("WorkspaceProfileSnapshot")
            // `WorkspaceProfileLoad::Record` contains the declared credential;
            // never derive snapshot debug output from that value.
            .field("load_kind", &load_kind)
            .field("raw_sha256", &self.raw_sha256)
            .field("raw_len", &self.raw_len)
            .field("present", &self.present)
            .field("identity_present", &self.file_identity.is_some())
            .finish()
    }
}

impl WorkspaceProfileSnapshot {
    /// The detached parse outcome.
    #[must_use]
    pub fn load(&self) -> &WorkspaceProfileLoad {
        &self.load
    }

    /// SHA-256 over the exact bytes read once from `siralos.toml`.
    #[must_use]
    pub fn raw_sha256(&self) -> &str {
        &self.raw_sha256
    }

    /// Exact byte length of the source document.
    #[must_use]
    pub fn raw_len(&self) -> usize {
        self.raw_len
    }

    /// Project this observed revision into a secret-free write token.
    ///
    /// `None` means the source could not be read completely enough to bind a
    /// later mutation to it (for example an unreadable or oversize target).
    /// Callers must refuse such a snapshot rather than treating it as absent.
    #[must_use]
    pub fn write_token(&self) -> Option<WorkspaceProfileWriteToken> {
        self.bindable.then(|| WorkspaceProfileWriteToken {
            raw_sha256: self.raw_sha256.clone(),
            raw_len: self.raw_len,
            present: self.present,
            file_identity: self.file_identity.clone(),
            consumed: Arc::clone(&self.write_authority),
        })
    }
}

/// Load only a secret-free write identity for the workspace profile.
///
/// This convenience seam is for frontends that must retain a revision across
/// an approval or form-completion boundary without carrying the parsed
/// profile (which may contain a credential) into their state.
#[must_use]
pub fn load_workspace_profile_write_token(
    root: &Path,
) -> Option<WorkspaceProfileWriteToken> {
    load_workspace_profile_snapshot(root).write_token()
}

/// Load the workspace profile from `<root>/siralos.toml`. The workspace
/// record file is shared with `[plugins]` (decision 38/39), so this
/// reader validates only the `[profile]` subtree and treats a missing
/// file or missing `[profile]` table as [`WorkspaceProfileLoad::Absent`].
/// The file is lstat-verified as a regular file (symlinks and special
/// files are refused) and byte-bounded before parsing.
#[must_use]
pub fn load_workspace_profile(root: &Path) -> WorkspaceProfileLoad {
    let path = root.join(SIRALOS_TOML_FILE_NAME);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkspaceProfileLoad::Absent;
        }
        Err(_error) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml is unreadable".to_owned(),
            };
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return WorkspaceProfileLoad::Invalid {
            diagnostic: "siralos.toml must be a regular file; refusing symlink or special file".to_owned(),
        };
    }
    let bytes = match read_complete_file_bounded(&path, MAX_SIRALOS_TOML_BYTES)
    {
        BoundedFileRead::Complete(bytes) => bytes,
        BoundedFileRead::TooLarge => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: format!(
                    "siralos.toml exceeds the {MAX_SIRALOS_TOML_BYTES}-byte bound."
                ),
            };
        }
        BoundedFileRead::NotReadable => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml must be a regular file; refusing symlink or special file".to_owned(),
            };
        }
        BoundedFileRead::IoError(_error) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml is unreadable".to_owned(),
            };
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml is not valid UTF-8.".to_owned(),
            };
        }
    };
    load_workspace_profile_text(&text)
}

/// Parse exact already-read profile bytes without touching the filesystem.
/// Writers use this for staged-file verification so credential-bearing bytes
/// are never copied into a second temporary directory.
#[must_use]
pub fn parse_workspace_profile_bytes(bytes: &[u8]) -> WorkspaceProfileLoad {
    if bytes.len() > MAX_PROFILE_DOCUMENT_BYTES {
        return WorkspaceProfileLoad::Invalid {
            diagnostic: format!(
                "siralos.toml exceeds the {MAX_SIRALOS_TOML_BYTES}-byte bound."
            ),
        };
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => load_workspace_profile_text(&text),
        Err(_) => WorkspaceProfileLoad::Invalid {
            diagnostic: "siralos.toml is not valid UTF-8.".to_owned(),
        },
    }
}

/// composition/reload seam; callers retain the returned digest and use this
/// same object for all decisions instead of re-reading a second revision.
#[must_use]
pub fn load_workspace_profile_snapshot(
    root: &Path,
) -> WorkspaceProfileSnapshot {
    let path = root.join(SIRALOS_TOML_FILE_NAME);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return WorkspaceProfileSnapshot {
                load: WorkspaceProfileLoad::Absent,
                raw_sha256: siralos_core::identity::sha256_hex(&[]),
                raw_len: 0,
                present: false,
                file_identity: None,
                bindable: true,
                write_authority: Arc::new(AtomicBool::new(false)),
            };
        }
        Err(_error) => {
            return WorkspaceProfileSnapshot {
                load: WorkspaceProfileLoad::Invalid {
                    diagnostic: "siralos.toml is unreadable".to_owned(),
                },
                raw_sha256: siralos_core::identity::sha256_hex(&[]),
                raw_len: 0,
                present: true,
                file_identity: None,
                bindable: false,
                write_authority: Arc::new(AtomicBool::new(false)),
            };
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return WorkspaceProfileSnapshot {
            load: WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml must be a regular file; refusing symlink or special file".to_owned(),
            },
            raw_sha256: siralos_core::identity::sha256_hex(&[]),
            raw_len: 0,
            present: true,
            file_identity: None,
            bindable: false,
            write_authority: Arc::new(AtomicBool::new(false)),
        };
    }
    let file_identity = profile_file_identity(&metadata);
    let bytes = match read_complete_file_bounded(&path, MAX_SIRALOS_TOML_BYTES)
    {
        BoundedFileRead::Complete(bytes) => bytes,
        BoundedFileRead::TooLarge => {
            return WorkspaceProfileSnapshot {
                load: WorkspaceProfileLoad::Invalid {
                    diagnostic: format!(
                        "siralos.toml exceeds the {MAX_SIRALOS_TOML_BYTES}-byte bound."
                    ),
                },
                raw_sha256: String::new(),
                raw_len: 0,
                present: true,
                file_identity: None,
                bindable: false,
                write_authority: Arc::new(AtomicBool::new(false)),
            };
        }
        BoundedFileRead::NotReadable => {
            return WorkspaceProfileSnapshot {
                load: WorkspaceProfileLoad::Invalid {
                    diagnostic: "siralos.toml must be a regular file; refusing symlink or special file".to_owned(),
                },
                raw_sha256: String::new(),
                raw_len: 0,
                present: true,
                file_identity: None,
                bindable: false,
                write_authority: Arc::new(AtomicBool::new(false)),
            };
        }
        BoundedFileRead::IoError(_error) => {
            return WorkspaceProfileSnapshot {
                load: WorkspaceProfileLoad::Invalid {
                    diagnostic: "siralos.toml is unreadable".to_owned(),
                },
                raw_sha256: String::new(),
                raw_len: 0,
                present: true,
                file_identity: None,
                bindable: false,
                write_authority: Arc::new(AtomicBool::new(false)),
            };
        }
    };
    let final_metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata)
            if !metadata.file_type().is_symlink() && metadata.is_file() =>
        {
            metadata
        }
        _ => {
            return WorkspaceProfileSnapshot {
                load: WorkspaceProfileLoad::Invalid {
                    diagnostic: "siralos.toml changed while being read"
                        .to_owned(),
                },
                raw_sha256: String::new(),
                raw_len: 0,
                present: true,
                file_identity: None,
                bindable: false,
                write_authority: Arc::new(AtomicBool::new(false)),
            };
        }
    };
    let final_identity = profile_file_identity(&final_metadata);
    if file_identity != final_identity {
        return WorkspaceProfileSnapshot {
            load: WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml changed while being read".to_owned(),
            },
            raw_sha256: String::new(),
            raw_len: 0,
            present: true,
            file_identity: None,
            bindable: false,
            write_authority: Arc::new(AtomicBool::new(false)),
        };
    }
    let raw_len = bytes.len();
    let raw_sha256 = siralos_core::identity::sha256_hex(&bytes);
    let load = match String::from_utf8(bytes.clone()) {
        Ok(text) => load_workspace_profile_text(&text),
        Err(_) => WorkspaceProfileLoad::Invalid {
            diagnostic: "siralos.toml is not valid UTF-8.".to_owned(),
        },
    };
    let bindable = file_identity.is_some();
    WorkspaceProfileSnapshot {
        load,
        raw_sha256,
        raw_len,
        present: true,
        file_identity,
        bindable,
        write_authority: Arc::new(AtomicBool::new(false)),
    }
}

fn load_workspace_profile_text(text: &str) -> WorkspaceProfileLoad {
    if text.len() > MAX_PROFILE_DOCUMENT_BYTES {
        return WorkspaceProfileLoad::Invalid {
            diagnostic: format!(
                "siralos.toml exceeds the {MAX_PROFILE_DOCUMENT_BYTES}-byte bound."
            ),
        };
    }
    if text.trim().is_empty() {
        return WorkspaceProfileLoad::Absent;
    }
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(_) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: "siralos.toml does not parse".to_owned(),
            };
        }
    };
    let Some(profile) = value.get("profile") else {
        return WorkspaceProfileLoad::Absent;
    };
    match parse_profile_value(profile) {
        Ok(record) => WorkspaceProfileLoad::Record(record),
        Err(error) => {
            WorkspaceProfileLoad::Invalid { diagnostic: error.message }
        }
    }
}
/// Parse a `[profile]` TOML document into a validated `ProfileRecord`.
/// The record is re-validated by
/// `siralos_core::composition::resolve_profile_overlay`; this boundary
/// enforces the document shape and byte bounds.
///
/// # Errors
///
/// Returns `ProfileDocumentError` for oversize documents, TOML syntax
/// errors, unknown keys, malformed names/rules/capability ids, and entry
/// overflow.
pub fn parse_profile_document(
    raw: &str,
) -> Result<ProfileRecord, ProfileDocumentError> {
    if raw.len() > MAX_PROFILE_DOCUMENT_BYTES {
        return Err(error(format!(
            "The profile document exceeds the {MAX_PROFILE_DOCUMENT_BYTES}-byte bound."
        )));
    }
    let value: toml::Value = toml::from_str(raw)
        .map_err(|_| error("The profile document TOML syntax is invalid."))?;
    let root = value
        .as_table()
        .ok_or_else(|| error("The profile document must be a table."))?;
    for key in root.keys() {
        if key != "profile" {
            return Err(error("Unknown document field."));
        }
    }
    let Some(profile) = value.get("profile") else {
        return Err(error("The profile document requires a [profile] table."));
    };
    parse_profile_value(profile)
}

/// Parse the additive `[profile.context]` control (Stage 5.8, decision
/// 54): either the inline string form `context = "live"` or a
/// `[profile.context]` table with a bounded `kind` and an optional
/// `digest`. Validation delegates to the frozen 5.3 constructor, so a
/// malformed control makes the whole profile not applied (5.2
/// semantics). `live` must not carry a digest.
fn parse_context_control(
    control: &toml::Value,
) -> Result<ContextPolicy, ProfileDocumentError> {
    if let Some(kind) = control.as_str() {
        return ContextPolicy::new(kind, None)
            .map_err(|err| error(err.message));
    }
    let Some(table) = control.as_table() else {
        return Err(error(
            "The [profile.context] entry must be a string or a table.",
        ));
    };
    for key in table.keys() {
        if key != "kind" && key != "digest" {
            return Err(error("Unknown profile context field."));
        }
    }
    let Some(kind) = table.get("kind").and_then(toml::Value::as_str) else {
        return Err(error(
            "The [profile.context] entry requires a string kind.".to_owned(),
        ));
    };
    let digest = match table.get("digest") {
        None => None,
        Some(value) => {
            let Some(text) = value.as_str() else {
                return Err(error(
                    "The profile context digest must be a string.".to_owned(),
                ));
            };
            Some(text)
        }
    };
    if kind == "live" && digest.is_some() {
        return Err(error(
            "The live profile context control must not carry a digest."
                .to_owned(),
        ));
    }
    ContextPolicy::new(kind, digest).map_err(|err| error(err.message))
}
/// Validate a `[profile]` table value into a `ProfileRecord`. Shared by
/// [`parse_profile_document`] (full-document input) and
/// [`load_workspace_profile`] (the `[profile]` subtree of the shared
/// workspace `siralos.toml`).
///
/// # Errors
///
/// Returns `ProfileDocumentError` for unknown profile fields, malformed
/// names/rules/capability ids, and entry overflow.
pub fn parse_profile_value(
    profile: &toml::Value,
) -> Result<ProfileRecord, ProfileDocumentError> {
    let Some(profile_table) = profile.as_table() else {
        return Err(error("The [profile] entry must be a table."));
    };
    for key in profile_table.keys() {
        if key != "name"
            && key != "permissions"
            && key != "plugins"
            && key != "context"
            && key != "skills"
            && key != "provider"
            && key != "model"
            && key != "credential"
            && key != "endpoint"
            && key != "record-replay"
            && key != "replay"
            && key != "context_system"
            && key != "protocol"
            && key != "model_display_name"
        {
            return Err(error("Unknown profile field."));
        }
    }
    let Some(name) = profile.get("name").and_then(toml::Value::as_str) else {
        return Err(error("The [profile] table requires a string name."));
    };
    if name.len() > MAX_PROFILE_NAME_BYTES {
        // Bound and validity are distinct diagnostics: the bound case names the
        // limit, and neither message echoes the rejected name.
        return Err(error("The profile name exceeds the 64-byte bound."));
    }
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(error(
            "The profile name must be non-empty and control-free.",
        ));
    }
    let mut overlay = Vec::new();
    if let Some(permissions) = profile.get("permissions") {
        let Some(table) = permissions.as_table() else {
            return Err(error(
                "The [profile.permissions] entry must be a table.",
            ));
        };
        if table.len() > MAX_PROFILE_OVERLAY_ENTRIES {
            return Err(error(format!(
                "The profile exceeds the {MAX_PROFILE_OVERLAY_ENTRIES}-entry bound."
            )));
        }
        for (capability, rule) in table {
            let capability_id =
                CapabilityId::parse(capability).map_err(|_| {
                    error("The profile contains an invalid capability id.")
                })?;
            let Some(rule_text) = rule.as_str() else {
                return Err(error(
                    "The rule for a profile capability must be a string."
                        .to_owned(),
                ));
            };
            let requested = PermissionRule::parse(rule_text).ok_or_else(|| {
                error("The rule for a profile capability must be one of allow, ask, deny.")
            })?;
            overlay.push(ProfileOverlayEntry {
                capability: capability_id,
                requested,
            });
        }
    }
    let mut plugins: Option<Vec<String>> = None;
    if let Some(selection) = profile.get("plugins") {
        let Some(list) = selection.as_array() else {
            return Err(error(
                "The [profile.plugins] entry must be an array.",
            ));
        };
        let mut ids = Vec::new();
        if list.len() > MAX_PROFILE_PLUGIN_ENTRIES {
            return Err(error(
                "The [profile.plugins] selection exceeds its entry bound."
                    .to_owned(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in list {
            let Some(text) = id.as_str() else {
                return Err(error(
                    "Each profile plugin id must be a string.".to_owned(),
                ));
            };
            if text.is_empty()
                || text.len() > MAX_PROFILE_PLUGIN_ID_BYTES
                || text.chars().any(char::is_control)
                || !seen.insert(text)
            {
                return Err(error(
                    "Each profile plugin id must be a unique printable name."
                        .to_owned(),
                ));
            }
            ids.push(text.to_owned());
        }
        plugins = Some(ids);
    }
    let mut context: Option<ContextPolicy> = None;
    if let Some(control) = profile.get("context") {
        context = Some(parse_context_control(control)?);
    }
    let mut skills: Option<Vec<String>> = None;
    if let Some(selection) = profile.get("skills") {
        let Some(list) = selection.as_array() else {
            return Err(error("The [profile.skills] entry must be an array."));
        };
        let mut names = Vec::new();
        if list.len() > MAX_PROFILE_SKILL_ENTRIES {
            return Err(error(
                "The [profile.skills] selection exceeds its entry bound."
                    .to_owned(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for skill_name in list {
            let Some(text) = skill_name.as_str() else {
                return Err(error(
                    "Each profile skill name must be a string.".to_owned(),
                ));
            };
            if text.is_empty()
                || text.len() > MAX_PROFILE_SKILL_NAME_BYTES
                || text.chars().any(char::is_control)
                || !seen.insert(text)
            {
                return Err(error(
                    "Each profile skill name must be a unique printable name."
                        .to_owned(),
                ));
            }
            names.push(text.to_owned());
        }
        skills = Some(names);
    }
    let mut provider: Option<String> = None;
    if let Some(value) = profile.get("provider") {
        let Some(text) = value.as_str() else {
            return Err(error(
                "The [profile.provider] entry must be a string.".to_owned(),
            ));
        };
        provider = Some(text.to_owned());
    }
    let mut model: Option<String> = None;
    if let Some(value) = profile.get("model") {
        let Some(text) = value.as_str() else {
            return Err(error(
                "The [profile.model] entry must be a string.".to_owned(),
            ));
        };
        model = Some(text.to_owned());
    }
    let mut credential: Option<String> = None;
    if let Some(value) = profile.get("credential") {
        let Some(text) = value.as_str() else {
            return Err(error(
                "The [profile.credential] entry must be a string.".to_owned(),
            ));
        };
        credential = Some(text.to_owned());
    }
    let mut endpoint: Option<String> = None;
    if let Some(value) = profile.get("endpoint") {
        let Some(text) = value.as_str() else {
            return Err(error(
                "The [profile.endpoint] entry must be a string.".to_owned(),
            ));
        };
        endpoint = Some(text.to_owned());
    }
    let mut context_system_enabled = false;
    let mut record_replay = false;
    if let Some(value) = profile.get("record-replay") {
        let Some(flag) = value.as_bool() else {
            return Err(error(
                "The [profile.record-replay] entry must be a boolean."
                    .to_owned(),
            ));
        };
        record_replay = flag;
    }
    let mut replay = false;
    if let Some(value) = profile.get("replay") {
        let Some(flag) = value.as_bool() else {
            return Err(error(
                "The [profile.replay] entry must be a boolean.".to_owned(),
            ));
        };
        replay = flag;
    }
    if record_replay && replay {
        return Err(error(
            "The profile cannot set both record-replay and replay; they are contradictory.".to_owned(),
        ));
    }
    // Activation B3b (decision 99): the additive `[profile.context_system]`
    // table with exactly one key `enabled: bool`. Absent table -> default
    // false (byte-transparent, the session behavior matches a non-opted-in
    // session); a present table without a valid boolean `enabled` leaves the
    // whole profile unapplied (the established malformed-leaves-unapplied
    // pattern, decision 48 C3). Distinct from the decision 54
    // `[profile.context]` CONTROLS key; both may coexist.
    if let Some(value) = profile.get("context_system") {
        let Some(table) = value.as_table() else {
            return Err(error(
                "The [profile.context_system] entry must be a table.",
            ));
        };
        for key in table.keys() {
            if key != "enabled" {
                return Err(error(
                    "Unknown profile context_system field.".to_owned(),
                ));
            }
        }
        let Some(flag) = table.get("enabled").and_then(toml::Value::as_bool)
        else {
            return Err(error(
                "The [profile.context_system] table requires a boolean enabled.".to_owned(),
            ));
        };
        context_system_enabled = flag;
    }
    let mut protocol = siralos_core::composition::Protocol::default();
    if let Some(value) = profile.get("protocol") {
        let Some(text) = value.as_str() else {
            return Err(error(
                "The [profile.protocol] entry must be a string.".to_owned(),
            ));
        };
        let Some(parsed) = siralos_core::composition::Protocol::parse(text)
        else {
            return Err(error(
                "The [profile.protocol] entry must be \"openai-completions\", \"openai-responses\", or \"anthropic-messages\".".to_owned(),
            ));
        };
        protocol = parsed;
    }
    let mut model_display_name: Option<String> = None;
    if let Some(value) = profile.get("model_display_name") {
        let Some(text) = value.as_str() else {
            return Err(error(
                "The [profile.model_display_name] entry must be a string."
                    .to_owned(),
            ));
        };
        if text.len()
            > siralos_core::composition::MAX_PROFILE_MODEL_DISPLAY_NAME_BYTES
        {
            return Err(error(format!(
                "The model display name exceeds the {}-byte bound.",
                siralos_core::composition::MAX_PROFILE_MODEL_DISPLAY_NAME_BYTES
            )));
        }
        if text.contains('\0') {
            return Err(error(
                "A model display name must not contain NUL.".to_owned(),
            ));
        }
        if !text.chars().all(|c| !c.is_control()) {
            return Err(error(
                "A model display name must be printable.".to_owned(),
            ));
        }
        model_display_name = Some(text.to_owned());
    }
    Ok(ProfileRecord {
        name: name.to_owned(),
        overlay,
        plugins,
        context,
        skills,
        provider,
        model,
        credential,
        endpoint,
        record_replay,
        replay,
        context_system_enabled,
        protocol,
        model_display_name,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROFILE_DOCUMENT_BYTES, SIRALOS_TOML_FILE_NAME,
        WorkspaceProfileLoad, load_workspace_profile_snapshot,
        parse_profile_document,
    };
    use siralos_core::tool::permission::PermissionRule;

    fn workspace() -> std::path::PathBuf {
        // Nanosecond time alone can repeat across parallel test threads; the
        // counter makes each fixture directory unique within the process.
        static NEXT_NONCE: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence =
            NEXT_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "siralos-profile-tests-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    #[test]
    fn profile_debug_never_exposes_literal_credentials_or_endpoints() {
        let root = workspace();
        let document = "[profile]\nname = \"dev\"\ncredential = \"key:literal-secret\"\nendpoint = \"https://user:pass@example.com/v1?token=top-secret\"\n";
        std::fs::write(root.join(SIRALOS_TOML_FILE_NAME), document)
            .expect("profile document");
        let snapshot = load_workspace_profile_snapshot(&root);
        let snapshot_debug = format!("{snapshot:?}");
        let load_debug = match snapshot.load() {
            WorkspaceProfileLoad::Record(record) => format!("{record:?}"),
            other => panic!("expected record, got {other:?}"),
        };
        for rendered in [snapshot_debug, load_debug] {
            assert!(!rendered.contains("literal-secret"), "{rendered}");
            assert!(!rendered.contains("top-secret"), "{rendered}");
            assert!(!rendered.contains("user:pass"), "{rendered}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn parses_a_valid_document() {
        let document = "\n[profile]\nname = \"dev\"\n\n[profile.permissions]\n\"tool.workspace.read\" = \"ask\"\n\"tool.workspace.search\" = \"deny\"\n";
        let record = parse_profile_document(document).expect("valid");
        assert_eq!(record.name, "dev");
        assert_eq!(record.overlay.len(), 2);
        assert_eq!(record.overlay[0].requested, PermissionRule::Ask);
        assert_eq!(record.overlay[1].requested, PermissionRule::Deny);
    }

    #[test]
    fn parses_the_skills_selection_forms() {
        let document =
            "\n[profile]\nname = \"dev\"\nskills = [\"alpha\", \"guest\"]\n";
        let record = parse_profile_document(document).expect("valid");
        assert_eq!(
            record.skills,
            Some(vec!["alpha".to_owned(), "guest".to_owned()])
        );
        let absent = parse_profile_document("\n[profile]\nname = \"dev\"\n")
            .expect("valid");
        assert_eq!(absent.skills, None);
    }

    #[test]
    fn rejects_malformed_skills_selections() {
        let non_array = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nskills = \"alpha\"\n",
        );
        assert!(non_array.is_err());
        let non_string = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nskills = [\"alpha\", 7]\n",
        );
        assert!(non_string.is_err());
    }
    #[test]
    fn parses_the_context_control_forms() {
        let bound = "a".repeat(64);
        let inline = "\n[profile]\nname = \"dev\"\ncontext = \"live\"\n";
        let record = parse_profile_document(inline).expect("valid");
        assert!(matches!(
            record.context,
            Some(siralos_core::context::ContextPolicy::Live)
        ));
        let tabled = format!(
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"pinned\"\ndigest = \"{bound}\"\n",
        );
        let record = parse_profile_document(&tabled).expect("valid");
        assert!(matches!(
            record.context,
            Some(siralos_core::context::ContextPolicy::Pinned { .. })
        ));
        let absent = "\n[profile]\nname = \"dev\"\n";
        let record = parse_profile_document(absent).expect("valid");
        assert_eq!(record.context, None);
    }

    #[test]
    fn rejects_malformed_context_controls() {
        let bound = "a".repeat(64);
        for document in [
            "\n[profile]\nname = \"dev\"\ncontext = \"wat\"\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"pinned\"\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"live\"\ndigest = \"x\"\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"pinned\"\ndigest = \"zz\"\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"pinned\"\ndigest = \"x\"\nwat = 1\n",
            "\n[profile]\nname = \"dev\"\ncontext = 7\n",
            format!(
                "\n[profile]\nname = \"dev\"\n\n[profile.context]\nkind = \"frozen\"\ndigest = \"{bound}\"\nsurprise = true\n",
            )
                .as_str(),
        ] {
            let error = parse_profile_document(document)
                .expect_err("malformed control refused");
            assert!(!error.message.is_empty());
        }
    }
    #[test]
    fn rejects_unknown_fields_and_bad_rules() {
        let error =
            parse_profile_document("[profile]\nname = \"x\"\nextra = 1\n")
                .expect_err("unknown field refused");
        assert!(error.message.contains("Unknown profile field"));
        let error = parse_profile_document(
            "[profile]\nname = \"x\"\n\n[profile.permissions]\n\"tool.a\" = \"grant\"\n",
        )
        .expect_err("bad rule refused");
        assert!(error.message.contains("allow, ask, deny"));
        let error = parse_profile_document("[other]\nname = \"x\"\n")
            .expect_err("missing table refused");
        assert!(error.message.contains("Unknown document field"));
    }

    #[test]
    fn enforces_byte_bounds() {
        let name = "a".repeat(65);
        let document = format!("[profile]\nname = \"{name}\"\n");
        let error =
            parse_profile_document(&document).expect_err("name refused");
        // The bound case names the limit and never the rejected name; the
        // control-free case is a distinct diagnostic.
        assert!(error.message.contains("64-byte bound"));
        assert!(!error.message.contains(&name));
        let control = "[profile]\nname = \"a\\u0001b\"\n";
        let error = parse_profile_document(control)
            .expect_err("control character refused");
        assert!(error.message.contains("control-free"));
        let oversized = format!(
            "[profile]\nname = \"big\"\n\n[profile.permissions]\n\"c.x\" = \"deny\"\n\n[padding]\nvalue = \"{}\"\n",
            "p".repeat(MAX_PROFILE_DOCUMENT_BYTES)
        );
        let error = parse_profile_document(&oversized)
            .expect_err("document size refused");
        assert!(error.message.contains("document exceeds"));
    }

    #[test]
    fn loads_a_valid_workspace_profile() {
        let root = workspace();
        std::fs::write(
            root.join("siralos.toml"),
            "\n[profile]\nname = \"dev\"\n\n[profile.permissions]\n\"tool.workspace.read\" = \"ask\"\n\n[plugins]\n\n[plugins.guest]\npath = \"guest.wasm\"\ndigest = \"aa\"\n",
        )
        .expect("write record");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Record(record) = load else {
            panic!("expected a record, got {load:?}");
        };
        assert_eq!(record.name, "dev");
        assert_eq!(record.overlay.len(), 1);
    }

    #[test]
    fn profile_plugins_parse_and_validate() {
        let document = r#"
[profile]
name = "dev"
plugins = ["guest", "alpha"]

[profile.permissions]
"tool.workspace.read" = "ask"
"#;
        let record = parse_profile_document(document).expect("parsed");
        assert_eq!(
            record.plugins,
            Some(vec!["guest".to_owned(), "alpha".to_owned()])
        );
        let duplicate = r#"
[profile]
name = "dev"
plugins = ["guest", "guest"]
"#;
        let error = parse_profile_document(duplicate)
            .expect_err("dup refused at parse");
        assert!(
            error.message.contains("unique printable name"),
            "{}",
            error.message
        );
        let malformed = r#"
[profile]
name = "dev"
plugins = [7]
"#;
        let error = parse_profile_document(malformed).expect_err("type");
        assert!(error.message.contains("must be a string"));
        let unknown = r#"
[profile]
name = "dev"
widgets = ["x"]
"#;
        let error = parse_profile_document(unknown).expect_err("unknown");
        assert!(error.message.contains("Unknown profile field"));
    }

    #[test]
    fn absent_when_no_file_or_no_profile_table() {
        let root = workspace();
        assert_eq!(
            super::load_workspace_profile(&root),
            super::WorkspaceProfileLoad::Absent,
        );
        std::fs::write(root.join("siralos.toml"), "[other]\nx = 1\n")
            .expect("write record");
        assert_eq!(
            super::load_workspace_profile(&root),
            super::WorkspaceProfileLoad::Absent,
        );
    }

    #[test]
    fn invalid_documents_are_typed_not_applied() {
        let root = workspace();
        std::fs::write(
            root.join("siralos.toml"),
            "[profile]\nname = \"x\"\nextra = 1\n",
        )
        .expect("write record");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid, got {load:?}");
        };
        assert!(diagnostic.contains("Unknown profile field"));
        std::fs::write(root.join("siralos.toml"), "not toml [[[")
            .expect("write record");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid, got {load:?}");
        };
        assert!(diagnostic.contains("does not parse"));
    }

    #[test]
    fn context_system_parses_and_malformed_leaves_unapplied() {
        // Absent table -> default off (byte-transparent).
        let absent = parse_profile_document("\n[profile]\nname = \"dev\"\n")
            .expect("absent");
        assert!(!absent.context_system_enabled);
        // enabled = true -> on; enabled = false -> off.
        let on = parse_profile_document(
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = true\n",
        )
        .expect("on");
        assert!(on.context_system_enabled);
        let off = parse_profile_document(
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = false\n",
        )
        .expect("off");
        assert!(!off.context_system_enabled);
        // A present table without a valid boolean enabled leaves the whole
        // profile unapplied (parsing fails -> Invalid, decision 48 C3).
        for document in [
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = \"yes\"\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = 1\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\nother = true\n",
            "\n[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = true\nsurprise = 1\n",
            "\n[profile]\nname = \"dev\"\ncontext_system = true\n",
        ] {
            let error = parse_profile_document(document)
                .expect_err("malformed context_system refused");
            assert!(!error.message.is_empty());
        }
    }

    #[test]
    fn context_system_coexists_with_decision_54_controls() {
        // Both the decision 54 [profile.context] CONTROLS key and the
        // decision 99 [profile.context_system] opt-in may coexist.
        let document = "\n[profile]\nname = \"dev\"\ncontext = \"live\"\n\n[profile.context_system]\nenabled = true\n";
        let record = parse_profile_document(document).expect("coexist");
        assert!(matches!(
            record.context,
            Some(siralos_core::context::ContextPolicy::Live)
        ));
        assert!(record.context_system_enabled);
        // Also via workspace load -> profile carries the opt-in.
        let root = workspace();
        std::fs::write(
            root.join("siralos.toml"),
            "[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = true\n",
        )
        .expect("write");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Record(record) = load else {
            panic!("expected a record, got {load:?}");
        };
        assert!(record.context_system_enabled);
        // And via load with a malformed context_system -> Invalid (not applied).
        std::fs::write(
            root.join("siralos.toml"),
            "[profile]\nname = \"dev\"\n\n[profile.context_system]\nenabled = \"x\"\n",
        )
        .expect("write");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid, got {load:?}");
        };
        assert!(diagnostic.contains("boolean enabled"));
    }

    #[test]
    fn replay_flags_parse_and_validate() {
        // Valid booleans.
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nrecord-replay = true\n",
        )
        .expect("record-replay true");
        assert!(record.record_replay);
        assert!(!record.replay);
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nreplay = true\n",
        )
        .expect("replay true");
        assert!(record.replay);
        assert!(!record.record_replay);
        // Absent -> transparent false.
        let record = parse_profile_document("\n[profile]\nname = \"dev\"\n")
            .expect("absent");
        assert!(!record.record_replay);
        assert!(!record.replay);
        // Malformed string -> whole profile unapplied (Invalid via parse error).
        let malformed = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nreplay = \"true\"\n",
        );
        assert!(malformed.is_err());
        assert!(malformed.unwrap_err().message.contains("must be a boolean"));
        let malformed = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nrecord-replay = \"true\"\n",
        );
        assert!(malformed.is_err());
        // Both true -> contradictory, profile unapplied.
        let both = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nrecord-replay = true\nreplay = true\n",
        );
        assert!(both.is_err());
        assert!(both.unwrap_err().message.contains("both record-replay"));
        // Also via workspace load -> Invalid.
        let root = workspace();
        std::fs::write(
            root.join("siralos.toml"),
            "[profile]\nname = \"dev\"\nreplay = \"true\"\n",
        )
        .expect("write");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid");
        };
        assert!(diagnostic.contains("must be a boolean"));
        std::fs::write(
            root.join("siralos.toml"),
            "[profile]\nname = \"dev\"\nrecord-replay = true\nreplay = true\n",
        )
        .expect("write");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid");
        };
        assert!(diagnostic.contains("both record-replay"));
    }

    #[test]
    fn profile_parse_protocol_matrix() {
        // Present: openai-completions
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"openai-completions\"\n",
        )
        .expect("openai-completions");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::OpenAiCompletions
        );
        // Present: openai-responses
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"openai-responses\"\n",
        )
        .expect("openai-responses");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::OpenAiResponses
        );
        // Present: anthropic-messages
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"anthropic-messages\"\n",
        )
        .expect("anthropic-messages");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::AnthropicMessages
        );
        // Legacy alias: openai-compatible -> OpenAiCompletions
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"openai-compatible\"\n",
        )
        .expect("openai-compatible legacy");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::OpenAiCompletions
        );
        // Legacy alias: anthropic -> AnthropicMessages
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"anthropic\"\n",
        )
        .expect("anthropic legacy");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::AnthropicMessages
        );
        // Absent -> default openai-completions
        let record = parse_profile_document("\n[profile]\nname = \"dev\"\n")
            .expect("absent");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::OpenAiCompletions
        );
        // Malformed: unknown protocol -> error (profile unapplied)
        let malformed = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"gopher\"\n",
        );
        assert!(malformed.is_err());
        assert!(malformed.unwrap_err().message.contains("protocol"));
        // Via workspace load -> Invalid
        let root = workspace();
        std::fs::write(
            root.join("siralos.toml"),
            "[profile]\nname = \"dev\"\nprotocol = \"gopher\"\n",
        )
        .expect("write");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid");
        };
        assert!(diagnostic.contains("protocol"));
    }

    #[test]
    fn profile_parse_model_display_matrix() {
        // Present
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nmodel_display_name = \"My GPT\"\n",
        )
        .expect("present");
        assert_eq!(record.model_display_name.as_deref(), Some("My GPT"));
        // Absent -> None
        let record = parse_profile_document("\n[profile]\nname = \"dev\"\n")
            .expect("absent");
        assert!(record.model_display_name.is_none());
        // Oversize -> error
        let long = "a".repeat(257);
        let malformed = parse_profile_document(&format!(
            "\n[profile]\nname = \"dev\"\nmodel_display_name = \"{long}\"\n"
        ));
        assert!(malformed.is_err());
        assert!(malformed.unwrap_err().message.contains("model display name"));
        // Via workspace load -> Invalid
        let root = workspace();
        std::fs::write(
            root.join("siralos.toml"),
            format!(
                "[profile]\nname = \"dev\"\nmodel_display_name = \"{long}\"\n"
            ),
        )
        .expect("write");
        let load = super::load_workspace_profile(&root);
        let super::WorkspaceProfileLoad::Invalid { diagnostic } = load else {
            panic!("expected invalid");
        };
        assert!(diagnostic.contains("model display name"));
    }

    #[test]
    fn write_includes_new_keys() {
        // Protocol omitted when default, display omitted when empty — tested via write_profile_config
        // Here we test parse round-trip: writing default should not include protocol key
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"openai-completions\"\n",
        )
        .expect("default");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::OpenAiCompletions
        );
        // Anthropic-messages is non-default and should be present
        let record = parse_profile_document(
            "\n[profile]\nname = \"dev\"\nprotocol = \"anthropic-messages\"\n",
        )
        .expect("anthropic-messages");
        assert_eq!(
            record.protocol,
            siralos_core::composition::Protocol::AnthropicMessages
        );
    }

    #[test]
    fn write_token_distinguishes_absent_from_empty_present() {
        let root = workspace();
        let absent = super::load_workspace_profile_write_token(&root)
            .expect("absent token");
        assert!(absent.matches_bytes(None));
        assert!(!absent.matches_bytes(Some(&[])));

        std::fs::write(root.join("siralos.toml"), b"").expect("empty profile");
        let empty = super::load_workspace_profile_write_token(&root)
            .expect("empty-present token");
        assert!(empty.matches_bytes(Some(&[])));
        assert!(!empty.matches_bytes(None));
    }

    #[test]
    fn write_token_rejects_aba_identity_and_is_one_shot() {
        let root = workspace();
        let path = root.join("siralos.toml");
        let original = b"[profile]\nname = \"dev\"\nmodel = \"a\"\n";
        std::fs::write(&path, original).expect("write original");
        let snapshot = super::load_workspace_profile_snapshot(&root);
        let token = snapshot.write_token().expect("original token");
        let sibling = snapshot.write_token().expect("sibling token");
        let cloned_snapshot = snapshot.clone();
        let cloned_sibling =
            cloned_snapshot.write_token().expect("cloned token");
        assert!(token.matches_path(&path, Some(original)));

        // A transient different revision followed by byte-identical A is
        // still a different filesystem observation. Recreating the path gives
        // the portable identity evidence a deterministic A→B→A case.
        std::fs::write(&path, b"[profile]\nname = \"dev\"\nmodel = \"b\"\n")
            .expect("write intervening");
        std::fs::remove_file(&path).expect("remove intervening");
        std::fs::write(&path, original).expect("restore original bytes");
        assert!(
            !token.matches_path(&path, Some(original)),
            "content equality must not resurrect an ABA-stale write authority"
        );

        assert!(token.consume().is_ok());
        assert!(sibling.consume().is_err());
        assert!(cloned_sibling.consume().is_err());

        let one_shot = super::load_workspace_profile_write_token(&root)
            .expect("fresh token");
        assert!(one_shot.consume().is_ok());
        assert!(one_shot.consume().is_err());
    }
}
