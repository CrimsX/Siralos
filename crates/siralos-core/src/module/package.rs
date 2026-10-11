//! Module package identity (Stage 3R R6).
//!
//! A module package is bound to a stable package identifier, the exact
//! SHA-256 digest of the component bytes the Host accepted, and a
//! versioned ABI identity. The Host computes and verifies the digest
//! itself; filename, directory name, declared version, mtime, and
//! caller-provided digests are never trusted.

use crate::module::capability::CapabilityRequest;
use crate::module::failure::ModuleFailure;

/// Maximum length of a package identifier in bytes.
pub(crate) const MAX_PACKAGE_ID_BYTES: usize = 128;

/// Maximum length of a canonical ABI string in bytes.
pub(crate) const MAX_ABI_BYTES: usize = 128;

fn valid_separated_identifier(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_PACKAGE_ID_BYTES {
        return false;
    }
    let mut previous_separator = false;
    for (index, byte) in value.bytes().enumerate() {
        let separator = byte == b'.' || byte == b'-';
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || separator)
            || (separator && (index == 0 || previous_separator))
        {
            return false;
        }
        previous_separator = separator;
    }
    !previous_separator
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Stable package identifier (validated, detached).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModulePackageId(String);

impl ModulePackageId {
    /// Parse a canonical package identifier.
    pub fn parse(value: &str) -> Result<Self, ModuleFailure> {
        if !valid_separated_identifier(value) {
            return Err(ModuleFailure::InvalidInput {
                reason: "invalid package id".to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The canonical identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Exact package digest: lowercase hex SHA-256 over the accepted
/// component bytes (64 hex characters).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageDigest(String);

impl PackageDigest {
    /// Parse a canonical hex SHA-256 digest.
    pub fn parse(value: &str) -> Result<Self, ModuleFailure> {
        if !valid_digest(value) {
            return Err(ModuleFailure::InvalidInput {
                reason: "invalid package digest".to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The canonical hex digest text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Versioned ABI identity in canonical WIT package form
/// (`name` at `major.minor.patch`, for example
/// `siralos:domain-abi` at `1.0.0`).
/// Compatibility is exact equality: unknown or incompatible versions
/// fail closed and are never downgraded, reinterpreted, or
/// best-effort deserialized (docs/development/PROTOCOL_VERSIONING.md).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModuleAbi(String);

impl ModuleAbi {
    /// Parse a canonical ABI string.
    pub fn parse(value: &str) -> Result<Self, ModuleFailure> {
        if !valid_abi(value) {
            return Err(ModuleFailure::InvalidInput {
                reason: "invalid domain ABI".to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }

    /// The canonical ABI text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Exact ABI compatibility. Hard-incompatible versions never
    /// match: there is no downgrade, guess, or partial match.
    pub(crate) fn is_compatible_with(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

fn valid_abi(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_ABI_BYTES {
        return false;
    }
    let Some((name, version)) = value.split_once('@') else {
        return false;
    };
    if version.contains('@') || name.is_empty() || version.is_empty() {
        return false;
    }
    let name_ok = name.split(':').all(|segment| {
        valid_separated_identifier(segment)
            && segment.len() < MAX_PACKAGE_ID_BYTES
    }) && name.contains(':');
    let version_ok = version.split('.').all(|part| {
        !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
    }) && version.split('.').count() >= 2
        && version.split('.').count() <= 3;
    name_ok && version_ok
}

/// A locally supplied, Host-accepted domain package descriptor.
/// All fields are validated and detached; the digest is the exact
/// digest of the bytes the Host accepted, computed by the Host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePackage {
    id: ModulePackageId,
    digest: PackageDigest,
    abi: ModuleAbi,
    requested_capabilities: CapabilityRequest,
}

impl ModulePackage {
    /// Construct a package from already-validated parts.
    pub fn new(
        id: ModulePackageId,
        digest: PackageDigest,
        abi: ModuleAbi,
        requested_capabilities: CapabilityRequest,
    ) -> Self {
        Self { id, digest, abi, requested_capabilities }
    }

    /// Parse a package descriptor from untrusted strings.
    pub fn parse(
        id: &str,
        digest: &str,
        abi: &str,
        requested_capabilities: &[String],
    ) -> Result<Self, ModuleFailure> {
        Ok(Self {
            id: ModulePackageId::parse(id)?,
            digest: PackageDigest::parse(digest)?,
            abi: ModuleAbi::parse(abi)?,
            requested_capabilities: CapabilityRequest::parse(
                requested_capabilities,
            )?,
        })
    }

    /// The stable package identifier.
    pub fn id(&self) -> &ModulePackageId {
        &self.id
    }

    /// The exact package digest.
    pub fn digest(&self) -> &PackageDigest {
        &self.digest
    }

    /// The versioned ABI identity.
    pub fn abi(&self) -> &ModuleAbi {
        &self.abi
    }

    /// The capabilities this package declares it wants before
    /// activation. The Host decides the effective grant.
    pub fn requested_capabilities(&self) -> &CapabilityRequest {
        &self.requested_capabilities
    }
}

/// Host verification of the declared digest against the digest the
/// Host computed from the exact accepted component bytes.
pub fn verify_package_digest(
    declared: &PackageDigest,
    computed: &PackageDigest,
) -> Result<(), ModuleFailure> {
    if declared == computed {
        Ok(())
    } else {
        Err(ModuleFailure::IdentityMismatch {
            detail:
                "package digest does not match the accepted component bytes"
                    .to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ModuleAbi, ModulePackage, ModulePackageId, PackageDigest,
        verify_package_digest,
    };
    use crate::module::failure::ModuleFailure;

    #[test]
    fn package_id_validation() {
        assert!(ModulePackageId::parse("conformance-domain").is_ok());
        assert!(ModulePackageId::parse("a.b-c").is_ok());
        assert!(ModulePackageId::parse("").is_err());
        assert!(ModulePackageId::parse("UPPER").is_err());
        assert!(ModulePackageId::parse("-lead").is_err());
        assert!(ModulePackageId::parse("trail-").is_err());
        assert!(ModulePackageId::parse("a..b").is_err());
    }

    #[test]
    fn digest_validation() {
        let hex = "ab".repeat(32);
        assert!(PackageDigest::parse(&hex).is_ok());
        assert!(PackageDigest::parse(&hex.to_uppercase()).is_err());
        assert!(PackageDigest::parse(&hex[..63]).is_err());
        assert!(PackageDigest::parse("xyz").is_err());
    }

    #[test]
    fn abi_validation_and_exact_compatibility() {
        let v1 = ModuleAbi::parse("siralos:domain-abi@1.0.0").unwrap();
        let v1_again = ModuleAbi::parse("siralos:domain-abi@1.0.0").unwrap();
        let v2 = ModuleAbi::parse("siralos:domain-abi@1.1.0").unwrap();
        assert!(v1.is_compatible_with(&v1_again));
        assert!(!v1.is_compatible_with(&v2));
        assert!(ModuleAbi::parse("siralos:domain-abi@1.0.0").is_ok());
        assert!(ModuleAbi::parse("name@1.0.0").is_err(), "needs a namespace");
        assert!(ModuleAbi::parse("siralos:domain-abi@1.0").is_ok());
        assert!(ModuleAbi::parse("siralos:domain-abi@1").is_err());
        assert!(ModuleAbi::parse("siralos:domain-abi@x").is_err());
        assert!(ModuleAbi::parse("siralos:domain-abi").is_err());
        assert!(ModuleAbi::parse("siralos:domain-abi@1.0.0.0").is_err());
    }

    #[test]
    fn digest_verification_is_exact() {
        let a = PackageDigest::parse(&"ab".repeat(32)).unwrap();
        let b = PackageDigest::parse(&"cd".repeat(32)).unwrap();
        assert!(verify_package_digest(&a, &a).is_ok());
        assert!(matches!(
            verify_package_digest(&a, &b),
            Err(ModuleFailure::IdentityMismatch { .. })
        ));
    }

    #[test]
    fn package_parse_rejects_malformed_descriptors() {
        let digest = "ab".repeat(32);
        assert!(
            ModulePackage::parse(
                "conformance-domain",
                &digest,
                "siralos:domain-abi@1.0.0",
                &[],
            )
            .is_ok()
        );
        assert!(matches!(
            ModulePackage::parse("", &digest, "siralos:domain-abi@1.0.0", &[],),
            Err(ModuleFailure::InvalidInput { .. })
        ));
        assert!(matches!(
            ModulePackage::parse(
                "conformance-domain",
                "nope",
                "siralos:domain-abi@1.0.0",
                &[],
            ),
            Err(ModuleFailure::InvalidInput { .. })
        ));
    }
}
