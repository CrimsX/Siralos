//! CLI-owned composition of the R7.4 user configuration.
//!
//! The adapter parses the external file format. This module chooses the
//! explicit path override or the canonical user path, validates the selected
//! review provider against the providers actually registered by this binary,
//! and preserves reference semantic failures as nonfatal startup diagnostics.

use std::fmt;
use std::path::{Path, PathBuf};

use siralos_adapters::config::{
    ConfigError, ConfigErrorCategory, ConfigurationDiagnostics, UserConfig,
    default_user_config_path, load_user_config,
    read_configuration_diagnostics, reference_configuration_error,
};

/// The only provider profile registered by the R7.4 candidate.
pub const DEFAULT_REVIEW_PROVIDER_ID: &str = "deterministic-fake";

/// Configuration loaded and composed for the CLI bootstrap boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedUserConfig {
    /// The chosen path, retained for diagnostics and status reporting.
    pub path: PathBuf,
    /// The structurally validated user configuration.
    pub config: UserConfig,
    /// The registered provider selected for review.
    pub review_provider_id: String,
    /// A semantic reference failure that is intentionally nonfatal at startup.
    pub reference_config_error: Option<String>,
}

/// A failure at the adapter or CLI composition boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurationError {
    /// External file loading or structural validation failed.
    Load(ConfigError),
    /// The config named a provider that is not registered by this binary.
    UnknownReviewProvider {
        /// The unregistered provider identifier.
        provider_id: String,
    },
}

impl ConfigurationError {
    /// Stable diagnostic category.
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Load(error) => error.category().as_str(),
            Self::UnknownReviewProvider { .. } => "UNKNOWN_REVIEW_PROVIDER",
        }
    }

    /// Return the precise user-facing detail.
    pub fn detail(&self) -> String {
        match self {
            Self::Load(error) => error.to_string(),
            Self::UnknownReviewProvider { provider_id } => format!(
                "Configured quality.reviewProvider \"{provider_id}\" does not match any registered provider profile; configure it or remove the setting."
            ),
        }
    }
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail())
    }
}

impl std::error::Error for ConfigurationError {}

impl From<ConfigError> for ConfigurationError {
    fn from(error: ConfigError) -> Self {
        Self::Load(error)
    }
}

/// Load and compose the user configuration for the CLI.
///
/// The default path is resolved read-only. An explicit override is used
/// verbatim. A missing file returns defaults and does not create its parent.
/// Reference semantic errors are retained in the result rather than turning
/// startup into a partial application or a process failure.
pub fn load_user_configuration(
    path_override: Option<&Path>,
) -> Result<ComposedUserConfig, ConfigurationError> {
    let path = match path_override {
        Some(path) => path.to_path_buf(),
        None => default_user_config_path()?,
    };
    let config = load_user_config(&path)?;
    let review_provider_id = match config.quality.review_provider.as_deref() {
        None => DEFAULT_REVIEW_PROVIDER_ID.to_owned(),
        Some(DEFAULT_REVIEW_PROVIDER_ID) => {
            DEFAULT_REVIEW_PROVIDER_ID.to_owned()
        }
        Some(provider_id) => {
            return Err(ConfigurationError::UnknownReviewProvider {
                provider_id: provider_id.to_owned(),
            });
        }
    };
    let reference_config_error = reference_configuration_error(&config);
    Ok(ComposedUserConfig {
        path,
        config,
        review_provider_id,
        reference_config_error,
    })
}

/// Collect fixed-order diagnostics for the selected configuration path.
pub fn diagnose_user_configuration(
    path_override: Option<&Path>,
) -> Result<ConfigurationDiagnostics, ConfigurationError> {
    let path = match path_override {
        Some(path) => path.to_path_buf(),
        None => default_user_config_path()?,
    };
    Ok(read_configuration_diagnostics(&path))
}

/// Expose the adapter category without making callers depend on its enum.
pub const fn config_error_category(
    error: &ConfigError,
) -> ConfigErrorCategory {
    error.category()
}

#[cfg(test)]
mod tests {
    use super::{
        ConfigurationError, DEFAULT_REVIEW_PROVIDER_ID,
        load_user_configuration,
    };
    use std::fs::{create_dir, write};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("siralos-cli-r7-4-{label}-{nonce}"))
    }

    #[test]
    fn explicit_override_is_composed_without_directory_creation() {
        let directory = temp_path("override");
        create_dir(&directory).expect("directory");
        let path = directory.join("config.json");
        let composed = load_user_configuration(Some(&path)).expect("defaults");
        assert_eq!(composed.path, path);
        assert_eq!(composed.review_provider_id, DEFAULT_REVIEW_PROVIDER_ID);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn registered_provider_is_accepted_and_reference_failure_is_nonfatal() {
        let directory = temp_path("valid");
        create_dir(&directory).expect("directory");
        let path = directory.join("config.json");
        write(
            &path,
            br#"{"quality":{"reviewProvider":"deterministic-fake"},"references":{"aa":{"kind":"local-directory","path":"relative"}}}"#,
        )
        .expect("config");
        let composed = load_user_configuration(Some(&path)).expect("compose");
        assert_eq!(composed.review_provider_id, DEFAULT_REVIEW_PROVIDER_ID);
        assert!(composed.reference_config_error.is_some());
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn unknown_registered_provider_is_a_composition_error() {
        let directory = temp_path("provider");
        create_dir(&directory).expect("directory");
        let path = directory.join("config.json");
        write(&path, br#"{"quality":{"reviewProvider":"reviewer"}}"#)
            .expect("config");
        let error = load_user_configuration(Some(&path))
            .expect_err("unknown provider");
        assert!(matches!(
            error,
            ConfigurationError::UnknownReviewProvider { .. }
        ));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(directory);
    }
}
