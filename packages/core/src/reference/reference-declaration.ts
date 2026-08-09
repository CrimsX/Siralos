import {
  REFERENCE_LIMITS,
  type ReferenceKind,
  type ReferenceSource,
  type RepositoryRef,
} from "./reference-model.js";
import { validateReferenceAlias } from "./reference-model.js";

/**
 * Reference declaration parsing (Stage 3 milestone 5).
 *
 * Parses UNTRUSTED configuration JSON (user config or project config)
 * into the canonical `ReferenceDeclaration` form. Parsing is strict,
 * bounded, and rejects unknown keys — unknown keys are REJECTED so secret
 * values cannot hide inside a declaration. Nothing here touches the
 * filesystem or the network: resolution is the registry's job through the
 * resolver port.
 */

export interface ReferenceDeclaration {
  readonly alias: string;
  readonly kind: ReferenceKind;
  readonly source: ReferenceSource;
  readonly description: string | null;
}

export type ParseResult<T> =
  { readonly ok: true; readonly value: T } | { readonly ok: false; readonly reason: string };

export type ReferenceDeclarationParseResult = ParseResult<ReferenceDeclaration>;

const ALLOWED_DECLARATION_KEYS = new Set(["alias", "kind", "source", "description"]);
const ALLOWED_SOURCE_KEYS_LOCAL = new Set(["kind", "path"]);
const ALLOWED_SOURCE_KEYS_REPOSITORY = new Set(["kind", "repository", "ref"]);
const ALLOWED_REF_KEYS = new Set(["kind", "commit", "tag", "branch"]);

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function rejectUnknownKeys(
  record: Record<string, unknown>,
  allowed: ReadonlySet<string>,
  subject: string,
): string | null {
  for (const key of Object.keys(record)) {
    if (!allowed.has(key)) {
      return `Unknown key "${key}" in ${subject}; unknown keys are rejected.`;
    }
  }
  return null;
}

function requireString(
  record: Record<string, unknown>,
  key: string,
  subject: string,
): string | null {
  const value = record[key];
  if (typeof value !== "string") {
    return `${subject} requires a string "${key}".`;
  }
  return value;
}

/**
 * Normalize a repository origin to its canonical GitHub form.
 *
 * Accepts `owner/repo` (owner `[A-Za-z0-9-]+`, repo `[A-Za-z0-9._-]+`),
 * `https://github.com/owner/repo`, and `https://github.com/owner/repo.git`
 * — normalized to `https://github.com/owner/repo` (no `.git`, no trailing
 * slash). Rejects: any other host, `http://`, userinfo/credentials
 * (`user@`, `user:pass@`), query strings, fragments, empty owner/repo, and
 * path segments beyond owner/repo.
 */
export function normalizeRepositoryOrigin(
  input: string,
):
  { readonly ok: true; readonly origin: string } | { readonly ok: false; readonly reason: string } {
  if (typeof input !== "string" || input.trim().length === 0) {
    return { ok: false, reason: "A repository origin is required." };
  }
  const trimmed = input.trim();
  if (trimmed.length > REFERENCE_LIMITS.maxRepositoryLength) {
    return {
      ok: false,
      reason: `Repository origin exceeds the limit of ${REFERENCE_LIMITS.maxRepositoryLength} characters.`,
    };
  }
  if (trimmed.includes("\0")) {
    return { ok: false, reason: "Repository origins must not contain null bytes." };
  }
  if (trimmed.includes("#")) {
    return { ok: false, reason: "Repository origins must not contain a fragment." };
  }
  if (trimmed.includes("?")) {
    return { ok: false, reason: "Repository origins must not contain a query string." };
  }
  if (trimmed.includes("@")) {
    return {
      ok: false,
      reason: "Repository origins must not contain credentials (userinfo is rejected).",
    };
  }
  if (trimmed.startsWith("http://")) {
    return { ok: false, reason: "Repository origins must use https, not http." };
  }
  let rest = trimmed;
  if (rest.startsWith("https://")) {
    rest = rest.slice("https://".length);
    if (!rest.startsWith("github.com/")) {
      const host = rest.split("/", 1)[0] ?? "";
      return {
        ok: false,
        reason: `Unsupported repository host "${host}"; only github.com is supported.`,
      };
    }
    rest = rest.slice("github.com/".length);
  }
  rest = rest.replace(/\/+$/, "");
  if (rest.endsWith(".git")) {
    rest = rest.slice(0, -".git".length);
  }
  if (rest.length === 0) {
    return { ok: false, reason: "A repository origin must be exactly owner/repo." };
  }
  const segments = rest.split("/");
  if (segments.length !== 2) {
    return {
      ok: false,
      reason: "A repository origin must be exactly owner/repo with no additional path segments.",
    };
  }
  const owner = segments[0] as string;
  const repo = segments[1] as string;
  if (owner.length === 0 || !/^[A-Za-z0-9-]+$/.test(owner)) {
    return { ok: false, reason: `Invalid repository owner "${owner}".` };
  }
  if (repo.length === 0 || !/^[A-Za-z0-9._-]+$/.test(repo)) {
    return { ok: false, reason: `Invalid repository name "${repo}".` };
  }
  return { ok: true, origin: `https://github.com/${owner}/${repo}` };
}

/** POSIX absolute path, Windows drive path, or Windows UNC path. */
export function isAbsolutePath(path: string): boolean {
  if (path.startsWith("/")) {
    return true;
  }
  if (/^[A-Za-z]:[\\/]/.test(path)) {
    return true;
  }
  // UNC: \\server\share (also covers the \\?\ verbatim prefix).
  if (/^\\\\[^\\/]+[\\/][^\\/]+/.test(path)) {
    return true;
  }
  return false;
}

const COMMIT_PATTERN = /^[0-9a-fA-F]{7,64}$/;
const BRANCH_OR_TAG_PATTERN = /^[A-Za-z0-9._/-]+$/;

function parseRepositoryRef(value: unknown): ParseResult<RepositoryRef> {
  if (!isPlainObject(value)) {
    return { ok: false, reason: "A repository ref must be an object." };
  }
  const unknown = rejectUnknownKeys(value, ALLOWED_REF_KEYS, "reference ref");
  if (unknown !== null) {
    return { ok: false, reason: unknown };
  }
  const kind = value["kind"];
  if (kind !== "commit" && kind !== "tag" && kind !== "branch") {
    return {
      ok: false,
      reason: 'A repository ref requires "kind" of "commit", "tag", or "branch".',
    };
  }
  if (kind === "commit") {
    const commit = requireString(value, "commit", "A commit ref");
    if (commit === null) {
      return { ok: false, reason: "A commit ref requires a commit string." };
    }
    if (!COMMIT_PATTERN.test(commit)) {
      return {
        ok: false,
        reason: `The commit "${commit}" is malformed; commits are 7-64 hexadecimal characters.`,
      };
    }
    return { ok: true, value: { kind, commit } };
  }
  const pinKey = kind === "tag" ? "tag" : "branch";
  const pin = requireString(value, pinKey, `A ${pinKey} ref`);
  if (pin === null) {
    return { ok: false, reason: `A ${pinKey} ref requires a ${pinKey} string.` };
  }
  const maxLength =
    kind === "tag" ? REFERENCE_LIMITS.maxTagLength : REFERENCE_LIMITS.maxBranchLength;
  if (pin.length === 0 || pin.length > maxLength || !BRANCH_OR_TAG_PATTERN.test(pin)) {
    return {
      ok: false,
      reason: `The ${pinKey} "${pin}" is malformed; ${pinKey}s use letters, digits, ".", "_", "-", "/" and are at most ${maxLength} characters.`,
    };
  }
  return { ok: true, value: kind === "tag" ? { kind, tag: pin } : { kind, branch: pin } };
}

/**
 * Parse one untrusted reference declaration. Rejects unknown keys, missing
 * or malformed required fields, relative local paths, and out-of-bound
 * values. A repository declaration without `ref` defaults to the mutable
 * branch `main`; the registry refuses mutable refs unless
 * `allowMutableRefs` is set (the resolved commit is what is recorded).
 */
export function parseReferenceDeclaration(value: unknown): ReferenceDeclarationParseResult {
  if (!isPlainObject(value)) {
    return { ok: false, reason: "A reference declaration must be a plain JSON object." };
  }
  const unknown = rejectUnknownKeys(value, ALLOWED_DECLARATION_KEYS, "reference declaration");
  if (unknown !== null) {
    return { ok: false, reason: unknown };
  }
  const aliasValue = value["alias"];
  const alias = validateReferenceAlias(aliasValue);
  if (alias === null) {
    return {
      ok: false,
      reason: `The alias "${String(aliasValue)}" is malformed; aliases match ^[a-z][a-z0-9._-]{1,63}$.`,
    };
  }
  const kind = value["kind"];
  if (kind !== "local-directory" && kind !== "repository") {
    return {
      ok: false,
      reason: 'A reference declaration requires "kind" of "local-directory" or "repository".',
    };
  }
  const descriptionValue = value["description"];
  let description: string | null = null;
  if (descriptionValue !== undefined) {
    if (typeof descriptionValue !== "string") {
      return { ok: false, reason: "The reference description must be a string." };
    }
    if (new TextEncoder().encode(descriptionValue).length > REFERENCE_LIMITS.maxDescriptionBytes) {
      return {
        ok: false,
        reason: `The reference description exceeds the limit of ${REFERENCE_LIMITS.maxDescriptionBytes} bytes.`,
      };
    }
    description = descriptionValue;
  }
  const sourceValue = value["source"];
  if (!isPlainObject(sourceValue)) {
    return { ok: false, reason: "A reference declaration requires a source object." };
  }
  const sourceKind = sourceValue["kind"];
  if (sourceKind === "local-directory") {
    const sourceUnknownLocal = rejectUnknownKeys(
      sourceValue,
      ALLOWED_SOURCE_KEYS_LOCAL,
      "reference source",
    );
    if (sourceUnknownLocal !== null) {
      return { ok: false, reason: sourceUnknownLocal };
    }
    const path = requireString(sourceValue, "path", "A local-directory reference");
    if (path === null) {
      return { ok: false, reason: "A local-directory reference requires a path string." };
    }
    if (path.length === 0) {
      return { ok: false, reason: "A local-directory reference requires a non-empty path." };
    }
    if (path.length > REFERENCE_LIMITS.maxLocalDirectoryPathLength) {
      return {
        ok: false,
        reason: `The local-directory path exceeds the limit of ${REFERENCE_LIMITS.maxLocalDirectoryPathLength} characters.`,
      };
    }
    if (path.includes("\0")) {
      return { ok: false, reason: "The local-directory path must not contain null bytes." };
    }
    if (!isAbsolutePath(path)) {
      return {
        ok: false,
        reason: `The local-directory path "${path}" is not absolute; relative paths are not resolved.`,
      };
    }
    return {
      ok: true,
      value: { alias, kind, source: { kind: "local-directory", path }, description },
    };
  }
  if (sourceKind === "repository") {
    const sourceUnknownRepo = rejectUnknownKeys(
      sourceValue,
      ALLOWED_SOURCE_KEYS_REPOSITORY,
      "reference source",
    );
    if (sourceUnknownRepo !== null) {
      return { ok: false, reason: sourceUnknownRepo };
    }
    const repository = requireString(sourceValue, "repository", "A repository reference");
    if (repository === null) {
      return { ok: false, reason: "A repository reference requires a repository string." };
    }
    const normalized = normalizeRepositoryOrigin(repository);
    if (!normalized.ok) {
      return { ok: false, reason: normalized.reason };
    }
    const refValue = sourceValue["ref"];
    let ref: RepositoryRef;
    if (refValue === undefined) {
      // Absent ref defaults to the mutable branch "main": the registry
      // resolves it to a commit at resolution time and records THAT commit.
      // Without allowMutableRefs the registry refuses with a precise reason.
      ref = { kind: "branch", branch: "main" };
    } else {
      const parsedRef = parseRepositoryRef(refValue);
      if (!parsedRef.ok) {
        return { ok: false, reason: parsedRef.reason };
      }
      ref = parsedRef.value;
    }
    return {
      ok: true,
      value: {
        alias,
        kind,
        source: { kind: "repository", repository: normalized.origin, ref },
        description,
      },
    };
  }
  return {
    ok: false,
    reason: 'A reference source requires "kind" of "local-directory" or "repository".',
  };
}

/**
 * Parse the untrusted `reference` config section: a plain object mapping
 * alias -> declaration. Each key is validated as an alias, each value is
 * parsed with `parseReferenceDeclaration` and must repeat its key as
 * `alias`. Duplicate aliases are impossible by construction (object keys);
 * the count is bounded by `REFERENCE_LIMITS.maxReferences`.
 */
export function parseReferenceDeclarationsSection(
  value: unknown,
  limits: Partial<typeof REFERENCE_LIMITS> = {},
):
  | { readonly ok: true; readonly declarations: readonly ReferenceDeclaration[] }
  | { readonly ok: false; readonly reason: string } {
  const maxReferences = limits.maxReferences ?? REFERENCE_LIMITS.maxReferences;
  if (!isPlainObject(value)) {
    return {
      ok: false,
      reason: 'The "reference" config section must be a plain object mapping alias to declaration.',
    };
  }
  const keys = Object.keys(value);
  if (keys.length > maxReferences) {
    return {
      ok: false,
      reason: `The "reference" section declares ${keys.length} references; the limit is ${maxReferences}.`,
    };
  }
  const declarations: ReferenceDeclaration[] = [];
  for (const key of keys) {
    const alias = validateReferenceAlias(key);
    if (alias === null) {
      return {
        ok: false,
        reason: `The reference key "${key}" is not a valid alias (^[a-z][a-z0-9._-]{1,63}$).`,
      };
    }
    const parsed = parseReferenceDeclaration(value[key]);
    if (!parsed.ok) {
      return { ok: false, reason: `Reference "${key}": ${parsed.reason}` };
    }
    if (parsed.value.alias !== key) {
      return {
        ok: false,
        reason: `Reference "${key}": the declared alias "${parsed.value.alias}" does not match its key.`,
      };
    }
    declarations.push(parsed.value);
  }
  return { ok: true, declarations };
}
