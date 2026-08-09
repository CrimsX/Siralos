import { canonicalizeJson, sha256Hex } from "../digest.js";
import { DEVELOPMENT_LIMITS } from "./development-model.js";

/**
 * Provider-neutral exact text change-set model (§17–§20).
 *
 * A change set contains bounded create/edit/delete operations for UTF-8
 * text files: every edit/delete requires the exact current SHA-256, every
 * create provides complete content, edits use exact text replacements
 * only (never regex, never provider-supplied unified diffs), no binary
 * files, no directory mutation, no paths outside the workspace, and the
 * existing protected-path restrictions apply unchanged. Preparation is
 * entirely read-only: it validates, reads, hashes, produces every
 * resulting file in memory, checks limits, and freezes an immutable
 * digest; approval binds to exactly that digest.
 */

export interface ChangeSetReplacement {
  readonly oldText: string;
  readonly newText: string;
}

/**
 * A change against an existing file carries exactly one pre-state
 * identity: either the raw SHA-256 (legacy path) or an opaque revision
 * handle issued by Solaris (preferred model-facing path). The handle is
 * resolved to its SHA-256 by the host and the same revalidation runs.
 * Handles are ergonomic references, never authority.
 */
export const WORKSPACE_REVISION_HANDLE_PATTERN = /^rev_[0-9a-f]{32}$/;

export type ChangeSetOperation =
  | {
      readonly operation: "edit";
      readonly path: string;
      readonly expectedSha256?: string;
      readonly expectedRevision?: string;
      readonly replacements: readonly ChangeSetReplacement[];
    }
  | {
      readonly operation: "create";
      readonly path: string;
      readonly content: string;
    }
  | {
      readonly operation: "delete";
      readonly path: string;
      readonly expectedSha256?: string;
      readonly expectedRevision?: string;
    };

/** Precise stale-state failure for revision-bound changes. */
export interface StaleRevisionError {
  readonly kind: "stale_revision";
  readonly path: string;
  readonly expectedRevision: string | null;
  /** Last known handle for the file's current state; null when unknown. */
  readonly currentRevision: string | null;
  /** Expected pre-state SHA-256 (trusted internal identity). */
  readonly expectedSha256: string;
}

export interface ChangeSetRequest {
  readonly changes: readonly ChangeSetOperation[];
}

/** One prepared file of a change set, frozen at preparation time. */
export interface PreparedChangeSetFile {
  readonly path: string;
  readonly operation: "create" | "update" | "delete";
  /** Expected pre-state hash (null only for create). */
  readonly expectedSha256: string | null;
  /** Complete resulting content (create/update); null for delete. */
  readonly content: string | null;
  /** Pre-state hash recorded during preparation. */
  readonly beforeSha256: string | null;
  /** Post-state hash recorded during preparation. */
  readonly afterSha256: string | null;
  /** Deterministic bounded unified diff (create/update); empty for delete. */
  readonly unifiedDiff: string;
  /** Preview line counts (also recorded on checkpoint metadata). */
  readonly addedLines: number;
  readonly removedLines: number;
}

export interface PreparedChangeSetDigestParts {
  readonly changes: readonly {
    readonly operation: "create" | "update" | "delete";
    readonly path: string;
    readonly beforeSha256: string | null;
    readonly afterSha256: string | null;
  }[];
}

export function computeChangeSetDigest(parts: PreparedChangeSetDigestParts): string {
  return sha256Hex(canonicalizeJson(parts));
}

export function validateChangeSetRequest(
  request: unknown,
):
  | { readonly ok: true; readonly request: ChangeSetRequest }
  | { readonly ok: false; readonly message: string } {
  if (typeof request !== "object" || request === null || Array.isArray(request)) {
    return { ok: false, message: "The change-set input must be an object with a changes array." };
  }
  const record = request as Record<string, unknown>;
  const changes = record["changes"];
  if (!Array.isArray(changes)) {
    return { ok: false, message: "The change-set input must contain a changes array." };
  }
  if (changes.length === 0) {
    return { ok: false, message: "The change set must contain at least one change." };
  }
  if (changes.length > DEVELOPMENT_LIMITS.maxFilesPerChangeSet) {
    return {
      ok: false,
      message: `A change set is limited to ${DEVELOPMENT_LIMITS.maxFilesPerChangeSet} files.`,
    };
  }
  const seenPaths = new Set<string>();
  const parsed: ChangeSetOperation[] = [];
  for (const change of changes) {
    if (typeof change !== "object" || change === null || Array.isArray(change)) {
      return { ok: false, message: "Every change must be an object." };
    }
    const entry = change as Record<string, unknown>;
    const operation = entry["operation"];
    if (operation !== "edit" && operation !== "create" && operation !== "delete") {
      return {
        ok: false,
        message: 'Every change must declare operation "edit", "create", or "delete".',
      };
    }
    const path = entry["path"];
    if (typeof path !== "string" || path.length === 0 || path.trim().length === 0) {
      return { ok: false, message: "Every change requires a non-empty workspace-relative path." };
    }
    const normalizedPath = path.trim();
    if (normalizedPath.includes(":")) {
      return {
        ok: false,
        message: `The path "${normalizedPath}" contains a colon; alternate-data-stream and drive-qualified paths are rejected.`,
      };
    }
    if (normalizedPath.endsWith(".tscn") || normalizedPath.endsWith(".tres")) {
      return {
        ok: false,
        message: `Scene and resource files (${normalizedPath}) are outside this milestone's change-set scope; only text source files can be changed.`,
      };
    }
    if (seenPaths.has(normalizedPath)) {
      return {
        ok: false,
        message: `The change set addresses the path "${normalizedPath}" more than once.`,
      };
    }
    seenPaths.add(normalizedPath);
    if (operation === "create") {
      const content = entry["content"];
      if (typeof content !== "string") {
        return {
          ok: false,
          message: `The create for "${normalizedPath}" requires string content.`,
        };
      }
      parsed.push({ operation, path: normalizedPath, content });
      continue;
    }
    const sha256Text: string =
      typeof entry["expectedSha256"] === "string" ? entry["expectedSha256"] : "";
    const revisionText: string =
      typeof entry["expectedRevision"] === "string" ? entry["expectedRevision"] : "";
    const expectedSha256: unknown = entry["expectedSha256"];
    const expectedRevision: unknown = entry["expectedRevision"];
    const sha256Present = typeof expectedSha256 === "string";
    const revisionPresent = typeof expectedRevision === "string";
    if (sha256Present && revisionPresent) {
      return {
        ok: false,
        message: `The ${operation} for "${normalizedPath}" must carry exactly one pre-state identity: either expectedSha256 or expectedRevision, not both.`,
      };
    }
    if (sha256Present) {
      if (!SHA256_PATTERN.test(sha256Text)) {
        return {
          ok: false,
          message: `The ${operation} for "${normalizedPath}" requires the exact 64-hex-digit current SHA-256.`,
        };
      }
    } else if (revisionPresent) {
      if (!WORKSPACE_REVISION_HANDLE_PATTERN.test(revisionText)) {
        return {
          ok: false,
          message: `The ${operation} for "${normalizedPath}" requires a valid Solaris revision handle (rev_...).`,
        };
      }
    } else {
      return {
        ok: false,
        message: `The ${operation} for "${normalizedPath}" requires a pre-state identity: the exact current SHA-256 or a Solaris revision handle.`,
      };
    }
    if (operation === "delete") {
      parsed.push({
        operation,
        path: normalizedPath,
        ...(sha256Present ? { expectedSha256: sha256Text } : { expectedRevision: revisionText }),
      });
      continue;
    }
    const replacements = entry["replacements"];
    if (!Array.isArray(replacements) || replacements.length === 0) {
      return {
        ok: false,
        message: `The edit for "${normalizedPath}" requires at least one exact replacement.`,
      };
    }
    if (replacements.length > DEVELOPMENT_LIMITS.maxReplacementsPerFile) {
      return {
        ok: false,
        message: `The edit for "${normalizedPath}" exceeds the limit of ${DEVELOPMENT_LIMITS.maxReplacementsPerFile} replacements.`,
      };
    }
    const parsedReplacements: ChangeSetReplacement[] = [];
    for (const replacement of replacements) {
      if (typeof replacement !== "object" || replacement === null || Array.isArray(replacement)) {
        return { ok: false, message: "Every replacement must be an object." };
      }
      const oldText = (replacement as Record<string, unknown>)["oldText"];
      const newText = (replacement as Record<string, unknown>)["newText"];
      if (typeof oldText !== "string" || typeof newText !== "string") {
        return {
          ok: false,
          message: `The edit for "${normalizedPath}" requires string oldText and newText replacements.`,
        };
      }
      if (oldText.length === 0) {
        return { ok: false, message: "Replacement oldText must not be empty." };
      }
      if (utf8ByteLength(oldText) > DEVELOPMENT_LIMITS.maxReplacementTextBytes) {
        return {
          ok: false,
          message: `A replacement for "${normalizedPath}" exceeds the replacement text limit.`,
        };
      }
      if (utf8ByteLength(newText) > DEVELOPMENT_LIMITS.maxReplacementTextBytes) {
        return {
          ok: false,
          message: `A replacement for "${normalizedPath}" exceeds the replacement text limit.`,
        };
      }
      parsedReplacements.push({ oldText, newText });
    }
    parsed.push({
      operation,
      path: normalizedPath,
      ...(sha256Present ? { expectedSha256: sha256Text } : { expectedRevision: revisionText }),
      replacements: parsedReplacements,
    });
  }
  return { ok: true, request: { changes: parsed } };
}

export function countChangeSetResultBytes(files: readonly PreparedChangeSetFile[]): number {
  let total = 0;
  for (const file of files) {
    if (file.operation === "create" || file.operation === "update") {
      total += utf8ByteLength(file.content ?? "");
    }
  }
  return total;
}

const SHA256_PATTERN = /^[0-9a-f]{64}$/;

const textEncoder = new TextEncoder();

function utf8ByteLength(text: string): number {
  return textEncoder.encode(text).length;
}
