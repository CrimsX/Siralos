//! Add Module manifest loading and workspace module record (decision 38).
//!
//! The Add Module picker reads `module-manifest.toml` at the picked
//! folder's ROOT only, validates its fields against the generic
//! `siralos-core::module` package parsers (id, digest, abi, declared
//! capabilities), optionally verifies a named relative component file
//! (bounded, regular, no symlink, digest-matched), and records the
//! installed module in the workspace-root `siralos.toml`
//! (`[modules.<id>] path + digest`). Every failure is typed and
//! performs no installation. This slice is View + Add Module only:
//! `Enable`/`Activate` remain Host-gated and are not implemented here.
//!
//! Decision 114 Q4 — approved atomic-writer surfaces (documented):
//! the five approved temp+verify+rename writers are `siralos.lock`
//! (`crates/siralos-adapters/src/lockfile.rs` `write_workspace_lock`),
//! checkpoint storage (`crates/siralos-adapters/src/workspace/checkpoint.rs`),
//! replay store (`crates/siralos-adapters/src/replay_store.rs`
//! `write_replay_store`), module manifest records
//! (`crates/siralos-adapters/src/module/manifest.rs`
//! `write_record_document` at ~558–596), and the profile config
//! (`crates/siralos-cli/src/interactive.rs` `write_profile_config`
//! per decision 122 C2) — each with conflict/symlink refusal tests
//! (manifest: `record_conflict_is_refused`,
//! `crafted_record_with_absolute_path_is_refused` and symlink checks at
//! ~640–656, 572–580).

use crate::module::host::ModuleHost;
use crate::module::host::ModuleHostBounds;
use crate::workspace::fs::{
    BoundedFileRead, MUTATION_TEMP_PREFIX, is_model_protected_workspace_path,
    read_complete_file_bounded,
};
use siralos_core::identity::sha256_hex;
use siralos_core::module::capability::HostAuthority;
use siralos_core::module::failure::ModuleFailure as CoreModuleFailure;
use siralos_core::module::package::{ModulePackage, ModulePackageId};

const MAX_MODULE_RECORDS: usize = 128;
const MAX_MODULE_FIELD_BYTES: usize = 4096;

use std::fmt;
use std::path::{Path, PathBuf};

/// The manifest file name looked for in the picked folder root.
pub const MODULE_MANIFEST_FILE_NAME: &str = "module-manifest.toml";
/// The workspace module record file name.
pub const SIRALOS_TOML_FILE_NAME: &str = "siralos.toml";
/// Maximum manifest size in bytes (bounded complete read).
pub const MAX_MANIFEST_BYTES: usize = 4 * 1024;
/// Maximum workspace `siralos.toml` size in bytes.
pub const MAX_SIRALOS_TOML_BYTES: usize = 1024 * 1024;
/// Maximum component file bytes accepted at add time.
pub const MAX_COMPONENT_BYTES: usize = 16 * 1024 * 1024;
/// Maximum declared UI component specs per manifest (ADR 0039 §5).
pub const MAX_MANIFEST_UI_SPECS: usize = 8;

/// A structurally valid module manifest parsed from
/// `module-manifest.toml`.
#[derive(Clone, PartialEq, Eq)]
pub struct ModuleManifest {
    package: ModulePackage,
    component: Option<PathBuf>,
    ui_specs: Vec<siralos_core::ui::UiComponentSpec>,
}

impl fmt::Debug for ModuleManifest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModuleManifest")
            .field("package", &"validated")
            .field("component", &self.component.as_ref().map(|_| "<redacted>"))
            .field("ui_specs", &self.ui_specs.len())
            .finish()
    }
}

impl ModuleManifest {
    /// The validated module package identity.
    pub fn package(&self) -> &ModulePackage {
        &self.package
    }

    /// The optional absolute component path the manifest names (always
    /// inside the workspace; never absolute in the source text).
    pub fn component(&self) -> Option<&Path> {
        self.component.as_deref()
    }

    /// Validated module-declared UI component specs (ADR 0039 §5):
    /// data only, rendered by the Host when the module is enabled.
    pub fn ui_specs(&self) -> &[siralos_core::ui::UiComponentSpec] {
        &self.ui_specs
    }
}

/// Why a manifest or module record was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleManifestFailure {
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
    /// The module record file could not be inspected or written.
    RecordIo(String),
    /// No `siralos.toml` exists yet (a clean empty workspace).
    NoRecord,
}

impl ModuleManifestFailure {
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
            Self::NoRecord => "NO_RECORD",
        }
    }
}

impl fmt::Display for ModuleManifestFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let detail = match self {
            Self::ManifestNotReadable => {
                "module manifest is missing, a symlink, or not a regular file"
            }
            Self::ManifestTooLarge => "module manifest exceeds the byte bound",
            Self::ManifestNotUtf8 => "module manifest is not UTF-8",
            Self::ManifestSyntax(_) => "manifest did not parse",
            Self::ManifestInvalid(_) => "manifest is invalid",
            Self::ComponentUnusable(_) => "component is unusable",
            Self::ComponentDigestMismatch { declared, computed } => {
                return write!(
                    formatter,
                    "component digest does not match the declared package digest: declared {declared}, computed {computed}"
                );
            }
            Self::RecordConflict(_) => "module record conflict",
            Self::RecordIo(_) => "module record I/O failure",
            Self::NoRecord => {
                return formatter.write_str("no siralos.toml exists yet");
            }
        };
        formatter.write_str(detail)
    }
}

impl std::error::Error for ModuleManifestFailure {}

/// Read and validate `module-manifest.toml` at the picked folder root.
///
/// `folder` must be a workspace-relative path under `root`; the containment is
/// enforced here rather than trusted from the caller: an absolute or escaping
/// folder is refused, the canonical root and folder are compared after
/// canonicalization, and the manifest is lstat-checked, bounded, UTF-8, then
/// parsed as TOML. Unknown top-level keys are ignored; missing required keys
/// and invalid values fail with `ManifestInvalid` (never with the raw TOML
/// diagnostic).
pub fn load_manifest(
    root: &Path,
    folder: &Path,
) -> Result<ModuleManifest, ModuleManifestFailure> {
    if folder.as_os_str().is_empty()
        || folder.components().any(|component| {
            matches!(component, std::path::Component::ParentDir)
        })
    {
        return Err(ModuleManifestFailure::ManifestInvalid(
            "module folder must not escape the workspace".to_owned(),
        ));
    }
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|_error| ModuleManifestFailure::ManifestNotReadable)?;
    let joined = if folder.is_absolute() {
        folder.to_path_buf()
    } else {
        let Some(folder_text) = folder.to_str() else {
            return Err(ModuleManifestFailure::ManifestInvalid(
                "module folder must be valid UTF-8".to_owned(),
            ));
        };
        let joined =
            crate::workspace::fs::normalize_join(&canonical_root, folder_text);
        if joined != canonical_root && !joined.starts_with(&canonical_root) {
            return Err(ModuleManifestFailure::ManifestInvalid(
                "module folder is outside the workspace".to_owned(),
            ));
        }
        joined
    };
    let canonical_folder = std::fs::canonicalize(&joined)
        .map_err(|_error| ModuleManifestFailure::ManifestNotReadable)?;
    if canonical_folder != canonical_root
        && !canonical_folder.starts_with(&canonical_root)
    {
        return Err(ModuleManifestFailure::ManifestInvalid(
            "module folder resolves outside the workspace".to_owned(),
        ));
    }
    let manifest_path = canonical_folder.join(MODULE_MANIFEST_FILE_NAME);
    let bytes =
        match read_complete_file_bounded(&manifest_path, MAX_MANIFEST_BYTES) {
            BoundedFileRead::Complete(bytes) => bytes,
            BoundedFileRead::TooLarge => {
                return Err(ModuleManifestFailure::ManifestTooLarge);
            }
            BoundedFileRead::NotReadable | BoundedFileRead::IoError(_) => {
                return Err(ModuleManifestFailure::ManifestNotReadable);
            }
        };
    let text = String::from_utf8(bytes)
        .map_err(|_| ModuleManifestFailure::ManifestNotUtf8)?;
    let value: toml::Value = toml::from_str(&text).map_err(|_| {
        ModuleManifestFailure::ManifestSyntax(
            "module manifest TOML syntax is invalid".to_owned(),
        )
    })?;
    let table = match value {
        toml::Value::Table(table) => table,
        _ => {
            return Err(ModuleManifestFailure::ManifestInvalid(
                "manifest must be a TOML table".to_owned(),
            ));
        }
    };

    let missing = |key: &str| {
        ModuleManifestFailure::ManifestInvalid(format!(
            "missing required key {key}"
        ))
    };
    let not_string = |key: &str| {
        ModuleManifestFailure::ManifestInvalid(format!(
            "{key} must be a string"
        ))
    };
    let field = |key: &str| -> Result<String, ModuleManifestFailure> {
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
                        return Err(ModuleManifestFailure::ManifestInvalid(
                            "capabilities must be an array of strings"
                                .to_owned(),
                        ));
                    }
                }
            }
            collected
        }
        Some(_) => {
            return Err(ModuleManifestFailure::ManifestInvalid(
                "capabilities must be an array of strings".to_owned(),
            ));
        }
    };
    let component = match table.get("component") {
        None => None,
        Some(toml::Value::String(name)) => {
            let path = Path::new(name);
            if name.is_empty()
                || name.chars().any(char::is_control)
                || name.contains(':')
                || name.contains('\\')
                || path.is_absolute()
                || path.components().count() != 1
                || !siralos_core::workspace::path::validate_relative_path(name)
                    .is_ok()
            {
                return Err(ModuleManifestFailure::ManifestInvalid(
                    "component must be a single relative file name".to_owned(),
                ));
            }
            let relative = workspace_relative(root, folder)?;
            let requested = if relative.is_empty() {
                name.replace('\\', "/")
            } else {
                format!("{relative}/{}", name.replace('\\', "/"))
            };
            if is_model_protected_workspace_path(&requested) {
                return Err(ModuleManifestFailure::ManifestInvalid(
                    "component path is protected from model-facing inspection"
                        .to_owned(),
                ));
            }
            // Lexical containment against the canonical root; existence
            // and regular-file checks are the verifier's job (the file
            // may legitimately not exist when the manifest is parsed).
            let canonical_root =
                std::fs::canonicalize(root).map_err(|_| {
                    ModuleManifestFailure::ComponentUnusable(
                        "workspace root is not accessible".to_owned(),
                    )
                })?;
            let resolved = crate::workspace::fs::normalize_join(
                &canonical_root,
                &requested,
            );
            if resolved != canonical_root
                && !resolved.starts_with(&canonical_root)
            {
                return Err(ModuleManifestFailure::ComponentUnusable(
                    "component path is outside the workspace".to_owned(),
                ));
            }
            Some(resolved)
        }
        Some(_) => {
            return Err(ModuleManifestFailure::ManifestInvalid(
                "component must be a string".to_owned(),
            ));
        }
    };

    let package = ModulePackage::parse(&id, &digest, &abi, &capabilities)
        .map_err(|_| {
            ModuleManifestFailure::ManifestInvalid(
                "manifest package identity is invalid".to_owned(),
            )
        })?;
    let ui_specs = parse_manifest_ui_specs(&table)?;
    Ok(ModuleManifest { package, component, ui_specs })
}

/// Parse the optional `[ui]` manifest section into validated
/// component specs (ADR 0039 §5). Module input is hostile:
/// oversized, misshaped, or unknown-key sections are typed
/// refusals. Unknown TOP-LEVEL keys stay ignored (legacy
/// tolerance); inside the explicitly new `[ui]` section, unknown
/// entry keys refuse so typos cannot silently drop fields.
fn parse_manifest_ui_specs(
    table: &toml::Table,
) -> Result<Vec<siralos_core::ui::UiComponentSpec>, ModuleManifestFailure> {
    // TOML extraction stays per-surface: this reads the manifest
    // `ui.components` envelope with the `toml` crate, while the
    // workspace document reads `components` with `toml_edit` in
    // the cli. Only the validation rules are shared, in
    // `UiComponentSpec::parse`.
    use siralos_core::ui::{UiComponentSpec, UnvalidatedSpec};
    let invalid = |reason: &str| {
        ModuleManifestFailure::ManifestInvalid(format!(
            "manifest ui section invalid: {reason}"
        ))
    };
    let Some(ui) = table.get("ui") else {
        return Ok(Vec::new());
    };
    let ui = ui.as_table().ok_or_else(|| invalid("`ui` must be a table"))?;
    let Some(components) = ui.get("components") else {
        return Ok(Vec::new());
    };
    let components = components.as_array().ok_or_else(|| {
        invalid("`ui.components` must be an array of tables")
    })?;
    if components.len() > MAX_MANIFEST_UI_SPECS {
        return Err(invalid("too many ui components"));
    }
    let mut specs = Vec::new();
    for entry in components {
        let entry = entry
            .as_table()
            .ok_or_else(|| invalid("ui components must be tables"))?;
        for key in entry.keys() {
            match key.as_str() {
                "id" | "title" | "feed" | "enabled" | "placement"
                | "max_rows" | "max_cols" | "params" => {}
                _ => {
                    return Err(invalid("ui components carry unknown keys"));
                }
            }
        }
        let string = |key: &str| -> Result<String, ModuleManifestFailure> {
            match entry.get(key) {
                Some(toml::Value::String(value)) => Ok(value.clone()),
                _ => Err(invalid("ui component fields must be strings")),
            }
        };
        let params = match entry.get("params") {
            None => Vec::new(),
            Some(toml::Value::Table(table)) => {
                let mut collected = Vec::new();
                for (key, value) in table {
                    match value {
                        toml::Value::String(text) => {
                            collected.push((key.clone(), text.clone()));
                        }
                        _ => {
                            return Err(invalid(
                                "ui component params must be strings",
                            ));
                        }
                    }
                }
                collected
            }
            Some(_) => {
                return Err(invalid("ui component params must be a table"));
            }
        };
        let raw = UnvalidatedSpec {
            id: string("id")?,
            title: string("title")?,
            feed: string("feed")?,
            enabled: match entry.get("enabled") {
                None => true,
                Some(toml::Value::Boolean(flag)) => *flag,
                Some(_) => {
                    return Err(invalid(
                        "ui component `enabled` must be a boolean",
                    ));
                }
            },
            placement: string("placement")?,
            max_rows: match entry.get("max_rows") {
                None => siralos_core::ui::DEFAULT_SPEC_MAX_ROWS,
                Some(toml::Value::Integer(size)) => *size,
                Some(_) => {
                    return Err(invalid(
                        "ui component size hints must be integers",
                    ));
                }
            },
            max_cols: match entry.get("max_cols") {
                None => siralos_core::ui::DEFAULT_SPEC_MAX_COLS,
                Some(toml::Value::Integer(size)) => *size,
                Some(_) => {
                    return Err(invalid(
                        "ui component size hints must be integers",
                    ));
                }
            },
            params,
        };
        specs.push(UiComponentSpec::parse(&raw).map_err(|error| {
            invalid(&format!("ui component invalid: {}", error.message()))
        })?);
    }
    Ok(specs)
}

/// Express an absolute folder path as its workspace-relative string.
fn workspace_relative(
    root: &Path,
    folder: &Path,
) -> Result<String, ModuleManifestFailure> {
    let canonical_root = std::fs::canonicalize(root).map_err(|_| {
        ModuleManifestFailure::ComponentUnusable(
            "workspace root is not accessible".to_owned(),
        )
    })?;
    let canonical_folder = std::fs::canonicalize(folder).map_err(|_| {
        ModuleManifestFailure::ComponentUnusable(
            "picked folder is not accessible".to_owned(),
        )
    })?;
    let relative =
        canonical_folder.strip_prefix(&canonical_root).map_err(|_| {
            ModuleManifestFailure::ComponentUnusable(
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
    manifest: &ModuleManifest,
) -> Result<(), ModuleManifestFailure> {
    let Some(path) = manifest.component() else {
        return Ok(());
    };
    let declared = manifest.package().digest().as_str();
    if let Some(parent) = path.parent() {
        // The component name is lexically contained at parse time. Re-checking
        // that the parent is still a real directory narrows the window; it is
        // still a pathname check, so an ancestor substituted after
        // `load_manifest` canonicalized is NOT covered here. This is the
        // documented residual race, not a guarantee this code makes.
        match std::fs::symlink_metadata(parent) {
            Ok(metadata)
                if crate::workspace::fs::is_link_or_reparse(&metadata)
                    || !metadata.is_dir() =>
            {
                return Err(ModuleManifestFailure::ComponentUnusable(
                    "component directory must be a real directory".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(_) => {
                return Err(ModuleManifestFailure::ComponentUnusable(
                    "component directory is unavailable".to_owned(),
                ));
            }
        }
    }
    let bytes = match read_complete_file_bounded(path, MAX_COMPONENT_BYTES) {
        BoundedFileRead::Complete(bytes) => bytes,
        BoundedFileRead::TooLarge => {
            return Err(ModuleManifestFailure::ComponentUnusable(
                "component exceeds the byte bound".to_owned(),
            ));
        }
        BoundedFileRead::NotReadable => {
            return Err(ModuleManifestFailure::ComponentUnusable(
                "component is missing, a symlink, or not a regular file"
                    .to_owned(),
            ));
        }
        BoundedFileRead::IoError(_) => {
            return Err(ModuleManifestFailure::ComponentUnusable(
                "component could not be read".to_owned(),
            ));
        }
    };
    let computed = sha256_hex(&bytes);
    if computed != declared {
        return Err(ModuleManifestFailure::ComponentDigestMismatch {
            declared: declared.to_owned(),
            computed,
        });
    }
    Ok(())
}

/// One `[modules.<id>]` record persisted in workspace `siralos.toml`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleRecord {
    /// The installed module package id.
    pub id: String,
    /// Workspace-relative folder of the module source (using `/`
    /// separators; never an absolute path).
    pub path: String,
    /// The recorded package digest, spelled `sha256:<hex>`.
    pub digest: String,
}

impl fmt::Debug for ModuleRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModuleRecord")
            .field("id", &"<redacted>")
            .field("path", &"<redacted>")
            .field("digest", &self.digest)
            .finish()
    }
}

/// Install one loaded manifest through the production host boundary.
///
/// The host reads the exact component bytes (bounded, regular file,
/// no symlink) and verifies the declared digest itself; the lifecycle
/// transitions to `Installed`. Installation carries no authority gain:
/// the host starts with an empty authority, and `Enable`/`Activate`
/// remain separate Host-gated steps. When the manifest names no
/// component there are no bytes to verify, so the declared manifest
/// identity is recorded as-is (still typed, never silence).
pub fn install_module(
    manifest: &ModuleManifest,
    workspace_root: &Path,
    folder: &Path,
) -> Result<(), ModuleManifestFailure> {
    let record = ModuleRecord {
        id: manifest.package().id().as_str().to_owned(),
        path: workspace_relative(workspace_root, folder)?,
        digest: format!("sha256:{}", manifest.package().digest().as_str()),
    };
    let current_records = load_module_records(workspace_root)?;
    if current_records.iter().any(|existing| {
        existing.id == record.id
            && (existing.path != record.path
                || existing.digest != record.digest)
    }) {
        return Err(ModuleManifestFailure::RecordConflict(
            "module record conflict".to_owned(),
        ));
    }
    if let Some(component) = manifest.component() {
        let abi = manifest.package().abi().clone();
        let authority = HostAuthority::parse(&[]).map_err(|_| {
            ModuleManifestFailure::ManifestInvalid(
                "component authority is invalid".to_owned(),
            )
        })?;
        let mut host = ModuleHost::new(
            abi,
            authority,
            component.to_path_buf(),
            workspace_root.to_path_buf(),
            ModuleHostBounds::default(),
        );
        host.install(manifest.package().clone()).map_err(
            |error| match error {
                CoreModuleFailure::IdentityMismatch { .. } => {
                    ModuleManifestFailure::ComponentDigestMismatch {
                        declared: manifest
                            .package()
                            .digest()
                            .as_str()
                            .to_owned(),
                        computed: "mismatch".to_owned(),
                    }
                }
                _ => ModuleManifestFailure::ComponentUnusable(
                    "component installation failed".to_owned(),
                ),
            },
        )?;
        verify_component(manifest)?;
    }
    record_module(workspace_root, &record)
}

/// Read the workspace module records from `siralos.toml`.
///
/// A missing file means no modules are installed (empty, not an
/// error). Unreadable, oversized, or non-UTF-8 files fail typed.
/// Malformed `[modules]` content is skipped per entry with a typed
/// failure? No: this slice treats a malformed record file as a typed
/// refusal (fail-closed: never a silent partial success).
pub fn load_module_records(
    root: &Path,
) -> Result<Vec<ModuleRecord>, ModuleManifestFailure> {
    let path = root.join(SIRALOS_TOML_FILE_NAME);
    let Some(text) = read_record_text(&path)? else {
        return Ok(Vec::new());
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: toml::Value = toml::from_str(&text).map_err(|_| {
        ModuleManifestFailure::ManifestSyntax(
            "siralos.toml syntax is invalid".to_owned(),
        )
    })?;
    let Some(modules_value) = value.get("modules") else {
        return Ok(Vec::new());
    };
    let toml::Value::Table(modules) = modules_value else {
        return Err(ModuleManifestFailure::ManifestSyntax(
            "the [modules] entry must be a table".to_owned(),
        ));
    };
    if modules.len() > MAX_MODULE_RECORDS {
        return Err(ModuleManifestFailure::RecordConflict(
            "module record count exceeds the safety bound".to_owned(),
        ));
    }
    let mut records = Vec::with_capacity(modules.len());
    for (id, entry) in modules {
        let (path, digest) = module_record_fields(entry)?;
        validate_record(id, path, digest)?;
        records.push(ModuleRecord {
            id: id.clone(),
            path: path.to_owned(),
            digest: digest.to_owned(),
        });
    }
    records.sort();
    Ok(records)
}

/// Borrow the two required string fields from one stored record entry.
fn module_record_fields(
    entry: &toml::Value,
) -> Result<(&str, &str), ModuleManifestFailure> {
    let fields = entry.as_table().ok_or_else(|| {
        ModuleManifestFailure::RecordConflict(
            "module record entry must be a table".to_owned(),
        )
    })?;
    let path =
        fields.get("path").and_then(toml::Value::as_str).ok_or_else(|| {
            ModuleManifestFailure::RecordConflict(
                "module record requires a string path".to_owned(),
            )
        })?;
    let digest = fields
        .get("digest")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            ModuleManifestFailure::RecordConflict(
                "module record requires a string digest".to_owned(),
            )
        })?;
    Ok((path, digest))
}

/// Validate one record's bounded shape without resolving or rendering it.
fn validate_record(
    id: &str,
    path: &str,
    digest: &str,
) -> Result<(), ModuleManifestFailure> {
    ModulePackageId::parse(id).map_err(|_| {
        ModuleManifestFailure::RecordConflict(
            "module record id is invalid".to_owned(),
        )
    })?;
    let path_ok = path.len() <= MAX_MODULE_FIELD_BYTES
        && !path.chars().any(char::is_control)
        && !path.contains(':')
        && !path.contains('\\')
        && siralos_core::workspace::path::validate_relative_path(path).is_ok()
        && !is_model_protected_workspace_path(path);
    let digest_ok = digest.len() == "sha256:".len() + 64
        && digest.starts_with("sha256:")
        && is_hex64(&digest["sha256:".len()..]);
    if !path_ok || !digest_ok {
        return Err(ModuleManifestFailure::RecordConflict(
            "module record has an invalid path or digest".to_owned(),
        ));
    }
    Ok(())
}

fn validate_module_table(
    modules: &toml::Table,
) -> Result<(), ModuleManifestFailure> {
    if modules.len() > MAX_MODULE_RECORDS {
        return Err(ModuleManifestFailure::RecordConflict(
            "module record count exceeds the safety bound".to_owned(),
        ));
    }
    for (id, entry) in modules {
        let (path, digest) = module_record_fields(entry)?;
        validate_record(id, path, digest)?;
    }
    Ok(())
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Read one bounded, UTF-8 file. A missing file yields `None`; a
/// symlink or non-regular file is a typed refusal (the caller never
/// writes through a substituted pathname).
fn read_record_text(
    path: &Path,
) -> Result<Option<String>, ModuleManifestFailure> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(_error) => {
            return Err(ModuleManifestFailure::RecordIo(
                "siralos.toml is unreadable".to_owned(),
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ModuleManifestFailure::RecordConflict(
            "siralos.toml must be a regular file; refusing symlink or special file"
                .to_owned(),
        ));
    }
    match read_complete_file_bounded(path, MAX_SIRALOS_TOML_BYTES) {
        BoundedFileRead::Complete(bytes) => {
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| ModuleManifestFailure::ManifestNotUtf8)
        }
        BoundedFileRead::TooLarge => Err(ModuleManifestFailure::RecordIo(
            "siralos.toml exceeds the byte bound".to_owned(),
        )),
        BoundedFileRead::NotReadable => Err(ModuleManifestFailure::RecordConflict(
            "siralos.toml must be a regular file; refusing symlink or special file"
                .to_owned(),
        )),
        BoundedFileRead::IoError(_error) => Err(ModuleManifestFailure::RecordIo(
            "siralos.toml is unreadable".to_owned(),
        )),
    }
}

/// Write the module record document atomically against the exact source
/// revision. An existing target is committed only when its SHA-256 still
/// matches; an initially absent target must still be absent at commit.
///
/// The document is staged with [`crate::atomic::stage_atomic`] and swapped in
/// with [`crate::atomic::StagedWrite::commit_if_digest`] or
/// [`crate::atomic::StagedWrite::commit_if_absent`]. The target is never opened
/// for write: either commit refuses a symlink or special-file target before the
/// swap, so such a target is refused rather than followed or replaced. Any
/// failure removes the staged file.
fn write_record_document(
    root: &Path,
    document: &toml::Table,
    expected_digest: Option<&str>,
) -> Result<(), ModuleManifestFailure> {
    let serialized = toml::to_string(document).map_err(|_| {
        ModuleManifestFailure::RecordIo(
            "siralos.toml could not be serialized".to_owned(),
        )
    })?;
    if serialized.len() > MAX_SIRALOS_TOML_BYTES {
        return Err(ModuleManifestFailure::RecordConflict(
            "serialized module record document exceeds the byte bound"
                .to_owned(),
        ));
    }
    let staged = crate::atomic::stage_atomic(
        root,
        SIRALOS_TOML_FILE_NAME,
        &format!("{MUTATION_TEMP_PREFIX}siralos-toml"),
        serialized.as_bytes(),
        None,
    )
    .map_err(|error| match error {
        crate::atomic::AtomicWriteFailure::TargetIsNotARegularFile { .. } => {
            ModuleManifestFailure::RecordConflict(
                "siralos.toml must be a regular file; refusing symlink or special file"
                    .to_owned(),
            )
        }
        _ => ModuleManifestFailure::RecordIo(
            "siralos.toml could not be staged".to_owned(),
        ),
    })?;
    let commit = match expected_digest {
        Some(expected_digest) => staged.commit_if_digest(expected_digest),
        None => staged.commit_if_absent(),
    };
    commit.map_err(|error| match error {
        crate::atomic::AtomicWriteFailure::TargetIsNotARegularFile { .. } => {
            ModuleManifestFailure::RecordConflict(
                "siralos.toml must be a regular file; refusing symlink or special file"
                    .to_owned(),
            )
        }
        crate::atomic::AtomicWriteFailure::TargetChanged { .. } => {
            ModuleManifestFailure::RecordConflict(
                "siralos.toml changed before replacement".to_owned(),
            )
        }
        _ => ModuleManifestFailure::RecordIo(
            "siralos.toml could not be replaced".to_owned(),
        ),
    })?;
    Ok(())
}

/// Merge one module record into the workspace `siralos.toml`,
/// preserving every other section and record (structurally; comments
/// and original formatting are not preserved by the TOML round-trip).
/// The incoming record and existing module table must satisfy the record
/// count and field bounds before any merge or serialization. Creating the
/// file when absent is fine; an existing record under the same id with a
/// different package identity conflicts (typed refusal, no write).
pub fn record_module(
    root: &Path,
    record: &ModuleRecord,
) -> Result<(), ModuleManifestFailure> {
    validate_record(&record.id, &record.path, &record.digest)?;
    let path = root.join(SIRALOS_TOML_FILE_NAME);
    let source = read_record_text(&path)?;
    let expected_digest =
        source.as_ref().map(|text| sha256_hex(text.as_bytes()));
    let text = source.unwrap_or_default();
    let mut document: toml::Table = if text.trim().is_empty() {
        toml::Table::new()
    } else {
        toml::from_str(&text).map_err(|_| {
            // Fail closed on any parse error of a pre-existing file:
            // never silently rewrite an unparseable record file.
            ModuleManifestFailure::RecordConflict(
                "siralos.toml does not parse".to_owned(),
            )
        })?
    };
    // The root module-record table is independent from the `[profile]`
    // subtree; remove only this exact key before validating and merging it.
    let modules = match document.remove("modules") {
        None => toml::Table::new(),
        Some(toml::Value::Table(modules)) => modules,
        Some(_) => {
            return Err(ModuleManifestFailure::RecordConflict(
                "[modules] must be a table".to_owned(),
            ));
        }
    };
    validate_module_table(&modules)?;
    if modules.len() >= MAX_MODULE_RECORDS && !modules.contains_key(&record.id)
    {
        return Err(ModuleManifestFailure::RecordConflict(
            "module record count exceeds the safety bound".to_owned(),
        ));
    }
    let conflict = match modules.get(&record.id) {
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
        return Err(ModuleManifestFailure::RecordConflict(
            "module is already recorded with a different identity".to_owned(),
        ));
    }
    let mut modules = modules;
    let mut entry = toml::Table::new();
    entry.insert("path".to_owned(), toml::Value::String(record.path.clone()));
    entry.insert(
        "digest".to_owned(),
        toml::Value::String(record.digest.clone()),
    );
    modules.insert(record.id.clone(), toml::Value::Table(entry));
    document.insert("modules".to_owned(), toml::Value::Table(modules));
    write_record_document(root, &document, expected_digest.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, read, remove_dir_all, write};
    use std::time::{SystemTime, UNIX_EPOCH};

    const ABI: &str = "siralos:domain-abi@1.0.0";

    fn workspace() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("siralos-module-tests-{nonce}"));
        create_dir_all(&path).expect("temp root");
        path
    }

    fn digest_hex(byte: u8) -> String {
        format!("{byte:02x}").repeat(32)
    }

    fn document_with_module_count(count: usize) -> String {
        let mut document = String::from(
            "[profile]\nname = \"dev\"\nplugins = [\"selected\"]\n\n",
        );
        for index in 0..count {
            document.push_str(&format!(
                "[modules.module{index:03}]\npath = \"plugins/plugin{index:03}\"\ndigest = \"sha256:{}\"\n",
                digest_hex((index % 256) as u8),
            ));
        }
        document
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

    const UNSAFE_PLUGIN_RECORD_PATHS: &[&str] = &[
        "../private-plugin-marker",
        "plugins/../private-plugin-marker",
        "plugins/private-plugin-marker/..",
        "AGENTS.md",
        "nested/AGENTS.md",
        ".siralos/private-plugin-marker",
        "nested/.siralos/private-plugin-marker",
    ];

    fn module_record_document(path: &str) -> String {
        format!(
            "[modules.godot]\npath = \"{path}\"\ndigest = \"sha256:{}\"\n",
            digest_hex(0x42),
        )
    }

    fn assert_generic_record_rejections(
        outcomes: Vec<(&'static str, Result<(), ModuleManifestFailure>)>,
    ) {
        let observed = outcomes
            .iter()
            .map(|(path, result)| match result {
                Ok(()) => format!("{path}: accepted"),
                Err(failure) => {
                    format!("{path}: {failure} (code {})", failure.code())
                }
            })
            .collect::<Vec<_>>();
        assert!(
            outcomes.iter().all(|(_, result)| result.is_err()),
            "every unsafe module record path must be rejected: {observed:#?}",
        );

        for (path, result) in outcomes {
            let failure = result.expect_err("unsafe record path was rejected");
            assert_eq!(failure.code(), "RECORD_CONFLICT");
            let diagnostic = failure.to_string();
            for marker in
                ["..", "AGENTS.md", ".siralos", "private-plugin-marker"]
            {
                assert!(
                    !diagnostic.contains(marker),
                    "diagnostic echoed {marker:?} for {path:?}: {diagnostic}",
                );
            }
        }
    }

    #[test]
    fn load_manifest_accepts_valid_manifest() {
        let temp = workspace();
        create_dir_all(temp.join("plugins/godot")).unwrap();
        let manifest_path =
            temp.join("plugins/godot").join(MODULE_MANIFEST_FILE_NAME);
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
        let debug = format!("{manifest:?}");
        assert!(!debug.contains("godot"));
        assert!(debug.contains("<redacted>"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_refuses_folders_outside_the_workspace() {
        let temp = workspace();
        let outside = workspace();
        write(
            outside.join(MODULE_MANIFEST_FILE_NAME),
            manifest_text("godot", &digest_hex(0xab), ABI, None),
        )
        .expect("write manifest outside");
        // A caller cannot use the public loader to read a manifest that is not
        // inside the declared workspace root.
        let failure = load_manifest(&temp, &outside).unwrap_err();
        assert_eq!(failure.code(), "MANIFEST_INVALID");
        let escape =
            Path::new("..").join(outside.file_name().unwrap_or_default());
        let failure = load_manifest(&temp, &escape).unwrap_err();
        assert_eq!(failure.code(), "MANIFEST_INVALID");
        let _ = remove_dir_all(temp);
        let _ = remove_dir_all(outside);
    }

    #[test]
    fn load_manifest_detects_missing_manifest() {
        let temp = workspace();
        let manifest = load_manifest(&temp, &temp);
        assert_eq!(
            manifest.unwrap_err(),
            ModuleManifestFailure::ManifestNotReadable
        );
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_rejects_invalid_digest() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
            manifest_text("godot", "not-a-digest", ABI, None),
        )
        .expect("write manifest");
        let failure = load_manifest(&temp, &temp).unwrap_err();
        assert_eq!(failure.code(), "MANIFEST_INVALID");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn manifest_and_record_documents_reject_non_utf8() {
        let temp = workspace();
        std::fs::write(temp.join(MODULE_MANIFEST_FILE_NAME), [0xff])
            .expect("write manifest");
        assert_eq!(
            load_manifest(&temp, &temp).expect_err("manifest refused"),
            ModuleManifestFailure::ManifestNotUtf8,
        );

        std::fs::write(temp.join(SIRALOS_TOML_FILE_NAME), [0xff])
            .expect("write record document");
        assert_eq!(
            load_module_records(&temp).expect_err("record document refused"),
            ModuleManifestFailure::ManifestNotUtf8,
        );
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x42)),
        };
        assert_eq!(
            record_module(&temp, &record).expect_err("write refused"),
            ModuleManifestFailure::ManifestNotUtf8,
        );
        assert_eq!(
            std::fs::read(temp.join(SIRALOS_TOML_FILE_NAME)).expect("read"),
            [0xff],
        );
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_rejects_control_characters_in_component_path() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
            manifest_text(
                "godot",
                &digest_hex(0xab),
                ABI,
                Some(r"\u0007component.wasm"),
            ),
        )
        .expect("write manifest");

        let failure = load_manifest(&temp, &temp)
            .expect_err("control character must be rejected");

        assert_eq!(failure.code(), "MANIFEST_INVALID");
        assert!(!failure.to_string().contains("component.wasm"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_rejects_parent_component_paths_generically() {
        let temp = workspace();
        for component in ["..", "../outside", r"nested\\component.wasm"] {
            write(
                temp.join(MODULE_MANIFEST_FILE_NAME),
                manifest_text(
                    "godot",
                    &digest_hex(0xab),
                    ABI,
                    Some(component),
                ),
            )
            .expect("write manifest");

            let failure = load_manifest(&temp, &temp)
                .expect_err("parent component path must be rejected");
            assert_eq!(
                failure.code(),
                "MANIFEST_INVALID",
                "component {component:?}",
            );
            let diagnostic = failure.to_string();
            for marker in
                ["..", "outside", "nested", "component.wasm", "godot"]
            {
                assert!(
                    !diagnostic.contains(marker),
                    "diagnostic echoed {marker:?} for {component:?}: \
                     {diagnostic}",
                );
            }
        }
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_manifest_ignores_unknown_keys() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
            format!(
                "id = \"godot\"\ndigest = \"{}\"\nabi = \"{ABI}\"\nunknown = 42\n",
                digest_hex(0x01)
            ),
        )
        .expect("write manifest");
        assert!(load_manifest(&temp, &temp).is_ok());
        let _ = remove_dir_all(temp);
    }

    fn ui_manifest_text(specs: &str) -> String {
        format!(
            "id = \"godot\"\ndigest = \"{}\"\nabi = \"{ABI}\"\ncapabilities = [\"workspace-read\"]\n{specs}",
            digest_hex(0x01)
        )
    }

    const UI_ONE_SPEC: &str = "[ui]\n[[ui.components]]\nid = \"panel-one\"\ntitle = \"One\"\nfeed = \"tools.catalog\"\nenabled = true\nplacement = \"tab\"\nmax_rows = 20\nmax_cols = 60\n";

    #[test]
    fn manifest_ui_section_parses_validated_specs() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
            ui_manifest_text(&format!(
                "{UI_ONE_SPEC}[[ui.components]]\nid = \"panel-two\"\ntitle = \"Two\"\nfeed = \"workspace.list\"\nplacement = \"cell:0,0\"\n"
            )),
        )
        .expect("write manifest");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        let specs = manifest.ui_specs();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].id(), "panel-one");
        assert_eq!(specs[0].title(), "One");
        assert!(specs[0].enabled());
        assert_eq!(specs[1].id(), "panel-two");
        // Defaults apply where the manifest omits keys.
        assert!(specs[1].enabled());
        assert_eq!(specs[1].max_rows(), 20);
        assert_eq!(specs[1].max_cols(), 60);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn manifest_without_ui_section_has_no_specs() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
            manifest_text("godot", &digest_hex(0x01), ABI, None),
        )
        .expect("write manifest");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        assert!(manifest.ui_specs().is_empty());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn manifest_ui_section_refusals_are_typed() {
        let temp = workspace();
        let cases = [
            // Nine specs exceed the per-manifest bound.
            ("oversized", "[ui]\n".to_owned() + &"[[ui.components]]\nid = \"x\"\ntitle = \"x\"\nfeed = \"tools.catalog\"\nplacement = \"tab\"\n".repeat(9)),
            // Unknown feed.
            (
                "feed",
                format!(
                    "{UI_ONE_SPEC}[[ui.components]]\nid = \"bad\"\ntitle = \"b\"\nfeed = \"nope\"\nplacement = \"tab\"\n"
                ),
            ),
            // Unknown entry key.
            (
                "keys",
                format!(
                    "{UI_ONE_SPEC}[[ui.components]]\nid = \"bad\"\ntitle = \"b\"\nfeed = \"tools.catalog\"\nplacement = \"tab\"\nfuture = 1\n"
                ),
            ),
            // Non-table entries.
            (
                "entries",
                "[ui]\ncomponents = [42]\n".to_owned(),
            ),
            // Non-array components.
            ("array", "[ui]\ncomponents = 42\n".to_owned()),
        ];
        for (label, specs) in cases {
            write(
                temp.join(MODULE_MANIFEST_FILE_NAME),
                ui_manifest_text(&specs),
            )
            .expect("write manifest");
            assert_eq!(
                load_manifest(&temp, &temp).expect_err("must refuse").code(),
                "MANIFEST_INVALID",
                "{label}"
            );
        }
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn component_digest_mismatch_is_typed() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
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
            temp.join(MODULE_MANIFEST_FILE_NAME),
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
    fn install_module_record_conflict_precedes_missing_component() {
        let temp = workspace();
        let folder = temp.join("plugins/godot");
        create_dir_all(&folder).expect("module folder");
        let component_path = folder.join("missing.wasm");
        let candidate_digest = digest_hex(0xab);
        write(
            folder.join(MODULE_MANIFEST_FILE_NAME),
            manifest_text(
                "godot",
                &candidate_digest,
                ABI,
                Some("missing.wasm"),
            ),
        )
        .expect("candidate manifest");
        let manifest =
            load_manifest(&temp, &folder).expect("candidate manifest loads");
        assert!(!component_path.exists());

        let existing_digest = digest_hex(0xcd);
        let existing = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/other".to_owned(),
            digest: format!("sha256:{existing_digest}"),
        };
        record_module(&temp, &existing).expect("existing record");
        let record_path = temp.join(SIRALOS_TOML_FILE_NAME);
        let original_record =
            read(&record_path).expect("read existing record");

        let failure = install_module(&manifest, &temp, &folder)
            .expect_err("persisted conflict must win over missing component");

        assert_eq!(failure.code(), "RECORD_CONFLICT");
        assert_ne!(failure.code(), "COMPONENT_UNUSABLE");
        let diagnostic = failure.to_string();
        for marker in [
            "godot",
            "plugins/godot",
            "plugins/other",
            "missing.wasm",
            existing_digest.as_str(),
            candidate_digest.as_str(),
        ] {
            assert!(
                !diagnostic.contains(marker),
                "diagnostic echoed {marker:?}: {diagnostic}",
            );
        }
        assert_eq!(read(record_path).expect("reread record"), original_record);
        assert!(!component_path.exists());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn component_without_declared_path_passes_verification() {
        let temp = workspace();
        write(
            temp.join(MODULE_MANIFEST_FILE_NAME),
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
            temp.join(MODULE_MANIFEST_FILE_NAME),
            manifest_text("godot", &digest, ABI, Some("c.wasm")),
        )
        .expect("write manifest");
        write(temp.join("c.wasm"), &bytes).expect("component");
        let manifest = load_manifest(&temp, &temp).expect("manifest loads");
        assert!(verify_component(&manifest).is_ok());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn module_records_load_from_missing_file_as_empty() {
        let temp = workspace();
        let records = load_module_records(&temp).expect("loads");
        assert!(records.is_empty());
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_and_reread_roundtrip() {
        let temp = workspace();
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0xcd)),
        };
        record_module(&temp, &record).expect("records");
        let records = load_module_records(&temp).expect("loads");
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
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        record_module(&temp, &record).expect("records");
        let text = std::fs::read_to_string(temp.join(SIRALOS_TOML_FILE_NAME))
            .expect("read");
        assert!(text.contains("key = \"kept\""));
        assert!(text.contains("[modules.godot]"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_preserves_independent_profile_module_fields() {
        let temp = workspace();
        write(
            temp.join(SIRALOS_TOML_FILE_NAME),
            "[profile]\nname = \"dev\"\nmodules = [\"selected\"]\n\n[profile.context]\nkind = \"live\"\n\n[modules.existing]\npath = \"plugins/existing\"\ndigest = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        )
        .expect("write");
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0xcd)),
        };

        record_module(&temp, &record).expect("records");

        let text = std::fs::read_to_string(temp.join(SIRALOS_TOML_FILE_NAME))
            .expect("read");
        let document: toml::Table = toml::from_str(&text).expect("valid TOML");
        let profile = document
            .get("profile")
            .and_then(toml::Value::as_table)
            .expect("profile table");
        let selected = profile
            .get("modules")
            .and_then(toml::Value::as_array)
            .expect("profile module selection");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].as_str(), Some("selected"));
        assert_eq!(
            profile
                .get("context")
                .and_then(toml::Value::as_table)
                .and_then(|context| context.get("kind"))
                .and_then(toml::Value::as_str),
            Some("live"),
        );
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_rejects_invalid_fields_before_writing() {
        let temp = workspace();
        let records = [
            ModuleRecord {
                id: "UPPER-case".to_owned(),
                path: "plugins/godot".to_owned(),
                digest: format!("sha256:{}", digest_hex(0x01)),
            },
            ModuleRecord {
                id: "godot".to_owned(),
                path: "a".repeat(MAX_MODULE_FIELD_BYTES + 1),
                digest: format!("sha256:{}", digest_hex(0x01)),
            },
            ModuleRecord {
                id: "godot".to_owned(),
                path: "plugins/\u{0007}godot".to_owned(),
                digest: format!("sha256:{}", digest_hex(0x01)),
            },
            ModuleRecord {
                id: "godot".to_owned(),
                path: "plugins/godot".to_owned(),
                digest: "not-a-digest".to_owned(),
            },
        ];

        for record in records {
            let failure = record_module(&temp, &record).expect_err("refused");
            assert_eq!(failure.code(), "RECORD_CONFLICT");
            assert!(!temp.join(SIRALOS_TOML_FILE_NAME).exists());
        }
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_rejects_module_count_over_limit_before_rewrite() {
        let temp = workspace();
        let path = temp.join(SIRALOS_TOML_FILE_NAME);
        let original = document_with_module_count(MAX_MODULE_RECORDS);
        write(&path, &original).expect("write");
        let record = ModuleRecord {
            id: "plugin999".to_owned(),
            path: "plugins/plugin999".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x99)),
        };

        let failure = record_module(&temp, &record).expect_err("refused");

        assert_eq!(failure.code(), "RECORD_CONFLICT");
        assert!(!failure.to_string().contains("plugin999"));
        assert!(!failure.to_string().contains("plugins/plugin999"));
        assert_eq!(std::fs::read_to_string(path).expect("read"), original);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_write_refuses_serialized_document_over_byte_bound() {
        let temp = workspace();
        let mut profile = toml::Table::new();
        profile.insert(
            "padding".to_owned(),
            toml::Value::String("a".repeat(MAX_SIRALOS_TOML_BYTES + 1)),
        );
        let mut document = toml::Table::new();
        document.insert("profile".to_owned(), toml::Value::Table(profile));

        let failure = write_record_document(&temp, &document, None)
            .expect_err("refused");

        match &failure {
            ModuleManifestFailure::RecordConflict(reason) => {
                assert!(reason.contains("serialized"));
            }
            other => panic!("unexpected failure: {other:?}"),
        }
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        assert!(!temp.join(SIRALOS_TOML_FILE_NAME).exists());
        assert_eq!(std::fs::read_dir(&temp).expect("entries").count(), 0);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_write_refuses_stale_source_digest() {
        let temp = workspace();
        let path = temp.join(SIRALOS_TOML_FILE_NAME);
        let original = "[profile]\nname = \"before\"\n";
        write(&path, original).expect("write original");
        let expected_digest = sha256_hex(original.as_bytes());
        let concurrent = "[profile]\nname = \"concurrent\"\n";
        write(&path, concurrent).expect("write concurrent");
        let document: toml::Table =
            toml::from_str(concurrent).expect("valid TOML");

        let failure =
            write_record_document(&temp, &document, Some(&expected_digest))
                .expect_err("stale source refused");

        assert_eq!(failure.code(), "RECORD_CONFLICT");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), concurrent,);
        assert_eq!(std::fs::read_dir(&temp).expect("entries").count(), 1);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_write_refuses_target_that_appears() {
        let temp = workspace();
        let path = temp.join(SIRALOS_TOML_FILE_NAME);
        let concurrent = "[profile]\nname = \"concurrent\"\n";
        write(&path, concurrent).expect("write concurrent");
        let replacement: toml::Table =
            toml::from_str("[profile]\nname = \"replacement\"\n")
                .expect("valid TOML");

        let failure = write_record_document(&temp, &replacement, None)
            .expect_err("refused");

        assert_eq!(failure.code(), "RECORD_CONFLICT");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), concurrent,);
        assert_eq!(std::fs::read_dir(&temp).expect("entries").count(), 1);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_diagnostics_do_not_echo_toml_id_or_path() {
        let temp = workspace();
        let path = temp.join(SIRALOS_TOML_FILE_NAME);
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let debug = format!("{record:?}");
        assert!(!debug.contains("godot"));
        assert!(!debug.contains("plugins/godot"));
        assert!(debug.contains("<redacted>"));

        write(&path, "private_marker = ???\n").expect("write malformed TOML");
        let diagnostic =
            record_module(&temp, &record).expect_err("refused").to_string();
        assert!(!diagnostic.contains("private_marker"));

        write(
            &path,
            "[modules.UPPER-private]\npath = \"plugins/private\"\ndigest = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        )
        .expect("write bad id");
        let diagnostic =
            load_module_records(&temp).expect_err("refused").to_string();
        assert!(!diagnostic.contains("UPPER-private"));
        assert!(!diagnostic.contains("plugins/private"));

        write(
            &path,
            "[modules.godot]\npath = \"C:/private/workspace\"\ndigest = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        )
        .expect("write bad path");
        let diagnostic =
            load_module_records(&temp).expect_err("refused").to_string();
        assert!(!diagnostic.contains("godot"));
        assert!(!diagnostic.contains("C:/private/workspace"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_io_diagnostic_does_not_echo_workspace_path() {
        let temp = workspace();
        let missing_root = temp.join("private-workspace");
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };

        let failure =
            record_module(&missing_root, &record).expect_err("refused");

        assert_eq!(failure.code(), "RECORD_IO");
        assert!(!failure.to_string().contains("private-workspace"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_second_module_is_appended() {
        let temp = workspace();
        let first = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let second = ModuleRecord {
            id: "konstruct".to_owned(),
            path: "plugins/konstruct".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x02)),
        };
        record_module(&temp, &first).expect("first");
        record_module(&temp, &second).expect("second");
        let records = load_module_records(&temp).expect("loads");
        assert_eq!(records, vec![first, second]);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_same_identity_is_idempotent() {
        let temp = workspace();
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        record_module(&temp, &record).expect("first");
        record_module(&temp, &record).expect("second (same identity)");
        let records = load_module_records(&temp).expect("loads");
        assert_eq!(records, vec![record]);
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_conflicting_identity_is_refused() {
        let temp = workspace();
        let first = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let conflicting = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x02)),
        };
        record_module(&temp, &first).expect("first");
        let failure = record_module(&temp, &conflicting).unwrap_err();
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_symlink_target_is_refused_without_touching_it() {
        #[cfg(unix)]
        {
            let temp = workspace();
            let target = temp.join("elsewhere.toml");
            write(&target, b"[unrelated]\nkey = \"original\"\n")
                .expect("target");
            std::os::unix::fs::symlink(
                &target,
                temp.join(SIRALOS_TOML_FILE_NAME),
            )
            .expect("symlink");
            let record = ModuleRecord {
                id: "godot".to_owned(),
                path: "plugins/godot".to_owned(),
                digest: format!("sha256:{}", digest_hex(0x01)),
            };
            let failure = record_module(&temp, &record).unwrap_err();
            assert_eq!(failure.code(), "RECORD_CONFLICT");
            let target_text =
                std::fs::read_to_string(&target).expect("target unchanged");
            assert!(target_text.contains("original"));
            let _ = remove_dir_all(temp);
        }
    }

    #[test]
    fn record_directory_target_is_refused() {
        let temp = workspace();
        std::fs::create_dir(temp.join(SIRALOS_TOML_FILE_NAME)).expect("dir");
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let failure = record_module(&temp, &record).unwrap_err();
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn record_oversized_siralos_toml_is_refused() {
        let temp = workspace();
        let oversized = vec![b'x'; MAX_SIRALOS_TOML_BYTES + 1];
        write(temp.join(SIRALOS_TOML_FILE_NAME), &oversized).expect("file");
        let record = ModuleRecord {
            id: "godot".to_owned(),
            path: "plugins/godot".to_owned(),
            digest: format!("sha256:{}", digest_hex(0x01)),
        };
        let failure = record_module(&temp, &record).unwrap_err();
        assert_eq!(failure.code(), "RECORD_IO");
        // The target must be untouched (still the original oversized bytes).
        assert_eq!(
            std::fs::metadata(temp.join(SIRALOS_TOML_FILE_NAME))
                .expect("file still exists")
                .len(),
            (MAX_SIRALOS_TOML_BYTES + 1) as u64
        );
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn crafted_record_with_bad_id_poison_is_refused() {
        let temp = workspace();
        write(
            temp.join(SIRALOS_TOML_FILE_NAME),
            "[modules.UPPER-case]\npath = \"plugins/x\"\ndigest = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        )
        .expect("file");
        let failure = load_module_records(&temp).unwrap_err();
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn crafted_record_with_bad_digest_shape_is_refused() {
        let temp = workspace();
        write(
            temp.join(SIRALOS_TOML_FILE_NAME),
            "[modules.godot]\npath = \"plugins/x\"\ndigest = \"md5:deadbeef\"\n",
        )
        .expect("file");
        let failure = load_module_records(&temp).unwrap_err();
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn crafted_record_with_absolute_path_is_refused() {
        let temp = workspace();
        write(
            temp.join(SIRALOS_TOML_FILE_NAME),
            "[modules.godot]\npath = \"C:/outside\"\ndigest = \"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
        )
        .expect("file");
        let failure = load_module_records(&temp).unwrap_err();
        assert_eq!(failure.code(), "RECORD_CONFLICT");
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_module_records_rejects_escaped_control_path() {
        let temp = workspace();
        write(
            temp.join(SIRALOS_TOML_FILE_NAME),
            module_record_document(r"plugins/\u0007godot"),
        )
        .expect("write module record");

        let failure =
            load_module_records(&temp).expect_err("control path refused");

        assert_eq!(failure.code(), "RECORD_CONFLICT");
        assert!(!failure.to_string().contains("godot"));
        let _ = remove_dir_all(temp);
    }

    #[test]
    fn load_module_records_rejects_unsafe_paths_generically() {
        let outcomes = UNSAFE_PLUGIN_RECORD_PATHS
            .iter()
            .map(|path| {
                let temp = workspace();
                write(
                    temp.join(SIRALOS_TOML_FILE_NAME),
                    module_record_document(path),
                )
                .expect("write module record");
                let result = load_module_records(&temp).map(|_| ());
                let _ = remove_dir_all(temp);
                (*path, result)
            })
            .collect();

        assert_generic_record_rejections(outcomes);
    }

    #[test]
    fn record_module_rejects_unsafe_paths_generically() {
        let outcomes = UNSAFE_PLUGIN_RECORD_PATHS
            .iter()
            .map(|path| {
                let temp = workspace();
                let record = ModuleRecord {
                    id: "godot".to_owned(),
                    path: (*path).to_owned(),
                    digest: format!("sha256:{}", digest_hex(0x42)),
                };
                let result = record_module(&temp, &record);
                let wrote_record = temp.join(SIRALOS_TOML_FILE_NAME).exists();
                let _ = remove_dir_all(temp);
                (*path, result, wrote_record)
            })
            .collect::<Vec<_>>();

        assert!(
            outcomes.iter().all(|(_, result, _)| result.is_err()),
            "every unsafe module record path must be rejected: {outcomes:#?}",
        );
        assert!(
            outcomes.iter().all(|(_, _, wrote_record)| !wrote_record),
            "a rejected module record path must not be written: {outcomes:#?}",
        );
        assert_generic_record_rejections(
            outcomes
                .into_iter()
                .map(|(path, result, _)| (path, result))
                .collect(),
        );
    }
}
