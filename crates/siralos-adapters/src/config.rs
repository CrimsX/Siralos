//! User-level configuration loading and structural validation (Stage 3R R7.4).
//!
//! This module owns the external `~/.siralos/config.json` format: path
//! discovery is read-only, files are lstat-checked and bounded, JSON is
//! decoded strictly, and every accepted field is validated at its boundary.
//! The CLI composition root owns overrides and the provider/reference policy
//! decisions that consume this value. No configuration authority is added to
//! `siralos-core`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::paths::{StateDirError, state_dir};
use crate::workspace::fs::{
    BoundedFileRead, decode_utf8, read_complete_file_bounded,
};

/// Maximum complete configuration-file size, including the JSON bytes.
pub const MAX_CONFIG_FILE_BYTES: usize = 1024 * 1024;
/// Maximum configured Godot installations.
pub const MAX_GODOT_INSTALLATIONS: usize = 16;
/// Maximum configured references.
pub const MAX_REFERENCES: usize = 16;
/// Maximum installation and reference identifier length.
pub const MAX_IDENTIFIER_LENGTH: usize = 64;
/// Maximum configured review-provider identifier length.
pub const MAX_REVIEW_PROVIDER_LENGTH: usize = 128;
/// Maximum reference description size in UTF-8 bytes.
pub const MAX_REFERENCE_DESCRIPTION_BYTES: usize = 512;
/// Maximum local-directory reference path length.
pub const MAX_LOCAL_REFERENCE_PATH_LENGTH: usize = 4096;
/// Maximum repository origin length.
pub const MAX_REPOSITORY_LENGTH: usize = 2048;
/// Maximum commit pin length.
pub const MAX_COMMIT_LENGTH: usize = 64;
/// Maximum tag and branch pin length.
pub const MAX_TAG_OR_BRANCH_LENGTH: usize = 128;

/// Built-in sandbox profile identifiers accepted by user configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSandboxProfileId {
    /// Read-only inspection profile.
    Inspect,
    /// Read-only development profile with the built-in offline posture.
    DevelopOffline,
}

impl UserSandboxProfileId {
    /// Return the external configuration spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::DevelopOffline => "develop-offline",
        }
    }
}

/// Built-in sandbox backend identifiers accepted by user configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSandboxBackendId {
    /// Select the host's pinned backend policy.
    Auto,
    /// Name the pinned Anthropic Sandbox Runtime backend.
    AnthropicRuntime,
}

impl UserSandboxBackendId {
    /// Return the external configuration spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AnthropicRuntime => "anthropic-runtime",
        }
    }
}

/// User-supplied sandbox selection. It selects only built-in host profiles;
/// it cannot grant capabilities or make an unavailable backend executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSandboxConfig {
    /// Built-in profile selection.
    pub profile: UserSandboxProfileId,
    /// Built-in backend selection.
    pub backend: UserSandboxBackendId,
}

/// User-supplied Godot edition hint. It is only a hint and is not engine
/// discovery or semantic edition selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserGodotEditionHint {
    /// Standard Godot edition hint.
    Standard,
    /// .NET Godot edition hint.
    Dotnet,
    /// No edition hint.
    Unknown,
}

impl UserGodotEditionHint {
    /// Return the external configuration spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Dotnet => "dotnet",
            Self::Unknown => "unknown",
        }
    }
}

/// One structurally valid configured installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGodotInstallationConfig {
    /// Absolute installation path as supplied by the user.
    pub path: String,
    /// Non-authoritative edition hint.
    pub edition_hint: UserGodotEditionHint,
}

/// Generic Godot configuration envelope. R8/R9 own semantic selection and
/// PATH/engine behavior; this type deliberately does not implement them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserGodotConfig {
    /// Optional configured installation id.
    pub active_installation: Option<String>,
    /// Configured installation map in deterministic key order.
    pub installations: BTreeMap<String, UserGodotInstallationConfig>,
    /// Whether the future Godot adapter may use fixed-name PATH discovery.
    pub discover_on_path: bool,
}

/// Quality configuration accepted at R7.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQualityConfig {
    /// Optional registered reviewer provider id.
    pub review_provider: Option<String>,
}

/// Reference declaration kind in the external flattened config format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserReferenceKind {
    /// A local directory reference.
    LocalDirectory,
    /// A repository reference.
    Repository,
}

impl UserReferenceKind {
    /// Return the external configuration spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LocalDirectory => "local-directory",
            Self::Repository => "repository",
        }
    }
}

/// Repository ref pin in the external config format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRepositoryRef {
    /// Immutable hexadecimal commit pin.
    Commit(String),
    /// Tag pin.
    Tag(String),
    /// Branch pin, which remains mutable until a later resolver policy.
    Branch(String),
}

impl UserRepositoryRef {
    /// Return the ref kind spelling.
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Commit(_) => "commit",
            Self::Tag(_) => "tag",
            Self::Branch(_) => "branch",
        }
    }

    /// Return the pin value.
    pub fn value(&self) -> &str {
        match self {
            Self::Commit(value) | Self::Tag(value) | Self::Branch(value) => {
                value
            }
        }
    }
}

/// One structurally valid external reference declaration. Semantic source
/// validation is exposed separately because the TypeScript application keeps
/// invalid reference declarations nonfatal at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserReferenceConfig {
    /// Reference source kind.
    pub kind: UserReferenceKind,
    /// Flattened local-directory path, when applicable.
    pub path: Option<String>,
    /// Flattened repository origin, when applicable.
    pub repository: Option<String>,
    /// Optional repository pin.
    pub reference: Option<UserRepositoryRef>,
    /// Optional bounded description.
    pub description: Option<String>,
}

/// Parsed user configuration with all absent sections materialized to their
/// deterministic defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserConfig {
    /// Sandbox selection.
    pub sandbox: UserSandboxConfig,
    /// Generic Godot envelope.
    pub godot: UserGodotConfig,
    /// Quality provider selection.
    pub quality: UserQualityConfig,
    /// External reference declarations.
    pub references: BTreeMap<String, UserReferenceConfig>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            sandbox: UserSandboxConfig {
                profile: UserSandboxProfileId::Inspect,
                backend: UserSandboxBackendId::Auto,
            },
            godot: UserGodotConfig {
                active_installation: None,
                installations: BTreeMap::new(),
                discover_on_path: true,
            },
            quality: UserQualityConfig { review_provider: None },
            references: BTreeMap::new(),
        }
    }
}

/// Stable error categories for diagnostics and differential canonicalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigErrorCategory {
    /// The process home directory could not be resolved.
    NoHomeDirectory,
    /// The path could not be inspected or read.
    CannotRead,
    /// The path is a symlink or non-regular file.
    NotRegular,
    /// The complete file exceeds the one-MiB bound.
    TooLarge,
    /// The complete file is not UTF-8.
    InvalidUtf8,
    /// The complete UTF-8 file is not valid JSON.
    InvalidJson,
    /// JSON shape or value validation failed.
    InvalidValue,
}

impl ConfigErrorCategory {
    /// Return the stable diagnostic category.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoHomeDirectory => "NO_HOME_DIRECTORY",
            Self::CannotRead => "CANNOT_READ",
            Self::NotRegular => "NOT_REGULAR",
            Self::TooLarge => "TOO_LARGE",
            Self::InvalidUtf8 => "INVALID_UTF8",
            Self::InvalidJson => "INVALID_JSON",
            Self::InvalidValue => "INVALID_VALUE",
        }
    }
}

/// User configuration failure. The message is suitable for a CLI diagnostic
/// after the caller has chosen whether to expose the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    category: ConfigErrorCategory,
    message: String,
}

impl ConfigError {
    fn new(category: ConfigErrorCategory, message: impl Into<String>) -> Self {
        Self { category, message: message.into() }
    }

    /// Return the stable diagnostic category.
    pub const fn category(&self) -> ConfigErrorCategory {
        self.category
    }

    /// Return the user-facing diagnostic message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConfigError {}

impl From<StateDirError> for ConfigError {
    fn from(error: StateDirError) -> Self {
        Self::new(ConfigErrorCategory::NoHomeDirectory, error.to_string())
    }
}

/// Resolve the default user configuration path without creating its parent.
pub fn default_user_config_path() -> Result<PathBuf, ConfigError> {
    Ok(state_dir()?.join("config.json"))
}

/// Load one user configuration file using the bounded complete-read
/// primitive. Missing files return defaults and never create a directory or
/// file. Symlinks and non-regular files are rejected before opening.
pub fn load_user_config(path: &Path) -> Result<UserConfig, ConfigError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UserConfig::default());
        }
        Err(error) => {
            return Err(ConfigError::new(
                ConfigErrorCategory::CannotRead,
                format!(
                    "Cannot read Siralos configuration at {}: {error}",
                    path.display()
                ),
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ConfigError::new(
            ConfigErrorCategory::NotRegular,
            format!(
                "Siralos configuration at {} is not a regular file.",
                path.display()
            ),
        ));
    }
    if metadata.len() > MAX_CONFIG_FILE_BYTES as u64 {
        return Err(ConfigError::new(
            ConfigErrorCategory::TooLarge,
            format!(
                "Siralos configuration at {} exceeds the {MAX_CONFIG_FILE_BYTES}-byte limit.",
                path.display()
            ),
        ));
    }
    let bytes = match read_complete_file_bounded(path, MAX_CONFIG_FILE_BYTES) {
        BoundedFileRead::Complete(bytes) => bytes,
        BoundedFileRead::TooLarge => {
            return Err(ConfigError::new(
                ConfigErrorCategory::TooLarge,
                format!(
                    "Siralos configuration at {} could not be read within the {MAX_CONFIG_FILE_BYTES}-byte limit.",
                    path.display()
                ),
            ));
        }
        BoundedFileRead::NotReadable => {
            return Err(ConfigError::new(
                ConfigErrorCategory::CannotRead,
                format!(
                    "Siralos configuration at {} could not be read within the {MAX_CONFIG_FILE_BYTES}-byte limit.",
                    path.display()
                ),
            ));
        }
        BoundedFileRead::IoError(error) => {
            return Err(ConfigError::new(
                ConfigErrorCategory::CannotRead,
                format!(
                    "Cannot read Siralos configuration at {}: {error}",
                    path.display()
                ),
            ));
        }
    };
    let content = decode_utf8(&bytes).ok_or_else(|| {
        ConfigError::new(
            ConfigErrorCategory::InvalidUtf8,
            format!(
                "Siralos configuration at {} is not valid UTF-8.",
                path.display()
            ),
        )
    })?;
    let value: Value = serde_json::from_str(&content).map_err(|error| {
        ConfigError::new(
            ConfigErrorCategory::InvalidJson,
            format!(
                "Siralos configuration at {} is not valid JSON: {error}",
                path.display()
            ),
        )
    })?;
    parse_user_config(&value)
}

/// Stable state of the selected configuration path for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationFileState {
    /// The path exists and is a regular file.
    Readable,
    /// The path does not exist.
    Missing,
    /// The path exists but is not a readable regular config file.
    Unreadable,
}

impl ConfigurationFileState {
    /// Return the stable diagnostic spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Readable => "readable",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
        }
    }
}

/// One fixed-order configuration section presence entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationSectionPresence {
    /// The section name.
    pub name: &'static str,
    /// Whether the raw JSON object declared the section.
    pub present: bool,
}

/// Read-only configuration diagnostics. It reuses [`load_user_config`] as
/// the single validator and never exposes credential values or environment
/// variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationDiagnostics {
    /// Whether loading and validation succeeded, including a missing file.
    pub loaded: bool,
    /// Fixed-order section presence entries.
    pub sections: Vec<ConfigurationSectionPresence>,
    /// Unknown fields are rejected by the validator, so this remains empty.
    pub unknown_fields: Vec<String>,
    /// Validation errors, when loading failed.
    pub validation_errors: Vec<String>,
    /// Credential references; R7.4 has no credential-bearing config.
    pub credential_refs: Vec<String>,
    /// Whether an explicit override is in use; the CLI supplies this context.
    pub override_in_use: bool,
    /// Readability state of the selected path.
    pub file_state: ConfigurationFileState,
}

/// Read configuration diagnostics without creating or mutating anything.
pub fn read_configuration_diagnostics(
    path: &Path,
) -> ConfigurationDiagnostics {
    let file_state = match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file() && !metadata.file_type().is_symlink() =>
        {
            ConfigurationFileState::Readable
        }
        Ok(_) => ConfigurationFileState::Unreadable,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ConfigurationFileState::Missing
        }
        Err(_) => ConfigurationFileState::Unreadable,
    };
    let mut sections = [
        ConfigurationSectionPresence { name: "sandbox", present: false },
        ConfigurationSectionPresence { name: "godot", present: false },
        ConfigurationSectionPresence { name: "quality", present: false },
        ConfigurationSectionPresence { name: "references", present: false },
    ];
    if let BoundedFileRead::Complete(bytes) =
        read_complete_file_bounded(path, MAX_CONFIG_FILE_BYTES)
    {
        if let Some(value) = decode_utf8(&bytes)
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| value.as_object().cloned())
        {
            for section in &mut sections {
                section.present = value.contains_key(section.name);
            }
        }
    }
    let validation_errors = match load_user_config(path) {
        Ok(_) => Vec::new(),
        Err(error) => vec![error.to_string()],
    };
    ConfigurationDiagnostics {
        loaded: validation_errors.is_empty(),
        sections: sections.to_vec(),
        unknown_fields: Vec::new(),
        validation_errors,
        credential_refs: Vec::new(),
        override_in_use: false,
        file_state,
    }
}

/// Parse the JSON representation without accessing the filesystem.
pub fn parse_user_config(value: &Value) -> Result<UserConfig, ConfigError> {
    let root = object(value, "Siralos configuration")?;
    if let Some(key) =
        first_unknown(root, &["sandbox", "godot", "quality", "references"])
    {
        return Err(ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!("Unknown Siralos configuration section: {key}."),
        ));
    }
    Ok(UserConfig {
        sandbox: match root.get("sandbox") {
            Some(value) => parse_sandbox(value)?,
            None => UserConfig::default().sandbox,
        },
        godot: match root.get("godot") {
            Some(value) => parse_godot(value)?,
            None => UserConfig::default().godot,
        },
        quality: match root.get("quality") {
            Some(value) => parse_quality(value)?,
            None => UserConfig::default().quality,
        },
        references: match root.get("references") {
            Some(value) => parse_references(value)?,
            None => BTreeMap::new(),
        },
    })
}

fn object<'a>(
    value: &'a Value,
    subject: &str,
) -> Result<&'a Map<String, Value>, ConfigError> {
    value.as_object().ok_or_else(|| {
        ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!("{subject} must be a JSON object."),
        )
    })
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    subject: &str,
) -> Result<(), ConfigError> {
    if let Some(key) = first_unknown(object, allowed) {
        return Err(ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!("Unknown {subject} key: {key}."),
        ));
    }
    Ok(())
}

fn first_unknown<'a>(
    object: &'a Map<String, Value>,
    allowed: &[&str],
) -> Option<&'a str> {
    object
        .keys()
        .find(|key| !allowed.contains(&key.as_str()))
        .map(String::as_str)
}

fn string_value<'a>(
    value: &'a Value,
    subject: &str,
) -> Result<&'a str, ConfigError> {
    value.as_str().ok_or_else(|| {
        ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!("{subject} must be a string."),
        )
    })
}

fn value_label(value: &Value) -> String {
    value.to_string()
}

fn parse_sandbox(value: &Value) -> Result<UserSandboxConfig, ConfigError> {
    let object = object(value, "Siralos configuration section \"sandbox\"")?;
    reject_unknown(
        object,
        &["profile", "backend"],
        "Siralos sandbox configuration",
    )?;
    let profile = match object.get("profile") {
        None => UserSandboxProfileId::Inspect,
        Some(value) => match string_value(value, "sandbox.profile")? {
            "inspect" => UserSandboxProfileId::Inspect,
            "develop-offline" => UserSandboxProfileId::DevelopOffline,
            other => {
                return Err(ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    format!(
                        "Unknown sandbox profile: {other}. Expected one of: inspect, develop-offline."
                    ),
                ));
            }
        },
    };
    let backend = match object.get("backend") {
        None => UserSandboxBackendId::Auto,
        Some(value) => match string_value(value, "sandbox.backend")? {
            "auto" => UserSandboxBackendId::Auto,
            "anthropic-runtime" => UserSandboxBackendId::AnthropicRuntime,
            other => {
                return Err(ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    format!(
                        "Unknown sandbox backend: {other}. Expected one of: auto, anthropic-runtime."
                    ),
                ));
            }
        },
    };
    Ok(UserSandboxConfig { profile, backend })
}

fn parse_quality(value: &Value) -> Result<UserQualityConfig, ConfigError> {
    let object = object(value, "Siralos configuration section \"quality\"")?;
    reject_unknown(
        object,
        &["reviewProvider"],
        "Siralos quality configuration",
    )?;
    let review_provider = match object.get("reviewProvider") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let provider = string_value(value, "quality.reviewProvider")?;
            if provider.is_empty()
                || provider.len() > MAX_REVIEW_PROVIDER_LENGTH
                || !provider.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'.' | b'_' | b'-')
                })
            {
                return Err(ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    "quality.reviewProvider must be a non-empty identifier (letters, digits, dot, dash, underscore) of at most 128 characters.",
                ));
            }
            Some(provider.to_owned())
        }
    };
    Ok(UserQualityConfig { review_provider })
}

fn parse_godot(value: &Value) -> Result<UserGodotConfig, ConfigError> {
    let object = object(value, "Siralos configuration section \"godot\"")?;
    reject_unknown(
        object,
        &["activeInstallation", "installations", "discoverOnPath"],
        "Siralos godot configuration",
    )?;
    let active_installation = match object.get("activeInstallation") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let id = string_value(value, "godot.activeInstallation")?;
            if id.is_empty() || id.chars().count() > MAX_IDENTIFIER_LENGTH {
                return Err(ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    format!(
                        "godot.activeInstallation must be a non-empty string of at most {MAX_IDENTIFIER_LENGTH} characters."
                    ),
                ));
            }
            Some(id.to_owned())
        }
    };
    let discover_on_path = match object.get("discoverOnPath") {
        None => true,
        Some(value) => value.as_bool().ok_or_else(|| {
            ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                "godot.discoverOnPath must be a boolean.",
            )
        })?,
    };
    let installations = match object.get("installations") {
        None | Some(Value::Null) => BTreeMap::new(),
        Some(value) => parse_installations(value)?,
    };
    Ok(UserGodotConfig {
        active_installation,
        installations,
        discover_on_path,
    })
}

fn parse_installations(
    value: &Value,
) -> Result<BTreeMap<String, UserGodotInstallationConfig>, ConfigError> {
    let entries = object(
        value,
        "Siralos configuration section \"godot.installations\"",
    )?;
    if entries.len() > MAX_GODOT_INSTALLATIONS {
        return Err(ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!(
                "godot.installations is limited to {MAX_GODOT_INSTALLATIONS} entries."
            ),
        ));
    }
    let mut installations = BTreeMap::new();
    for (id, value) in entries {
        if id.is_empty() || id.chars().count() > MAX_IDENTIFIER_LENGTH {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Godot installation id \"{id}\" must be non-empty and at most {MAX_IDENTIFIER_LENGTH} characters."
                ),
            ));
        }
        let installation =
            object(value, &format!("Godot installation \"{id}\""))?;
        if let Some(key) =
            first_unknown(installation, &["path", "editionHint"])
        {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Unknown Godot installation key: {key} (installation \"{id}\")."
                ),
            ));
        }
        let path = match installation.get("path") {
            Some(value) => string_value(
                value,
                &format!("Godot installation \"{id}\" path"),
            )?,
            None => {
                return Err(ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    format!(
                        "Godot installation \"{id}\" requires an absolute path."
                    ),
                ));
            }
        };
        if path.is_empty() {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Godot installation \"{id}\" requires an absolute path."
                ),
            ));
        }
        if !is_absolute_path(path) {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Godot installation \"{id}\" path must be absolute: relative paths are rejected."
                ),
            ));
        }
        let edition_hint = match installation.get("editionHint") {
            None => UserGodotEditionHint::Unknown,
            Some(value) => {
                match string_value(value, "godot.installations.editionHint")? {
                    "standard" => UserGodotEditionHint::Standard,
                    "dotnet" => UserGodotEditionHint::Dotnet,
                    "unknown" => UserGodotEditionHint::Unknown,
                    other => {
                        return Err(ConfigError::new(
                            ConfigErrorCategory::InvalidValue,
                            format!(
                                "Unknown Godot edition hint: {}. Expected one of: standard, dotnet, unknown.",
                                value_label(&Value::String(other.to_owned()))
                            ),
                        ));
                    }
                }
            }
        };
        installations.insert(
            id.clone(),
            UserGodotInstallationConfig {
                path: path.to_owned(),
                edition_hint,
            },
        );
    }
    Ok(installations)
}

fn parse_references(
    value: &Value,
) -> Result<BTreeMap<String, UserReferenceConfig>, ConfigError> {
    let entries =
        object(value, "Siralos configuration section \"references\"")?;
    if entries.len() > MAX_REFERENCES {
        return Err(ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!(
                "The \"references\" section declares {} references; the limit is {MAX_REFERENCES}.",
                entries.len()
            ),
        ));
    }
    let mut references = BTreeMap::new();
    for (alias, value) in entries {
        if !valid_reference_alias(alias) {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Reference alias \"{alias}\" is malformed; aliases match ^[a-z][a-z0-9._-]{{1,63}}$."
                ),
            ));
        }
        let declaration = object(value, &format!("Reference \"{alias}\""))?;
        if let Some(key) = first_unknown(
            declaration,
            &["kind", "path", "repository", "ref", "description"],
        ) {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Unknown Siralos reference key: {key} (reference \"{alias}\")."
                ),
            ));
        }
        let kind = match declaration.get("kind").and_then(Value::as_str) {
            Some("local-directory") => UserReferenceKind::LocalDirectory,
            Some("repository") => UserReferenceKind::Repository,
            _ => {
                return Err(ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    format!(
                        "Reference \"{alias}\" requires \"kind\" of \"local-directory\" or \"repository\"."
                    ),
                ));
            }
        };
        let description = match declaration.get("description") {
            None => None,
            Some(value) => Some(
                string_value(
                    value,
                    &format!("Reference \"{alias}\" description"),
                )?
                .to_owned(),
            ),
        };
        let parsed = match kind {
            UserReferenceKind::LocalDirectory => {
                if declaration.contains_key("repository")
                    || declaration.contains_key("ref")
                {
                    return Err(ConfigError::new(
                        ConfigErrorCategory::InvalidValue,
                        format!(
                            "Local-directory reference \"{alias}\" must not declare \"repository\" or \"ref\"."
                        ),
                    ));
                }
                let path = declaration
                    .get("path")
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .ok_or_else(|| {
                        ConfigError::new(
                            ConfigErrorCategory::InvalidValue,
                            format!(
                                "Local-directory reference \"{alias}\" requires a non-empty \"path\"."
                            ),
                        )
                    })?;
                UserReferenceConfig {
                    kind,
                    path: Some(path.to_owned()),
                    repository: None,
                    reference: None,
                    description,
                }
            }
            UserReferenceKind::Repository => {
                if declaration.contains_key("path") {
                    return Err(ConfigError::new(
                        ConfigErrorCategory::InvalidValue,
                        format!(
                            "Repository reference \"{alias}\" must not declare \"path\"."
                        ),
                    ));
                }
                let repository = declaration
                    .get("repository")
                    .and_then(Value::as_str)
                    .filter(|repository| !repository.is_empty())
                    .ok_or_else(|| {
                        ConfigError::new(
                            ConfigErrorCategory::InvalidValue,
                            format!(
                                "Repository reference \"{alias}\" requires a non-empty \"repository\"."
                            ),
                        )
                    })?;
                let reference = declaration
                    .get("ref")
                    .map(|value| parse_reference_pin(value, alias))
                    .transpose()?;
                UserReferenceConfig {
                    kind,
                    path: None,
                    repository: Some(repository.to_owned()),
                    reference,
                    description,
                }
            }
        };
        references.insert(alias.clone(), parsed);
    }
    Ok(references)
}

fn parse_reference_pin(
    value: &Value,
    alias: &str,
) -> Result<UserRepositoryRef, ConfigError> {
    let object = object(value, &format!("Reference \"{alias}\" ref"))?;
    if let Some(key) =
        first_unknown(object, &["kind", "commit", "tag", "branch"])
    {
        return Err(ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!(
                "Unknown Siralos reference ref key: {key} (reference \"{alias}\")."
            ),
        ));
    }
    let kind = object.get("kind").and_then(Value::as_str).ok_or_else(|| {
        ConfigError::new(
            ConfigErrorCategory::InvalidValue,
            format!(
                "Reference \"{alias}\" ref requires \"kind\" of \"commit\", \"tag\", or \"branch\"."
            ),
        )
    })?;
    let value_for_kind = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ConfigError::new(
                    ConfigErrorCategory::InvalidValue,
                    format!(
                        "Reference \"{alias}\" {kind} ref requires a non-empty {kind} string."
                    ),
                )
            })
    };
    let parsed = match kind {
        "commit" => {
            UserRepositoryRef::Commit(value_for_kind("commit")?.to_owned())
        }
        "tag" => UserRepositoryRef::Tag(value_for_kind("tag")?.to_owned()),
        "branch" => {
            UserRepositoryRef::Branch(value_for_kind("branch")?.to_owned())
        }
        _ => {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Reference \"{alias}\" ref requires \"kind\" of \"commit\", \"tag\", or \"branch\"."
                ),
            ));
        }
    };
    for key in ["commit", "tag", "branch"] {
        if key != kind && object.contains_key(key) {
            return Err(ConfigError::new(
                ConfigErrorCategory::InvalidValue,
                format!(
                    "Reference \"{alias}\" {kind} ref must not declare \"{key}\"; a ref pins exactly one of commit/tag/branch."
                ),
            ));
        }
    }
    Ok(parsed)
}

/// Validate semantic reference constraints while keeping the failure
/// nonfatal, matching the CLI's startup behavior. `None` means the config is
/// semantically usable; `Some` is surfaced by `/references` while the
/// application keeps an empty registry.
pub fn reference_configuration_error(config: &UserConfig) -> Option<String> {
    for (alias, reference) in &config.references {
        if let Some(description) = &reference.description {
            if description.len() > MAX_REFERENCE_DESCRIPTION_BYTES {
                return Some(format!(
                    "Reference \"{alias}\": The reference description exceeds the limit of {MAX_REFERENCE_DESCRIPTION_BYTES} bytes."
                ));
            }
        }
        match reference.kind {
            UserReferenceKind::LocalDirectory => {
                let path = reference.path.as_deref().unwrap_or_default();
                if path.len() > MAX_LOCAL_REFERENCE_PATH_LENGTH {
                    return Some(format!(
                        "Reference \"{alias}\": The local-directory path exceeds the limit of {MAX_LOCAL_REFERENCE_PATH_LENGTH} characters."
                    ));
                }
                if path.as_bytes().contains(&0) {
                    return Some(format!(
                        "Reference \"{alias}\": The local-directory path must not contain null bytes."
                    ));
                }
                if !is_absolute_path(path) {
                    return Some(format!(
                        "Reference \"{alias}\": The local-directory path \"{path}\" is not absolute; relative paths are not resolved."
                    ));
                }
            }
            UserReferenceKind::Repository => {
                let repository =
                    reference.repository.as_deref().unwrap_or_default();
                if let Err(reason) = normalize_repository_origin(repository) {
                    return Some(format!("Reference \"{alias}\": {reason}"));
                }
                if let Some(pin) = &reference.reference {
                    let value = pin.value();
                    let valid = match pin {
                        UserRepositoryRef::Commit(value) => {
                            value.len() <= MAX_COMMIT_LENGTH
                                && (7..=64).contains(&value.len())
                                && value
                                    .bytes()
                                    .all(|byte| byte.is_ascii_hexdigit())
                        }
                        UserRepositoryRef::Tag(value) => {
                            value.len() <= MAX_TAG_OR_BRANCH_LENGTH
                                && valid_branch_or_tag(value)
                        }
                        UserRepositoryRef::Branch(value) => {
                            value.len() <= MAX_TAG_OR_BRANCH_LENGTH
                                && valid_branch_or_tag(value)
                        }
                    };
                    if !valid {
                        let label = match pin {
                            UserRepositoryRef::Commit(_) => "commit",
                            UserRepositoryRef::Tag(_) => "tag",
                            UserRepositoryRef::Branch(_) => "branch",
                        };
                        return Some(format!(
                            "Reference \"{alias}\": The {label} \"{value}\" is malformed; {label}s use letters, digits, \".\", \"_\", \"-\", \"/\" and are at most {} characters.",
                            if label == "commit" {
                                MAX_COMMIT_LENGTH
                            } else {
                                MAX_TAG_OR_BRANCH_LENGTH
                            }
                        ));
                    }
                }
            }
        }
    }
    None
}

/// POSIX absolute path, Windows drive path, or Windows UNC path. This is
/// intentionally lexical: the config contract validates shape only and does
/// not probe or canonicalize a Godot or reference path.
pub fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/')
        || path.as_bytes().get(0..3).is_some_and(|prefix| {
            prefix[0].is_ascii_alphabetic()
                && prefix[1] == b':'
                && matches!(prefix[2], b'/' | b'\\')
        })
        || path.starts_with("\\\\")
            && path
                .split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .take(2)
                .count()
                == 2
}

fn valid_reference_alias(alias: &str) -> bool {
    let length = alias.chars().count();
    (2..=MAX_IDENTIFIER_LENGTH).contains(&length)
        && alias.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase()
            } else {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            }
        })
}

fn valid_branch_or_tag(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-' | b'/')
        })
}

fn normalize_repository_origin(input: &str) -> Result<String, String> {
    if input.trim().is_empty() {
        return Err("A repository origin is required.".to_owned());
    }
    let trimmed = input.trim();
    if trimmed.len() > MAX_REPOSITORY_LENGTH {
        return Err(format!(
            "Repository origin exceeds the limit of {MAX_REPOSITORY_LENGTH} characters."
        ));
    }
    if trimmed.contains('\0') {
        return Err(
            "Repository origins must not contain null bytes.".to_owned()
        );
    }
    if trimmed.contains('#') {
        return Err(
            "Repository origins must not contain a fragment.".to_owned()
        );
    }
    if trimmed.contains('?') {
        return Err(
            "Repository origins must not contain a query string.".to_owned()
        );
    }
    if trimmed.contains('@') {
        return Err("Repository origins must not contain credentials (userinfo is rejected).".to_owned());
    }
    if trimmed.starts_with("http://") {
        return Err("Repository origins must use https, not http.".to_owned());
    }
    let mut rest = trimmed;
    if let Some(stripped) = rest.strip_prefix("https://") {
        rest = stripped;
        if !rest.starts_with("github.com/") {
            let host = rest.split('/').next().unwrap_or_default();
            return Err(format!(
                "Unsupported repository host \"{host}\"; only github.com is supported."
            ));
        }
        rest = &rest["github.com/".len()..];
    }
    rest = rest.trim_end_matches('/');
    if let Some(stripped) = rest.strip_suffix(".git") {
        rest = stripped;
    }
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.len() != 2 {
        return Err("A repository origin must be exactly owner/repo with no additional path segments.".to_owned());
    }
    let owner = segments[0];
    let repo = segments[1];
    if owner.is_empty()
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(format!("Invalid repository owner \"{owner}\"."));
    }
    if repo.is_empty()
        || !repo.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(format!("Invalid repository name \"{repo}\"."));
    }
    Ok(format!("https://github.com/{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_CONFIG_FILE_BYTES, UserRepositoryRef, UserSandboxBackendId,
        UserSandboxProfileId, default_user_config_path, is_absolute_path,
        load_user_config, parse_user_config, reference_configuration_error,
    };
    use serde_json::json;
    use std::fs::{create_dir, remove_dir_all, remove_file, write};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("siralos-r7-4-{label}-{nonce}"))
    }

    #[test]
    fn absent_configuration_is_default_and_does_not_create_a_file() {
        let path = temp_path("missing").join("config.json");
        let config = load_user_config(&path).expect("missing config defaults");
        assert_eq!(config.sandbox.profile, UserSandboxProfileId::Inspect);
        assert_eq!(config.sandbox.backend, UserSandboxBackendId::Auto);
        assert!(!path.exists());
        let _ = remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn default_path_is_user_state_config_without_creation() {
        let path = default_user_config_path().expect("home path");
        assert!(path.ends_with(PathBuf::from(".siralos").join("config.json")));
    }

    #[test]
    fn parses_full_config_and_keeps_deterministic_maps() {
        let config = parse_user_config(&json!({
            "sandbox": { "profile": "develop-offline", "backend": "anthropic-runtime" },
            "godot": {
                "activeInstallation": "stable",
                "discoverOnPath": false,
                "installations": {
                    "stable": { "path": "C:\\\\Godot.exe", "editionHint": "standard" }
                }
            },
            "quality": { "reviewProvider": "deterministic-fake" },
            "references": {
                "engine-src": {
                    "kind": "repository",
                    "repository": "godotengine/godot",
                    "ref": { "kind": "commit", "commit": "0123456" },
                    "description": "Engine"
                }
            }
        }))
        .expect("valid config");
        assert_eq!(
            config.sandbox.profile,
            UserSandboxProfileId::DevelopOffline
        );
        assert_eq!(config.godot.installations.len(), 1);
        assert_eq!(
            config.quality.review_provider.as_deref(),
            Some("deterministic-fake")
        );
        assert_eq!(
            config.references["engine-src"].reference,
            Some(UserRepositoryRef::Commit("0123456".to_owned()))
        );
        assert!(reference_configuration_error(&config).is_none());
    }

    #[test]
    fn rejects_unknown_keys_at_each_structural_level() {
        assert!(parse_user_config(&json!({ "unknown": true })).is_err());
        assert!(
            parse_user_config(&json!({ "sandbox": { "secret": "x" } }))
                .is_err()
        );
        assert!(
            parse_user_config(&json!({ "godot": { "secret": true } }))
                .is_err()
        );
        assert!(
            parse_user_config(&json!({ "quality": { "secret": true } }))
                .is_err()
        );
        assert!(parse_user_config(&json!({
            "references": { "aa": { "kind": "local-directory", "path": "C:\\\\x", "secret": true } }
        })).is_err());
        assert!(parse_user_config(&json!({
            "references": { "aa": { "kind": "repository", "repository": "a/b", "ref": { "kind": "tag", "tag": "x", "secret": true } } }
        })).is_err());
    }

    #[test]
    fn rejects_invalid_enums_and_relative_installation_paths() {
        assert!(
            parse_user_config(
                &json!({ "sandbox": { "profile": "full-access" } })
            )
            .is_err()
        );
        assert!(
            parse_user_config(&json!({ "sandbox": { "backend": "docker" } }))
                .is_err()
        );
        assert!(parse_user_config(&json!({
            "godot": { "installations": { "stable": { "path": "godot.exe" } } }
        })).is_err());
        assert!(
            parse_user_config(
                &json!({ "godot": { "discoverOnPath": "yes" } })
            )
            .is_err()
        );
    }

    #[test]
    fn exact_size_is_accepted_and_one_byte_over_is_rejected() {
        let directory = temp_path("bound");
        create_dir(&directory).expect("directory");
        let path = directory.join("config.json");
        let exact = format!("{{}}{}", " ".repeat(MAX_CONFIG_FILE_BYTES - 2));
        assert_eq!(exact.len(), MAX_CONFIG_FILE_BYTES);
        write(&path, exact).expect("exact file");
        assert!(load_user_config(&path).is_ok());
        write(&path, format!("{{}}{}", " ".repeat(MAX_CONFIG_FILE_BYTES - 1)))
            .expect("oversized file");
        assert_eq!(
            load_user_config(&path).expect_err("oversize").category().as_str(),
            "TOO_LARGE"
        );
        let _ = remove_file(path);
        let _ = remove_dir_all(directory);
    }

    #[test]
    fn nonregular_configuration_is_rejected() {
        let directory = temp_path("directory");
        create_dir(&directory).expect("directory");
        let error =
            load_user_config(&directory).expect_err("directory is not config");
        assert_eq!(error.category().as_str(), "NOT_REGULAR");
        let _ = remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_configuration_is_rejected_before_open() {
        use std::os::unix::fs::symlink;
        let directory = temp_path("symlink");
        create_dir(&directory).expect("directory");
        let target = directory.join("target.json");
        let path = directory.join("config.json");
        write(&target, b"{}").expect("target");
        symlink(&target, &path).expect("symlink");
        let error =
            load_user_config(&path).expect_err("symlink is not config");
        assert_eq!(error.category().as_str(), "NOT_REGULAR");
        let _ = remove_file(path);
        let _ = remove_file(target);
        let _ = remove_dir_all(directory);
    }

    #[test]
    fn invalid_reference_paths_are_nonfatal_but_reported() {
        let config = parse_user_config(&json!({
            "references": { "aa": { "kind": "local-directory", "path": "relative" } }
        })).expect("structural config");
        assert_eq!(
            reference_configuration_error(&config).as_deref(),
            Some(
                "Reference \"aa\": The local-directory path \"relative\" is not absolute; relative paths are not resolved."
            )
        );
    }

    #[test]
    fn absolute_path_shapes_match_the_external_contract() {
        assert!(is_absolute_path("/srv/godot"));
        assert!(is_absolute_path("C:\\\\Godot.exe"));
        assert!(is_absolute_path("\\\\server\\share\\Godot.exe"));
        assert!(!is_absolute_path("relative/godot"));
    }
}
