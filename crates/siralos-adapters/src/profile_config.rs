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

use siralos_core::composition::{
    MAX_PROFILE_NAME_BYTES, MAX_PROFILE_OVERLAY_ENTRIES, ProfileOverlayEntry,
    ProfileRecord,
};
use siralos_core::tool::capability::CapabilityId;
use siralos_core::tool::permission::PermissionRule;

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
}
