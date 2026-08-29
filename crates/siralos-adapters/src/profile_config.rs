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
use siralos_core::tool::capability::CapabilityId;
use siralos_core::tool::permission::PermissionRule;
use std::path::Path;

/// Maximum complete profile-document size in UTF-8 bytes.
pub const MAX_PROFILE_DOCUMENT_BYTES: usize = 16 * 1024;

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
        Err(error) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: format!("siralos.toml is unreadable: {error}"),
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
        BoundedFileRead::IoError(error) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: format!("siralos.toml is unreadable: {error}"),
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

fn load_workspace_profile_text(text: &str) -> WorkspaceProfileLoad {
    if text.trim().is_empty() {
        return WorkspaceProfileLoad::Absent;
    }
    let value: toml::Value = match toml::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return WorkspaceProfileLoad::Invalid {
                diagnostic: format!(
                    "siralos.toml does not parse: {}",
                    error.message()
                ),
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
    let value: toml::Value =
        toml::from_str(raw).map_err(|err| error(err.message()))?;
    let root = value
        .as_table()
        .ok_or_else(|| error("The profile document must be a table."))?;
    for key in root.keys() {
        if key != "profile" {
            return Err(error(format!("Unknown document field {key:?}.")));
        }
    }
    let Some(profile) = value.get("profile") else {
        return Err(error("The profile document requires a [profile] table."));
    };
    parse_profile_value(profile)
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
        if key != "name" && key != "permissions" {
            return Err(error(format!("Unknown profile field {key:?}.")));
        }
    }
    let Some(name) = profile.get("name").and_then(toml::Value::as_str) else {
        return Err(error("The [profile] table requires a string name."));
    };
    if name.len() > MAX_PROFILE_NAME_BYTES {
        return Err(error(format!(
            "The profile name exceeds the {MAX_PROFILE_NAME_BYTES}-byte bound."
        )));
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
            let capability_id = CapabilityId::parse(capability)
                .map_err(|err| error(err.to_string()))?;
            let Some(rule_text) = rule.as_str() else {
                return Err(error(format!(
                    "The rule for capability {capability:?} must be a string."
                )));
            };
            let requested = PermissionRule::parse(rule_text).ok_or_else(|| {
                error(format!("The rule for capability {capability:?} must be one of allow, ask, deny."))
            })?;
            overlay.push(ProfileOverlayEntry {
                capability: capability_id,
                requested,
            });
        }
    }
    Ok(ProfileRecord { name: name.to_owned(), overlay })
}

#[cfg(test)]
mod tests {
    use super::{MAX_PROFILE_DOCUMENT_BYTES, parse_profile_document};
    use siralos_core::tool::permission::PermissionRule;

    fn workspace() -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("siralos-profile-tests-{nonce}"));
        std::fs::create_dir_all(&path).expect("temp root");
        path
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
        assert!(error.message.contains("byte bound"));
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
}
