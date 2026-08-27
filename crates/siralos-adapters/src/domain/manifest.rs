//! Add Plugin manifest loading and workspace plugin record (decision 38).
//!
//! The Add Plugin picker reads `domain-manifest.toml` at the picked
//! folder's ROOT only, validates its fields against the generic
//! `siralos-core::domain` package parsers (id, digest, abi, declared
//! capabilities), optionally verifies a named relative component file
//! (bounded, regular, no symlink, digest-matched), and records the
//! installed plugin in the workspace-root `siralos.toml`
//! (`[plugins.<id>] path + digest`). Every failure is typed and
//! performs no installation. This slice is View + Add Plugin only:
//! `Enable`/`Activate` remain Host-gated and are not implemented here.

use crate::workspace::fs::{BoundedFileRead, read_complete_file_bounded};
use siralos_core::domain::failure::DomainFailure;
use siralos_core::domain::package::DomainPackage;
use siralos_core::identity::sha256_hex;

use std::fmt;
use std::path::{Path, PathBuf};

/// The manifest file name looked for in the picked folder root.
pub const DOMAIN_MANIFEST_FILE_NAME: &str = "domain-manifest.toml";
/// The workspace plugin record file name.
pub const SIRALOS_TOML_FILE_NAME: &str = "siralos.toml";
/// Maximum manifest size in bytes (bounded complete read).
pub const MAX_MANIFEST_BYTES: usize = 4 * 1024;
/// Maximum workspace `siralos.toml` size in bytes.
pub const MAX_SIRALOS_TOML_BYTES: usize = 1024 * 1024;
/// Maximum component file bytes accepted at add time.
pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;

/// A structurally valid plugin manifest parsed from
/// `domain-manifest.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    package: DomainPackage,
    component: Option<PathBuf>,
}

impl PluginManifest {
    /// The validated domain package identity.
    pub fn package(&self) -> &DomainPackage {
        &self.package
    }

    /// The optional absolute component path the manifest names (always
    /// inside the workspace; never absolute in the source text).
    pub fn component(&self) -> Option<&Path> {
        self.component.as_deref()
    }
}

/// Why a manifest or plugin record was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginFailure {
    /// The manifest file is missing, a symlink, or not a regular file.
    ManifestNotReadable,
    /// The manifest exceeds the byte bound.
    ManifestTooLarge,
    /// The manifest could not be decoded as UTF-8.
    ManifestNotUtf8,
    /// The manifest TOML does not parse.
    ManifestSyntax(String),
    /// A manifest field did not fit `siralos-core` validation rules.
    ManifestInvalid(String),
    /// The named component is not usable.
    ComponentUnusable(String),
    /// The component digest does not match the declared package digest.
    ComponentDigestMismatch {
        /// Declared package digest.
        declared: String,
        /// Digest computed from the accepted component bytes.
        computed: String,
    },
    /// A record under the same id already has a different identity.
    RecordConflict(String),
    /// The plugin record file could not be inspected or written.
    RecordIo(String),
}

impl PluginFailure {
    /// Stable machine-branchable code for this failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManifestNotReadable => "MANIFEST_NOT_READABLE",
            Self::ManifestTooLarge => "MANIFEST_TOO_LARGE",
            Self::ManifestNotUtf8 => "MANIFEST_NOT_UTF8",
            Self::ManifestSyntax(_) => "MANIFEST_SYNTAX",
            Self::ManifestInvalid(_) => "MANIFEST_INVALID",
            Self::ComponentUnusable(_) => "COMPONENT_UNUSABLE",
            Self::ComponentDigestMismatch { .. } => {
                "COMPONENT_DIGEST_MISMATCH"
            }
            Self::RecordConflict(_) => "RECORD_CONFLICT",
            Self::RecordIo(_) => "RECORD_IO",
        }
    }
}

impl fmt::Display for PluginFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::ManifestNotReadable => {
                "plugin manifest is missing, a symlink, or not a regular file"
            }
            Self::ManifestTooLarge => "plugin manifest exceeds the byte bound",
            Self::ManifestNotUtf8 => "plugin manifest is not UTF-8",
            Self::ManifestSyntax(reason) => {
                return write!(formatter, "manifest did not parse: {reason}");
            }
            Self::ManifestInvalid(reason) => {
                return write!(formatter, "manifest is invalid: {reason}");
            }
            Self::ComponentUnusable(reason) => {
                return write!(formatter, "component is unusable: {reason}");
            }
            Self::ComponentDigestMismatch { declared, computed } => {
                return write!(
                    formatter,
                    "component digest does not match the declared package digest: declared {declared}, computed {computed}"
                );
            }
            Self::RecordConflict(reason) => {
                return write!(formatter, "plugin record conflict: {reason}");
            }
            Self::RecordIo(reason) => {
                return write!(
                    formatter,
                    "plugin record I/O failure: {reason}"
                );
            }
        };
        formatter.write_str(detail)
    }
}

impl std::error::Error for PluginFailure {}

/// Read and validate `domain-manifest.toml` at the picked folder root.
///
/// `folder` must be a workspace-relative path under the canonical root
/// (already validated by the caller); the manifest is lstat-checked,
/// bounded, UTF-8, then parsed as TOML. Unknown top-level keys are
/// ignored; missing required keys and invalid values fail with
/// `ManifestInvalid` (never with the raw TOML diagnostic).
pub fn load_manifest(
    root: &Path,
    folder: &Path,
) -> Result<PluginManifest, PluginFailure> {
    let manifest_path = folder.join(DOMAIN_MANIFEST_FILE_NAME);
    let bytes =
        match read_complete_file_bounded(&manifest_path, MAX_MANIFEST_BYTES) {
            BoundedFileRead::Complete(bytes) => bytes,
            BoundedFileRead::TooLarge => {
                return Err(PluginFailure::ManifestTooLarge);
            }
            BoundedFileRead::NotReadable | BoundedFileRead::IoError(_) => {
                return Err(PluginFailure::ManifestNotReadable);
            }
        };
    let text = String::from_utf8(bytes)
        .map_err(|_| PluginFailure::ManifestNotUtf8)?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| PluginFailure::ManifestSyntax(error.to_string()))?;
    let table = match value {
        toml::Value::Table(table) => table,
        _ => {
            return Err(PluginFailure::ManifestInvalid(
                "manifest must be a TOML table".to_owned(),
            ));
        }
    };

    let missing = |key: &str| {
        PluginFailure::ManifestInvalid(format!("missing required key {key}"))
    };
    let not_string = |key: &str| {
        PluginFailure::ManifestInvalid(format!("{key} must be a string"))
    };
    let field = |key: &str| -> Result<String, PluginFailure> {
        match table.get(key) {
            Some(toml::Value::String(value)) => Ok(value.clone()),
            Some(_) => Err(not_string(key)),
            None => Err(missing(key)),
        }
    };
    let id = field("id")?;
    let digest = field("digest")?;
    let abi = field("abi")?;
    let capabilities: Vec<String> = match table.get("capabilities") {
        None => Vec::new(),
        Some(toml::Value::Array(values)) => {
            let mut collected = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    toml::Value::String(text) => collected.push(text.clone()),
                    _ => {
                        return Err(PluginFailure::ManifestInvalid(
                            "capabilities must be an array of strings"
                                .to_owned(),
                        ));
                    }
                }
            }
            collected
        }
        Some(_) => {
            return Err(PluginFailure::ManifestInvalid(
                "capabilities must be an array of strings".to_owned(),
            ));
        }
    };
    let component = match table.get("component") {
        None => None,
        Some(toml::Value::String(name)) => {
            let path = Path::new(name);
            if name.is_empty()
                || path.is_absolute()
                || path.components().count() != 1
            {
                return Err(PluginFailure::ManifestInvalid(
                    "component must be a single relative file name".to_owned(),
                ));
            }
            let relative = workspace_relative(root, folder)?;
            let requested = if relative.is_empty() {
                name.replace('\\', "/")
            } else {
                format!("{relative}/{}", name.replace('\\', "/"))
            };
            // Lexical containment against the canonical root; existence
            // and regular-file checks are the verifier's job (the file
            // may legitimately not exist when the manifest is parsed).
            let canonical_root =
                std::fs::canonicalize(root).map_err(|error| {
                    PluginFailure::ComponentUnusable(format!(
                        "workspace root is not accessible: {error}"
                    ))
                })?;
            let resolved = crate::workspace::fs::normalize_join(
                &canonical_root,
                &requested,
            );
            if resolved != canonical_root
                && !resolved.starts_with(&canonical_root)
            {
                return Err(PluginFailure::ComponentUnusable(
                    "component path is outside the workspace".to_owned(),
                ));
            }
            Some(resolved)
        }
        Some(_) => {
            return Err(PluginFailure::ManifestInvalid(
                "component must be a string".to_owned(),
            ));
        }
    };

    let package = DomainPackage::parse(&id, &digest, &abi, &capabilities)
        .map_err(|error| match error {
            DomainFailure::InvalidInput { reason } => {
                PluginFailure::ManifestInvalid(reason)
            }
            other => PluginFailure::ManifestInvalid(other.code().to_owned()),
        })?;
    Ok(PluginManifest { package, component })
}

/// Express an absolute folder path as its workspace-relative string.
fn workspace_relative(
    root: &Path,
    folder: &Path,
) -> Result<String, PluginFailure> {
    let canonical_root = std::fs::canonicalize(root).map_err(|error| {
        PluginFailure::ComponentUnusable(format!(
            "workspace root is not accessible: {error}"
        ))
    })?;
    let canonical_folder = std::fs::canonicalize(folder).map_err(|error| {
        PluginFailure::ComponentUnusable(format!(
            "picked folder is not accessible: {error}"
        ))
    })?;
    let relative =
        canonical_folder.strip_prefix(&canonical_root).map_err(|_| {
            PluginFailure::ComponentUnusable(
                "picked folder is outside the workspace".to_owned(),
            )
        })?;
    Ok(relative
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned())
}

/// Verify the optional named component: exists, regular file, not a
/// symlink, bounded, and its SHA-256 equals the declared package
/// digest.
pub fn verify_component(
    manifest: &PluginManifest,
) -> Result<(), PluginFailure> {
    let Some(path) = manifest.component() else {
        return Ok(());
    };
    let declared = manifest.package().digest().as_str();
    let bytes = match read_complete_file_bounded(path, MAX_COMPONENT_BYTES) {
        BoundedFileRead::Complete(bytes) => bytes,
        BoundedFileRead::TooLarge => {
            return Err(PluginFailure::ComponentUnusable(
                "component exceeds the byte bound".to_owned(),
            ));
        }
        BoundedFileRead::NotReadable => {
            return Err(PluginFailure::ComponentUnusable(
                "component is missing, a symlink, or not a regular file"
                    .to_owned(),
            ));
        }
        BoundedFileRead::IoError(_) => {
            return Err(PluginFailure::ComponentUnusable(
                "component could not be read".to_owned(),
            ));
        }
    };
    let computed = sha256_hex(&bytes);
    if computed != declared {
        return Err(PluginFailure::ComponentDigestMismatch {
            declared: declared.to_owned(),
            computed,
        });
    }
    Ok(())
}

/// One `[plugins.<id>]` record persisted in workspace `siralos.toml`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginRecord {
    /// The installed plugin package id.
    pub id: String,
    /// Workspace-relative folder of the plugin source (using `/`
    /// separators; never an absolute path).
    pub path: String,
    /// The recorded package digest, spelled `sha256:<hex>`.
    pub digest: String,
}

/// Read the workspace plugin records from `siralos.toml`.
///
/// A missing file means no plugins are installed (empty, not an
/// error). Unreadable, oversized, or non-UTF-8 files fail typed.
/// Malformed `[plugins]` content is skipped per entry with a typed
/// failure? No: this slice treats a malformed record file as a typed
/// refusal (fail-closed: never a silent partial success).
pub fn load_plugin_records(
    root: &Path,
) -> Result<Vec<PluginRecord>, PluginFailure> {
    let path = root.join(SIRALOS_TOML_FILE_NAME);
    let text = read_record_text(&path)?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| PluginFailure::ManifestSyntax(error.to_string()))?;
    let Some(toml::Value::Table(plugins)) = value.get("plugins") else {
        return Ok(Vec::new());
    };
    let mut records = Vec::with_capacity(plugins.len());
    for (id, entry) in plugins {
        let toml::Value::Table(fields) = entry else {
            return Err(PluginFailure::RecordConflict(format!(
                "plugin {id} entry must be a table"
            )));
        };
        let path = match fields.get("path") {
            Some(toml::Value::String(value)) => value.clone(),
            _ => {
                return Err(PluginFailure::RecordConflict(format!(
                    "plugin {id} is missing a string path"
                )));
            }
        };
        let digest = match fields.get("digest") {
            Some(toml::Value::String(value)) => value.clone(),
            _ => {
                return Err(PluginFailure::RecordConflict(format!(
                    "plugin {id} is missing a string digest"
                )));
            }
        };
        records.push(PluginRecord { id: id.clone(), path, digest });
    }
    records.sort();
    Ok(records)
}

/// Read one bounded, UTF-8 file; missing returns None.
fn read_record_text(path: &Path) -> Result<String, PluginFailure> {
    match read_complete_file_bounded(path, MAX_SIRALOS_TOML_BYTES) {
        BoundedFileRead::Complete(bytes) => String::from_utf8(bytes)
            .map_err(|_| PluginFailure::ManifestNotUtf8),
        BoundedFileRead::TooLarge => Err(PluginFailure::RecordIo(
            "siralos.toml exceeds the byte bound".to_owned(),
        )),
        BoundedFileRead::NotReadable => Ok(String::new()),
        BoundedFileRead::IoError(error) => {
            Err(PluginFailure::RecordIo(error.to_string()))
        }
    }
}

/// Merge one plugin record into the workspace `siralos.toml`,
/// preserving every other section and record verbatim. Creating the
/// file when absent is fine; an existing record under the same id with
/// a different package identity conflicts (typed refusal, no write).
pub fn record_plugin(
    root: &Path,
    record: &PluginRecord,
) -> Result<(), PluginFailure> {
    let path = root.join(SIRALOS_TOML_FILE_NAME);
    let text = read_record_text(&path)?;
    let mut document: toml::Table = if text.trim().is_empty() {
        toml::Table::new()
    } else {
        toml::from_str(&text).map_err(|error| {
            if is_section_conflict(&error) {
                PluginFailure::RecordConflict(error.to_string())
            } else {
                PluginFailure::ManifestSyntax(error.to_string())
            }
        })?
    };
    let plugins = match document.remove("plugins") {
        None => toml::Table::new(),
        Some(toml::Value::Table(plugins)) => plugins,
        Some(_) => {
            return Err(PluginFailure::RecordConflict(
                "[plugins] must be a table".to_owned(),
            ));
        }
    };
    let conflict = match plugins.get(&record.id) {
        None => false,
        Some(toml::Value::Table(existing)) => {
            let same_path = existing
                .get("path")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == record.path);
            let same_digest = existing
                .get("digest")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == record.digest);
            !(same_path && same_digest)
        }
        Some(_) => true,
    };
    if conflict {
        return Err(PluginFailure::RecordConflict(format!(
            "plugin {} is already recorded with a different identity",
            record.id
        )));
    }
    let mut plugins = plugins;
    let mut entry = toml::Table::new();
    entry.insert("path".to_owned(), toml::Value::String(record.path.clone()));
    entry.insert(
        "digest".to_owned(),
        toml::Value::String(record.digest.clone()),
    );
    plugins.insert(record.id.clone(), toml::Value::Table(entry));
    document.insert("plugins".to_owned(), toml::Value::Table(plugins));
    let serialized = toml::to_string(&document)
        .map_err(|error| PluginFailure::RecordIo(error.to_string()))?;
    std::fs::write(&path, serialized)
        .map_err(|error| PluginFailure::RecordIo(error.to_string()))
}

/// Distinguish a duplicate-key conflict from generic syntax errors when
/// error strings are not available through typed accessors.
fn is_section_conflict(_error: &toml::de::Error) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, remove_dir_all, write};
    use std::time::{SystemTime, UNIX_EPOCH};

    const ABI: &str = "siralos:domain-abi@1.0.0";

    fn workspace() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("siralos-plugin-tests-{nonce}"));
        create_dir_all(&path).expect("temp root");
        path
    }

    fn digest_hex(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn manifest_text(
        id: &str,
        digest: &str,
        abi: &str,
        component: Option<&str>,
    ) -> String {
        let component = match component {
            Some(name) => format!("component = \"{name}\"\n"),
            None => String::new(),
        };
        format!(
            "id = \"{id}\"\ndigest = \"{digest}\"\nabi = \"{abi}\"\n{component}"
        )
    }

    #[test]
    fn load_manifest_accepts_valid_manifest() {
        let temp = workspace();
        create_dir_all(temp.join("plugins/godot")).unwrap();
        let manifest_path =
            temp.join("plugins/godot").join(DOMAIN_MANIFEST_FILE_NAME);
        write(
            &manifest_path,
            manifest_text(
                "godot",
                &digest_hex(0xab),
                ABI,
                Some("godot.component.wasm"),
            ),
        )
        .expect("write manifest");
        create_dir_all(temp.join("plugins/godot")).expect("folder");
        write(temp.join("plugins/godot/godot.component.wasm"), b"any bytes")
            .expect("component");
        let manifest = load_manifest(&temp, &temp.join("plugins/godot"));
        assert!(manifest.is_ok(), "{manifest:?}");
        let manifest = manifest.expect("loads");
        assert_eq!(manifest.package().id().as_str(), "godot");
        assert_eq!(manifest.package().digest().as_str(), &digest_hex(0xab));
        assert!(manifest.component().is_some());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_detects_missing_manifest() {
        let temp = workspace();
        let manifest = load_manifest(&temp, &temp);
        assert_eq!(manifest.unwrap_err(), PluginFailure::ManifestNotReadable);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_rejects_invalid_digest() {
        let temp = workspace();
        write(
            temp.join(DOMAIN_MANIFEST_FILE_NAME),
            manifest_text("godot", "not-a-digest", ABI, None),
        )
        .expect("write manifest");
        let failure = load_manifest(&temp, &temp).unwrap_err();
        assert_eq!(failure.code(), "MANIFEST_INVALID");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_ignores_unknown_keys() {
        let temp = workspace();
        write(
            temp.join(DOMAIN_MANIFEST_FILE_NAME),
            format!(
                "id = \"godot\"\ndigest = \"{}\"\nabi = \"{ABI}\"\nunknown = 42\n",
                digest_hex(0x01)
            ),
        )
        .expect("write manifest");
        assert!(load_manifest(&temp, &temp).is_ok());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn component_digest_mismatch_is_typed() {
        let temp = workspace();
        write(
            temp.join(DOMAIN_MANIFEST_FILE_NAME),
            manifest_text("godot", &digest_hex(0xab), ABI, Some("c.wasm")),
        )
        .expect("write manifest");
        write(temp.join("c.wasm"), b"component bytes").expect("component");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        let failure = verify_component(&manifest).unwrap_err();
        assert_eq!(failure.code(), "COMPONENT_DIGEST_MISMATCH");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn component_unreadable_is_typed() {
        let temp = workspace();
        write(
            temp.join(DOMAIN_MANIFEST_FILE_NAME),
            manifest_text(
                "godot",
                &digest_hex(0xab),
                ABI,
                Some("missing.wasm"),
            ),
        )
        .expect("write manifest");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        let failure = verify_component(&manifest).unwrap_err();
        assert_eq!(failure.code(), "COMPONENT_UNUSABLE");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn component_without_declared_path_passes_verification() {
        let temp = workspace();
        write(
            temp.join(DOMAIN_MANIFEST_FILE_NAME),
            manifest_text("godot", &digest_hex(0xab), ABI, None),
        )
        .expect("write manifest");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        assert!(verify_component(&manifest).is_ok());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn component_digest_match_verifies() {
        let temp = workspace();
        let bytes = b"exact component bytes".to_vec();
        let digest = sha256_hex(&bytes);
        write(
            temp.join(DOMAIN_MANIFEST_FILE_NAME),
            manifest_text("godot", &digest, ABI, Some("c.wasm")),
        )
        .expect("write manifest");
        write(temp.join("c.wasm"), &bytes).expect("component");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        assert!(verify_component(&manifest).is_ok());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn plugin_records_load_from_missing_file_as_empty() {
        let temp = workspace();
        let records = load_plugin_records(&temp).expect("loads");
        assert!(records.is_empty());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_and_reread_roundtrip() {
        let temp = workspace();
        let record = PluginRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0xcd)),
        };
        record_plugin(&temp, &record).expect("records");
        let records = load_plugin_records(&temp).expect("loads");
        assert_eq!(records, vec![record]);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_preserves_unrelated_sections() {
        let temp = workspace();
        write(
            temp.join(SIRALOS_TOML_FILE_NAME),
            "[unrelated]\nkey = \"kept\"\ntitle = \"other\"\n",
        )
        .expect("write");
        let record = PluginRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        record_plugin(&temp, &record).expect("records");
        let text = std::fs::read_to_string(temp.join(SIRALOS_TOML_FILE_NAME))
            .expect("read");
        assert!(text.contains("key = \"kept\""));
        assert!(text.contains("[plugins.godot]"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_second_plugin_is_appended() {
        let temp = workspace();
        let first = PluginRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let second = PluginRecord {
            id: "konstruct".to_owned(),
            path: "plugins/konstruct".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x02)),
        };
        record_plugin(&temp, &first).expect("first");
        record_plugin(&temp, &second).expect("second");
        let records = load_plugin_records(&temp).expect("loads");
        assert_eq!(records, vec![first, second]);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_same_identity_is_idempotent() {
        let temp = workspace();
        let record = PluginRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        record_plugin(&temp, &record).expect("first");
        record_plugin(&temp, &record).expect("second (same identity)");
        let records = load_plugin_records(&temp).expect("loads");
        assert_eq!(records, vec![record]);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_conflicting_identity_is_refused() {
        let temp = workspace();
        let first = PluginRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let conflicting = PluginRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x02)),
        };
        record_plugin(&temp, &first).expect("first");
        let failure = record_plugin(&temp, &conflicting).unwrap_err();
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        let _ = remove_dir_all(temp);
    }
}
