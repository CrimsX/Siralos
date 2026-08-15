/**
 * Domain package identity (Stage 3R R6).
 *
 * A domain package is bound to a stable package identifier, the exact
 * SHA-256 digest of the component bytes the Host accepted, and a
 * versioned ABI identity. The Host computes and verifies the digest
 * itself; filename, directory name, declared version, mtime, and
 * caller-provided digests are never trusted.
 */

import { parseCapabilityRequest, type CapabilityRequest } from "./capability.js";
import { invalidInput, type DomainFailure, type ParseResult } from "./failure.js";

/** Maximum length of a package identifier in bytes. */
export const MAX_PACKAGE_ID_BYTES = 128;

/** Maximum length of a canonical ABI string in bytes. */
export const MAX_ABI_BYTES = 128;

function validSeparatedIdentifier(value: string): boolean {
  if (value.length === 0 || value.length > MAX_PACKAGE_ID_BYTES) {
    return false;
  }
  let previousSeparator = false;
  for (let index = 0; index < value.length; index += 1) {
    const byte = value.charCodeAt(index);
    const separator = byte === 0x2e || byte === 0x2d; // '.' or '-'
    const lower = byte >= 0x61 && byte <= 0x7a; // a-z
    const digit = byte >= 0x30 && byte <= 0x39; // 0-9
    if (!(lower || digit || separator) || (separator && (index === 0 || previousSeparator))) {
      return false;
    }
    previousSeparator = separator;
  }
  return !previousSeparator;
}

function validDigest(value: string): boolean {
  return /^[0-9a-f]{64}$/u.test(value);
}

function validAbi(value: string): boolean {
  if (value.length === 0 || value.length > MAX_ABI_BYTES) {
    return false;
  }
  const at = value.indexOf("@");
  if (at === -1 || value.indexOf("@", at + 1) !== -1) {
    return false;
  }
  const name = value.slice(0, at);
  const version = value.slice(at + 1);
  if (name.length === 0 || version.length === 0 || !name.includes(":")) {
    return false;
  }
  const nameOk = name.split(":").every((segment) => validSeparatedIdentifier(segment));
  const parts = version.split(".");
  const versionOk =
    parts.length >= 2 &&
    parts.length <= 3 &&
    parts.every((part) => part.length > 0 && /^[0-9]+$/u.test(part));
  return nameOk && versionOk;
}

/** A validated stable package identifier. */
export type DomainPackageId = string;

/**
 * Exact package digest: lowercase hex SHA-256 over the accepted
 * component bytes (64 hex characters).
 */
export type PackageDigest = string;

/**
 * Versioned ABI identity in canonical WIT package form
 * (`name@major.minor.patch`, for example `siralos:domain-abi@1.0.0`).
 * Compatibility is exact equality: unknown or incompatible versions
 * fail closed and are never downgraded, reinterpreted, or best-effort
 * deserialized (docs/development/PROTOCOL_VERSIONING.md).
 */
export type DomainAbi = string;

/** Parse a canonical package identifier. */
export function parseDomainPackageId(value: unknown): ParseResult<DomainPackageId> {
  if (typeof value !== "string" || !validSeparatedIdentifier(value)) {
    return invalidInput("invalid package id");
  }
  return { ok: true, value };
}

/** Parse a canonical hex SHA-256 digest. */
export function parsePackageDigest(value: unknown): ParseResult<PackageDigest> {
  if (typeof value !== "string" || !validDigest(value)) {
    return invalidInput("invalid package digest");
  }
  return { ok: true, value };
}

/** Parse a canonical ABI string. */
export function parseDomainAbi(value: unknown): ParseResult<DomainAbi> {
  if (typeof value !== "string" || !validAbi(value)) {
    return invalidInput("invalid domain ABI");
  }
  return { ok: true, value };
}

/**
 * Exact ABI compatibility. Hard-incompatible versions never match:
 * there is no downgrade, guess, or partial match.
 */
export function abiIsCompatible(a: DomainAbi, b: DomainAbi): boolean {
  return a === b;
}

/**
 * A locally supplied, Host-accepted domain package descriptor. All
 * fields are validated and detached; the digest is the exact digest of
 * the bytes the Host accepted, computed by the Host.
 */
export interface DomainPackage {
  readonly id: DomainPackageId;
  readonly digest: PackageDigest;
  readonly abi: DomainAbi;
  /** The capabilities this package declares it wants before activation. */
  readonly requestedCapabilities: CapabilityRequest;
}

/** Parse a package descriptor from untrusted values. */
export function parseDomainPackage(
  id: unknown,
  digest: unknown,
  abi: unknown,
  requestedCapabilities: unknown,
): ParseResult<DomainPackage> {
  const parsedId = parseDomainPackageId(id);
  if (!parsedId.ok) {
    return parsedId;
  }
  const parsedDigest = parsePackageDigest(digest);
  if (!parsedDigest.ok) {
    return parsedDigest;
  }
  const parsedAbi = parseDomainAbi(abi);
  if (!parsedAbi.ok) {
    return parsedAbi;
  }
  const parsedRequest = parseCapabilityRequest(requestedCapabilities);
  if (!parsedRequest.ok) {
    return parsedRequest;
  }
  return {
    ok: true,
    value: {
      id: parsedId.value,
      digest: parsedDigest.value,
      abi: parsedAbi.value,
      requestedCapabilities: parsedRequest.value,
    },
  };
}

/**
 * Host verification of the declared digest against the digest the Host
 * computed from the exact accepted component bytes. Returns the typed
 * identity-mismatch failure on any difference.
 */
export function verifyPackageDigest(
  declared: PackageDigest,
  computed: PackageDigest,
): DomainFailure | null {
  if (declared === computed) {
    return null;
  }
  return {
    code: "IDENTITY_MISMATCH",
    detail: "package digest does not match the accepted component bytes",
  };
}
