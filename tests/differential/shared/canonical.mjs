/**
 * Canonical JSON and SHA-256 helpers for the differential harness.
 *
 * The serialization semantics MUST match `canonicalizeJson` /
 * `sha256Hex` in `@siralos/core` (packages/core/src/godot/digest.ts) —
 * sorted keys, compact separators, arrays in order — and the Rust
 * candidate must emit byte-identical canonical JSON (serde_json over
 * BTreeMap-backed values). This module is harness tooling; the
 * authoritative implementations live in `@siralos/core`.
 */
import { createHash } from "node:crypto";

/** Deterministic canonical serialization: sorted keys, no whitespace. */
export function canonicalizeJson(value) {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map((entry) => canonicalizeJson(entry)).join(",")}]`;
  }
  const record = value;
  const keys = Object.keys(record).sort();
  return `{${keys.map((key) => `${JSON.stringify(key)}:${canonicalizeJson(record[key])}`).join(",")}}`;
}

/** Lowercase hex SHA-256 of UTF-8 bytes. */
export function sha256Hex(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

/** Lowercase hex SHA-256 of raw bytes. */
export function sha256HexBytes(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
