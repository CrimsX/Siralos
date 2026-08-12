import { canonicalizeJson, sha256Hex } from "../godot/digest.js";

/**
 * Canonical artifact digest primitive (Stage 3 — Content Identity &
 * Delta Verification, ADR 0028).
 *
 * One shared typed digest for structured Siralos artifacts:
 *
 *   SHA-256("siralos:<ArtifactType>:v<SchemaVersion>\0" + canonicalPayload)
 *
 * The domain separator guarantees that two artifact types can never
 * collide by representation reuse, and the canonical JSON payload makes
 * identical semantic values produce identical digests regardless of
 * object-key insertion order.
 *
 * Digest semantics are strictly content identity:
 *
 *   digest match ≠ trusted | approved | authorized
 *
 * Capability policy, approvals, provenance, sandbox, and evidence
 * confidence remain authoritative. Workspace stale-write protection
 * keeps hashing exact source bytes — source bytes are never normalized
 * before those hashes.
 */

export const ARTIFACT_DIGEST_ALGORITHM = "sha256" as const;

export interface ArtifactDigest {
  readonly algorithm: typeof ARTIFACT_DIGEST_ALGORITHM;
  /** Domain separator, e.g. "TaskContract" or "GuidanceManifest". */
  readonly artifactType: string;
  /** Schema version of the canonical payload (not a content revision). */
  readonly schemaVersion: number;
  /** 64 lowercase hex characters. */
  readonly value: string;
}

const ARTIFACT_TYPE_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;

/** Canonical payload string for an artifact (domain-separated). */
export function canonicalArtifactPayload(
  artifactType: string,
  schemaVersion: number,
  payload: unknown,
): string {
  if (!ARTIFACT_TYPE_PATTERN.test(artifactType)) {
    throw new Error(`Invalid artifact type: ${artifactType}`);
  }
  if (!Number.isSafeInteger(schemaVersion) || schemaVersion < 1) {
    throw new Error("An artifact schema version must be a positive safe integer.");
  }
  return `siralos:${artifactType}:v${schemaVersion}\0${canonicalizeJson(payload)}`;
}

/** Hex digest of a domain-separated artifact (canonical JSON payload). */
export function computeArtifactDigestHex(input: {
  readonly artifactType: string;
  readonly schemaVersion: number;
  readonly payload: unknown;
}): string {
  return sha256Hex(
    canonicalArtifactPayload(input.artifactType, input.schemaVersion, input.payload),
  );
}

/** Typed digest of a domain-separated artifact. */
export function computeArtifactDigest(input: {
  readonly artifactType: string;
  readonly schemaVersion: number;
  readonly payload: unknown;
}): ArtifactDigest {
  return {
    algorithm: ARTIFACT_DIGEST_ALGORITHM,
    artifactType: input.artifactType,
    schemaVersion: input.schemaVersion,
    value: computeArtifactDigestHex(input),
  };
}

/** Validates and detaches a digest at a runtime boundary. */
export function validateArtifactDigest(input: ArtifactDigest): ArtifactDigest {
  if (input.algorithm !== ARTIFACT_DIGEST_ALGORITHM) {
    throw new Error(`Unsupported digest algorithm: ${String(input.algorithm)}`);
  }
  if (!ARTIFACT_TYPE_PATTERN.test(input.artifactType)) {
    throw new Error(`Invalid artifact type: ${input.artifactType}`);
  }
  if (!Number.isSafeInteger(input.schemaVersion) || input.schemaVersion < 1) {
    throw new Error("An artifact schema version must be a positive safe integer.");
  }
  if (!SHA256_HEX_PATTERN.test(input.value)) {
    throw new Error("An artifact digest value must be 64 lowercase hex characters.");
  }
  return { ...input };
}

export function sameArtifactDigest(a: ArtifactDigest, b: ArtifactDigest): boolean {
  return (
    a.algorithm === b.algorithm &&
    a.artifactType === b.artifactType &&
    a.schemaVersion === b.schemaVersion &&
    a.value === b.value
  );
}

/** Stable reference form, e.g. `sha256:abc123...`. */
export function digestReference(digest: ArtifactDigest): string {
  return `${digest.algorithm}:${digest.value}`;
}

/** Compact display form for status output; full values stay in diagnostics. */
export function abbreviateDigest(digest: ArtifactDigest, prefixLength = 8): string {
  return digest.value.slice(0, prefixLength);
}

/** Compact display form over a hex digest. */
export function abbreviateHexDigest(value: string, prefixLength = 8): string {
  return value.slice(0, prefixLength);
}
