import { lstat } from "node:fs/promises";
import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";
import { GODOT_LIMITS, REFERENCE_LIMITS, validateReferenceAlias } from "@siralos/core";
import { readFileBounded } from "../fs/file-read.js";
import { errorMessage } from "../support/error-message.js";

export type UserSandboxProfileId = "inspect" | "develop-offline";

export type UserSandboxBackendId = "auto" | "anthropic-runtime";

export interface UserSandboxConfig {
  readonly profile: UserSandboxProfileId;
  readonly backend: UserSandboxBackendId;
}

/** User-supplied edition hint. A hint only, never an authoritative result. */
export type UserGodotEditionHint = "standard" | "dotnet" | "unknown";

export interface UserGodotInstallationConfig {
  /** Absolute path to a Godot executable (or macOS application bundle). */
  readonly path: string;
  readonly editionHint: UserGodotEditionHint;
}

export interface UserGodotConfig {
  /** Must reference a configured or discovered installation id. */
  readonly activeInstallation: string | null;
  /** Immutable map of configured installations by id. */
  readonly installations: Readonly<Record<string, UserGodotInstallationConfig>>;
  /** Whether fixed-name PATH discovery is enabled. Defaults to true. */
  readonly discoverOnPath: boolean;
}

/**
 * Trusted user-level quality configuration (ADR 0013 §26). `reviewProvider`
 * optionally references an existing configured provider profile used for
 * the independent change reviewer; when absent the active development
 * provider profile is used. There is no new credential system: the
 * referenced profile must already be configured, and a missing profile
 * fails clearly instead of silently choosing an unrelated provider.
 */
export interface UserQualityConfig {
  readonly reviewProvider: string | null;
}

export interface UserConfig {
  readonly sandbox: UserSandboxConfig;
  readonly godot: UserGodotConfig;
  readonly quality: UserQualityConfig;
  /**
   * Declared external references, alias -> raw declaration. The shape is
   * validated defensively here (object keys, alias pattern, count bound,
   * per-kind required fields, unknown keys rejected at every level — a
   * credential field cannot hide); SEMANTIC validation (absolute paths,
   * repository normalization, ref shapes, bounds) is delegated to core's
   * `parseReferenceDeclarationsSection`, which the CLI feeds with the
   * canonical declaration form derived from this raw section.
   */
  readonly references: Readonly<Record<string, unknown>>;
}

export const DEFAULT_USER_CONFIG: UserConfig = {
  sandbox: {
    profile: "inspect",
    backend: "auto",
  },
  godot: {
    activeInstallation: null,
    installations: {},
    discoverOnPath: true,
  },
  quality: {
    reviewProvider: null,
  },
  references: {},
};

const SUPPORTED_PROFILES: readonly string[] = ["inspect", "develop-offline"];
const SUPPORTED_BACKENDS: readonly string[] = ["auto", "anthropic-runtime"];
const SUPPORTED_EDITION_HINTS: readonly string[] = ["standard", "dotnet", "unknown"];

export function getDefaultUserConfigPath(): string {
  return join(homedir(), ".siralos", "config.json");
}

/** Maximum user configuration file size (1 MiB). */
const MAX_CONFIG_FILE_BYTES = 1024 * 1024;

export async function loadUserConfig(configPath: string): Promise<UserConfig> {
  let stats;
  try {
    stats = await lstat(configPath);
  } catch (error: unknown) {
    if (isNotFoundError(error)) {
      return DEFAULT_USER_CONFIG;
    }
    throw new Error(
      `Cannot read Siralos configuration at ${configPath}: ${errorMessage(error, "unknown error")}`,
    );
  }
  if (stats.isSymbolicLink() || !stats.isFile()) {
    throw new Error(`Siralos configuration at ${configPath} is not a regular file.`);
  }
  if (stats.size > MAX_CONFIG_FILE_BYTES) {
    throw new Error(
      `Siralos configuration at ${configPath} exceeds the ${MAX_CONFIG_FILE_BYTES}-byte limit.`,
    );
  }
  const bytes = await readFileBounded(configPath, MAX_CONFIG_FILE_BYTES);
  if (bytes === null) {
    throw new Error(
      `Siralos configuration at ${configPath} could not be read within the ${MAX_CONFIG_FILE_BYTES}-byte limit.`,
    );
  }
  const content = bytes.toString("utf8");
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch (error: unknown) {
    throw new Error(
      `Siralos configuration at ${configPath} is not valid JSON: ${errorMessage(error, "unknown error")}`,
    );
  }
  return parseUserConfig(parsed);
}

export function parseUserConfig(data: unknown): UserConfig {
  if (typeof data !== "object" || data === null || Array.isArray(data)) {
    throw new Error("Siralos configuration must be a JSON object.");
  }
  const record = data as Record<string, unknown>;
  const unknownKeys = Object.keys(record).filter(
    (key) => key !== "sandbox" && key !== "godot" && key !== "quality" && key !== "references",
  );
  if (unknownKeys.length > 0) {
    throw new Error(`Unknown Siralos configuration section: ${unknownKeys[0]}.`);
  }
  const sandboxValue = record["sandbox"];
  if (sandboxValue === undefined) {
    return {
      ...DEFAULT_USER_CONFIG,
      godot: parseGodotSection(record["godot"]),
      quality: parseQualitySection(record["quality"]),
      references: parseReferencesSection(record["references"]),
    };
  }
  if (typeof sandboxValue !== "object" || sandboxValue === null || Array.isArray(sandboxValue)) {
    throw new Error('Siralos configuration section "sandbox" must be a JSON object.');
  }
  const sandbox = sandboxValue as Record<string, unknown>;
  const sandboxKeys = Object.keys(sandbox).filter((key) => key !== "profile" && key !== "backend");
  if (sandboxKeys.length > 0) {
    throw new Error(`Unknown Siralos sandbox configuration key: ${sandboxKeys[0]}.`);
  }
  const profile = sandbox["profile"] ?? "inspect";
  if (typeof profile !== "string" || !SUPPORTED_PROFILES.includes(profile)) {
    const profileLabel = typeof profile === "string" ? profile : JSON.stringify(profile);
    throw new Error(
      `Unknown sandbox profile: ${profileLabel}. Expected one of: inspect, develop-offline.`,
    );
  }
  const backend = sandbox["backend"] ?? "auto";
  if (typeof backend !== "string" || !SUPPORTED_BACKENDS.includes(backend)) {
    const backendLabel = typeof backend === "string" ? backend : JSON.stringify(backend);
    throw new Error(
      `Unknown sandbox backend: ${backendLabel}. Expected one of: auto, anthropic-runtime.`,
    );
  }
  return {
    sandbox: {
      profile: profile as UserSandboxProfileId,
      backend: backend as UserSandboxBackendId,
    },
    godot: parseGodotSection(record["godot"]),
    quality: parseQualitySection(record["quality"]),
    references: parseReferencesSection(record["references"]),
  };
}

function parseQualitySection(value: unknown): UserQualityConfig {
  if (value === undefined) {
    return DEFAULT_USER_CONFIG.quality;
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error('Siralos configuration section "quality" must be a JSON object.');
  }
  const quality = value as Record<string, unknown>;
  const qualityKeys = Object.keys(quality).filter((key) => key !== "reviewProvider");
  if (qualityKeys.length > 0) {
    throw new Error(`Unknown Siralos quality configuration key: ${qualityKeys[0]}.`);
  }
  const reviewProvider = parseReviewProvider(quality["reviewProvider"]);
  return { reviewProvider };
}

function parseReviewProvider(value: unknown): string | null {
  if (value === undefined || value === null) {
    return null;
  }
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 128 ||
    !/^[A-Za-z0-9._-]+$/.test(value)
  ) {
    throw new Error(
      "quality.reviewProvider must be a non-empty identifier (letters, digits, dot, dash, underscore) of at most 128 characters.",
    );
  }
  return value;
}

function parseGodotSection(value: unknown): UserGodotConfig {
  if (value === undefined) {
    return DEFAULT_USER_CONFIG.godot;
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error('Siralos configuration section "godot" must be a JSON object.');
  }
  const godot = value as Record<string, unknown>;
  const godotKeys = Object.keys(godot).filter(
    (key) => key !== "activeInstallation" && key !== "installations" && key !== "discoverOnPath",
  );
  if (godotKeys.length > 0) {
    throw new Error(`Unknown Siralos godot configuration key: ${godotKeys[0]}.`);
  }
  const activeInstallation = parseActiveInstallation(godot["activeInstallation"]);
  const installations = parseInstallations(godot["installations"]);
  const discoverOnPath = parseDiscoverOnPath(godot["discoverOnPath"]);
  return { activeInstallation, installations, discoverOnPath };
}

function parseActiveInstallation(value: unknown): string | null {
  if (value === undefined || value === null) {
    return null;
  }
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > GODOT_LIMITS.maxInstallationIdLength
  ) {
    throw new Error(
      `godot.activeInstallation must be a non-empty string of at most ${GODOT_LIMITS.maxInstallationIdLength} characters.`,
    );
  }
  return value;
}

function parseDiscoverOnPath(value: unknown): boolean {
  if (value === undefined) {
    return true;
  }
  if (typeof value !== "boolean") {
    throw new Error("godot.discoverOnPath must be a boolean.");
  }
  return value;
}

function parseInstallations(value: unknown): Readonly<Record<string, UserGodotInstallationConfig>> {
  if (value === undefined || value === null) {
    return {};
  }
  if (typeof value !== "object" || Array.isArray(value)) {
    throw new Error('Siralos configuration section "godot.installations" must be a JSON object.');
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length > GODOT_LIMITS.maxConfiguredInstallations) {
    throw new Error(
      `godot.installations is limited to ${GODOT_LIMITS.maxConfiguredInstallations} entries.`,
    );
  }
  const installations: Record<string, UserGodotInstallationConfig> = {};
  for (const [id, entry] of entries) {
    if (id.length === 0 || id.length > GODOT_LIMITS.maxInstallationIdLength) {
      throw new Error(
        `Godot installation id "${id}" must be non-empty and at most ${GODOT_LIMITS.maxInstallationIdLength} characters.`,
      );
    }
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) {
      throw new Error(`Godot installation "${id}" must be a JSON object.`);
    }
    const installation = entry as Record<string, unknown>;
    const installationKeys = Object.keys(installation).filter(
      (key) => key !== "path" && key !== "editionHint",
    );
    if (installationKeys.length > 0) {
      throw new Error(
        `Unknown Godot installation key: ${installationKeys[0]} (installation "${id}").`,
      );
    }
    const pathValue = installation["path"];
    if (typeof pathValue !== "string" || pathValue.length === 0) {
      throw new Error(`Godot installation "${id}" requires an absolute path.`);
    }
    if (!isAbsolute(pathValue)) {
      throw new Error(
        `Godot installation "${id}" path must be absolute: relative paths are rejected.`,
      );
    }
    const editionHint = installation["editionHint"] ?? "unknown";
    if (typeof editionHint !== "string" || !SUPPORTED_EDITION_HINTS.includes(editionHint)) {
      throw new Error(
        `Unknown Godot edition hint: ${JSON.stringify(editionHint)}. Expected one of: standard, dotnet, unknown.`,
      );
    }
    installations[id] = {
      path: pathValue,
      editionHint: editionHint as UserGodotEditionHint,
    };
  }
  return installations;
}

const REFERENCE_DECLARATION_KEYS = new Set(["kind", "path", "repository", "ref", "description"]);
const REFERENCE_REF_KEYS = new Set(["kind", "commit", "tag", "branch"]);

/**
 * Defensive structural parse of the `references` config section: a plain
 * object mapping validated alias -> declaration, bounded at
 * `REFERENCE_LIMITS.maxReferences` (16). Unknown keys are rejected at every
 * level (a credential field cannot hide). The parsed values are kept RAW
 * (unknown-typed) because SEMANTIC validation (absolute paths, repository
 * normalization, ref shape/bounds, description bytes) belongs to core's
 * `parseReferenceDeclarationsSection`; the CLI derives the canonical
 * declaration form from this section before calling it.
 */
function parseReferencesSection(value: unknown): Readonly<Record<string, unknown>> {
  if (value === undefined) {
    return {};
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error('Siralos configuration section "references" must be a JSON object.');
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (entries.length > REFERENCE_LIMITS.maxReferences) {
    throw new Error(
      `The "references" section declares ${entries.length} references; the limit is ${REFERENCE_LIMITS.maxReferences}.`,
    );
  }
  const references: Record<string, unknown> = {};
  for (const [alias, declaration] of entries) {
    if (validateReferenceAlias(alias) === null) {
      throw new Error(
        `Reference alias "${alias}" is malformed; aliases match ^[a-z][a-z0-9._-]{1,63}$.`,
      );
    }
    if (typeof declaration !== "object" || declaration === null || Array.isArray(declaration)) {
      throw new Error(`Reference "${alias}" must be a JSON object.`);
    }
    const record = declaration as Record<string, unknown>;
    const unknownKeys = Object.keys(record).filter((key) => !REFERENCE_DECLARATION_KEYS.has(key));
    if (unknownKeys.length > 0) {
      throw new Error(`Unknown Siralos reference key: ${unknownKeys[0]} (reference "${alias}").`);
    }
    const kind = record["kind"];
    if (kind !== "local-directory" && kind !== "repository") {
      throw new Error(`Reference "${alias}" requires "kind" of "local-directory" or "repository".`);
    }
    if (kind === "local-directory") {
      const path = record["path"];
      if (typeof path !== "string" || path.length === 0) {
        throw new Error(`Local-directory reference "${alias}" requires a non-empty "path".`);
      }
      if (record["repository"] !== undefined || record["ref"] !== undefined) {
        throw new Error(
          `Local-directory reference "${alias}" must not declare "repository" or "ref".`,
        );
      }
    } else {
      const repository = record["repository"];
      if (typeof repository !== "string" || repository.length === 0) {
        throw new Error(`Repository reference "${alias}" requires a non-empty "repository".`);
      }
      if (record["path"] !== undefined) {
        throw new Error(`Repository reference "${alias}" must not declare "path".`);
      }
      if (record["ref"] !== undefined) {
        record["ref"] = parseReferenceRef(record["ref"], alias);
      }
    }
    const description = record["description"];
    if (description !== undefined && typeof description !== "string") {
      throw new Error(`Reference "${alias}" description must be a string.`);
    }
    references[alias] = { ...record };
  }
  return references;
}

function parseReferenceRef(value: unknown, alias: string): unknown {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`Reference "${alias}" ref must be a JSON object.`);
  }
  const record = value as Record<string, unknown>;
  const unknownKeys = Object.keys(record).filter((key) => !REFERENCE_REF_KEYS.has(key));
  if (unknownKeys.length > 0) {
    throw new Error(`Unknown Siralos reference ref key: ${unknownKeys[0]} (reference "${alias}").`);
  }
  const kind = record["kind"];
  if (kind !== "commit" && kind !== "tag" && kind !== "branch") {
    throw new Error(`Reference "${alias}" ref requires "kind" of "commit", "tag", or "branch".`);
  }
  const pin = record[kind];
  if (typeof pin !== "string" || pin.length === 0) {
    throw new Error(`Reference "${alias}" ${kind} ref requires a non-empty ${kind} string.`);
  }
  for (const other of REFERENCE_REF_KEYS) {
    if (other !== "kind" && other !== kind && record[other] !== undefined) {
      throw new Error(
        `Reference "${alias}" ${kind} ref must not declare "${other}"; a ref pins exactly one of commit/tag/branch.`,
      );
    }
  }
  return { ...record };
}

function isNotFoundError(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}
