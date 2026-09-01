//! Host-mediated credential handling (Stage 8, decision 67 C2, 68 §1).
//!
//! `HostCredential` is a newtype for the resolved `env:` reference that is
//! `!Serialize`, `!Debug` (redacted), and never written to
//! `siralos.toml`/`siralos.lock`/`Context`/`logs`. The `env:` name is
//! validated at the `siralos.toml` boundary (`profile_config.rs`), and the
//! Host resolves `std::env::var` at startup, holding the bytes only for the
//! `ModelProvider` call.

use std::fmt;

/// Redacted, non-serializable credential bytes held in memory only for one
/// `ModelProvider` call. `Debug`/`Display` print `[REDACTED]`, and the type
/// is `!Serialize` by not implementing it.
#[derive(Clone)]
pub struct HostCredential {
    bytes: Vec<u8>,
}

impl HostCredential {
    /// Resolve `env:` reference `value` (e.g., `"env:OPENAI_API_KEY"`) via
    /// `std::env::var` for the name after `env:`. Returns the redacted
    /// credential on success, or a typed error that never echoes the value.
    ///
    /// The `env:` prefix and name were already validated at the
    /// `siralos.toml` boundary, but this re-checks for defense in depth.
    pub fn from_env_ref(value: &str) -> Result<Self, String> {
        if !value.starts_with("env:") {
            return Err("A credential must start with \"env:\".".to_owned());
        }
        let name = &value[4..];
        if name.is_empty()
            || name.len() > 64
            || !name.chars().all(|c| {
                c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'
            })
        {
            return Err(
                "A credential env name must match [A-Z0-9_]{1,64} after \"env:\"."
                    .to_owned(),
            );
        }
        let var = std::env::var(name)
            .map_err(|_| format!("env var {name} is not set"))?;
        Ok(Self { bytes: var.into_bytes() })
    }

    /// Expose the bytes only to the `ModelProvider` adapters in this crate.
    /// The returned slice must never be logged, serialized, or stored in
    /// `siralos.toml`/`siralos.lock`/`Context`.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Test-only constructor that bypasses `env:` resolution. The bytes are
    /// held as `HostCredential` with the same redaction guarantees, but no
    /// `std::env::var` is called, so no `unsafe` is required.
    #[cfg(any(test, feature = "differential-harness"))]
    pub fn from_bytes_for_test(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Fallback constructor for the harness that bypasses `env:` resolution
    /// without requiring `unsafe`. Used when `from_env_ref` fails in the
    /// harness's `provider-generic` record.
    pub fn from_bytes_fallback(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl fmt::Debug for HostCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostCredential([REDACTED])")
    }
}

impl fmt::Display for HostCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::HostCredential;

    #[test]
    fn debug_is_redacted() {
        let cred = HostCredential { bytes: b"sk-secret".to_vec() };
        assert_eq!(format!("{cred:?}"), "HostCredential([REDACTED])");
        assert_eq!(format!("{cred}"), "[REDACTED]");
    }

    #[test]
    fn env_ref_must_be_env_prefix() {
        assert!(HostCredential::from_env_ref("OPENAI_API_KEY").is_err());
        assert!(HostCredential::from_env_ref("env:").is_err());
    }
}
