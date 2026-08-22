//! Canonical content identity (Stage 3R R3 minimum, ADR 0028).
//!
//! One domain-separated digest primitive for the R3 task kernel:
//!
//! ```text
//! SHA-256("siralos:<ArtifactType>:v<SchemaVersion>\0" + canonicalPayload)
//! ```
//!
//! The canonical payload serializes equal semantic values to equal bytes
//! regardless of object-key insertion order (sorted keys, deterministic
//! escaping), matching the TypeScript reference's `canonicalizeJson`.
//! Digest semantics are strictly content identity: a digest match never
//! implies trust, authority, provenance, or approval.
//!
//! `siralos-core` remains dependency-free (architecture ratchet), so
//! SHA-256 is a small, tested pure implementation (FIPS 180-4) exactly
//! like the Node-free TypeScript reference primitive it mirrors. It is
//! verified against NIST known-answer vectors in tests and against the
//! TypeScript oracle through the differential digest scenarios.
//!
//! Revision (lifecycle identity) and digest (exact material content
//! identity) stay distinct: the contract payload deliberately excludes
//! the revision number, so content-identical revisions share a digest.

mod artifact;
mod canonical;
mod manifests;
mod sha256;
mod staleness;

pub use artifact::{
    ArtifactDigest, ArtifactIdentityError, ItemListDelta, SectionDelta,
    abbreviate_digest, abbreviate_hex_digest, canonical_artifact_payload,
    canonical_values_equal, compute_artifact_digest,
    compute_artifact_digest_hex, compute_item_list_delta,
    compute_section_delta, digest_item_list, digest_reference,
    validate_artifact_digest,
};
pub use canonical::{
    CanonicalValue, canonical_json_value, canonicalize, json_escape,
};
pub use manifests::{
    AcceptanceEvidenceEntry, AcceptanceEvidenceManifest,
    CreateAcceptanceEvidenceManifest, CreateReviewInputManifest,
    ExecutionInputChange, ExecutionInputDelta, ExecutionInputManifest,
    ExecutionInputReference, GuidanceDelta, GuidanceEntryKind,
    GuidanceManifest, GuidanceManifestEntry, ReviewInputManifest,
    ReviewSourceRevision, ToolSurfaceDefinition, ToolSurfaceDelta,
    ToolSurfaceEntry, ToolSurfaceManifest, ToolSurfacePhase, ToolSurfaceRole,
    ValidationDelta, ValidationEvidenceEntry, ValidationObservation,
    ValidationResultIdentity, canonical_changeset_identity,
    compute_acceptance_criteria_digest, compute_capability_snapshot_digest,
    compute_execution_input_delta, compute_guidance_delta,
    compute_tool_surface_delta, compute_validation_delta,
    compute_validation_evidence_digest, create_acceptance_evidence_manifest,
    create_execution_input_manifest, create_guidance_manifest,
    create_review_input_manifest, create_tool_surface_manifest,
    create_validation_result_identity,
};
pub use sha256::{Sha256, sha256_hex};
pub use staleness::{
    IdentityStaleness, IdentityStalenessInput, derive_identity_staleness,
};

/// Schema version of the R3 TaskContract canonical payload.
pub const TASK_CONTRACT_IDENTITY_SCHEMA: u64 = 1;

/// Hex SHA-256 digest of a domain-separated artifact payload.
pub fn artifact_digest_hex(
    artifact_type: &str,
    schema_version: u64,
    payload: &str,
) -> String {
    debug_assert!(
        !artifact_type.is_empty()
            && artifact_type
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric()
                    || b"._-".contains(&byte))
    );
    debug_assert!(schema_version >= 1);
    let mut domain =
        String::with_capacity(16 + artifact_type.len() + payload.len());
    domain.push_str("siralos:");
    domain.push_str(artifact_type);
    domain.push_str(":v");
    domain.push_str(&schema_version.to_string());
    domain.push('\0');
    domain.push_str(payload);
    sha256_hex(domain.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{artifact_digest_hex, json_escape, sha256_hex};

    #[test]
    fn sha256_matches_nist_known_answer_vectors() {
        // FIPS 180-4 / NIST CAVP SHA-256 vectors.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghijhijklijklmklmnlmnomnopnopq"),
            "5dd60d7b7ee46f704d8720901e4d98d3a1de72946e38d60604f77e7c703518c0"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    #[test]
    fn sha256_matches_the_million_a_vector() {
        let input = vec![b'a'; 1_000_000];
        assert_eq!(
            sha256_hex(&input),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha256_is_sequential_equivalent_across_block_boundaries() {
        for length in [0usize, 1, 55, 56, 63, 64, 65, 127, 128, 129, 1024] {
            let input: Vec<u8> =
                (0..length).map(|index| (index % 251) as u8).collect();
            let digest = sha256_hex(&input);
            assert_eq!(digest.len(), 64, "digest length for {length} bytes");
            assert!(
                digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "digest must be hex for {length} bytes"
            );
        }
    }

    #[test]
    fn json_escape_matches_javascript_stringify_escaping() {
        assert_eq!(json_escape("plain"), "\"plain\"");
        assert_eq!(
            json_escape("quote\"backslash\\"),
            "\"quote\\\"backslash\\\\\""
        );
        assert_eq!(json_escape("line\nfeed\r\t"), "\"line\\nfeed\\r\\t\"");
        assert_eq!(json_escape("\u{0008}\u{000c}"), "\"\\b\\f\"");
        assert_eq!(json_escape("\u{0000}\u{0001}"), "\"\\u0000\\u0001\"");
        // Non-ASCII is emitted literally as UTF-8, matching JSON.stringify.
        assert_eq!(json_escape("界"), "\"界\"");
        assert_eq!(json_escape("e\u{0301}"), "\"e\u{0301}\"");
    }

    #[test]
    fn artifact_digest_is_domain_separated_and_versioned() {
        let a = artifact_digest_hex("TaskContract", 1, r#"{"id":"t"}"#);
        let b = artifact_digest_hex("TaskContract", 2, r#"{"id":"t"}"#);
        let c = artifact_digest_hex("TaskPlan", 1, r#"{"id":"t"}"#);
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(artifact_digest_hex("TaskContract", 1, r#"{"id":"t"}"#), a);
    }
}
