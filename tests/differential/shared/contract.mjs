/**
 * Versioned, bounded protocol contract for the differential harness.
 *
 * Corpus files are repository-controlled inputs, but the migration gate must
 * still fail closed when they are truncated, tampered with, or malformed. The
 * Rust loader mirrors every invariant declared here.
 */
import { lstatSync, readFileSync } from "node:fs";
import { basename, dirname, isAbsolute, resolve } from "node:path";
import { canonicalizeJson, sha256Hex } from "./canonical.mjs";

export const CORPUS_SCHEMA_VERSION = 3;
export const CORPUS_VERSION = 11;
export const ALLOWED_SUBJECTS = new Set([
  "state-dir",
  "version-identity",
  "task-contract",
  "workspace-read",
  "workspace-list",
  "workspace-search",
  "workspace-revision",
  "workspace-prepare",
  "checkpoint",
  "git-inspection",
  "language-diagnostics",
  "language-structure",
  "language-definition",
  "domain-lifecycle",
  "domain-capability",
]);
export const ALLOWED_PLATFORMS = new Set(["*", "windows", "posix"]);
export const ALLOWED_PARITY = new Set(["required", "informational"]);
export const ALLOWED_ENV_KEYS = new Set(["HOME", "HOMEDRIVE", "HOMEPATH", "USERPROFILE"]);

export const CONTRACT_LIMITS = Object.freeze({
  manifestBytes: 64 * 1024,
  scenarioBytes: 16 * 1024,
  scenarios: 256,
  identifierBytes: 128,
  fileNameBytes: 160,
  envEntries: 16,
  envKeyBytes: 64,
  envValueBytes: 4 * 1024,
  probeOutputBytes: 1024 * 1024,
  recordsBytes: 1024 * 1024,
  taskInputBytes: 8 * 1024,
  workspaceInputBytes: 64 * 1024,
  languageInputBytes: 64 * 1024,
  domainInputBytes: 64 * 1024,
});

const IDENTIFIER = /^[a-z0-9]+(?:[.-][a-z0-9]+)*$/u;
const ENV_KEY = /^[A-Z_][A-Z0-9_]*$/u;
const LOWER_SHA256 = /^[0-9a-f]{64}$/u;
const UTF8_DECODER = new TextDecoder("utf-8", { fatal: true });

/** Typed fixture-identity failure; corrupted fixtures never execute. */
export class CorpusIntegrityError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "CorpusIntegrityError";
    this.code = code;
  }
}

function integrity(code, message) {
  return new CorpusIntegrityError(code, message);
}

function byteLength(value) {
  return Buffer.byteLength(value, "utf8");
}

function assertPlainObject(value, label) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
}

function isPlainRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function assertExactKeys(value, expected, label) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} has unknown or missing fields`);
  }
}

function assertBoundedString(value, maximumBytes, label) {
  if (typeof value !== "string" || value.length === 0 || byteLength(value) > maximumBytes) {
    throw new Error(`${label} must be a non-empty string of at most ${maximumBytes} UTF-8 bytes`);
  }
}

/** Read a regular UTF-8 file without permitting symlink traversal. */
export function readBoundedUtf8File(path, maximumBytes, label) {
  const metadata = lstatSync(path);
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    throw new Error(`${label} must be a regular file`);
  }
  if (metadata.size > maximumBytes) {
    throw new Error(`${label} exceeds ${maximumBytes} bytes`);
  }
  const bytes = readFileSync(path);
  if (bytes.length > maximumBytes) {
    throw new Error(`${label} exceeds ${maximumBytes} bytes`);
  }
  try {
    return UTF8_DECODER.decode(bytes);
  } catch {
    throw new Error(`${label} is not valid UTF-8`);
  }
}

function parseJson(text, label) {
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(
      `${label} is not valid JSON: ${error instanceof Error ? error.message : error}`,
    );
  }
}

function assertCorpusDirectory(path, label) {
  const absolute = resolve(path);
  const metadata = lstatSync(absolute);
  if (metadata.isSymbolicLink()) {
    throw new Error(`${label} may not be a symlink`);
  }
  if (!metadata.isDirectory()) {
    throw new Error(`${label} must be a directory`);
  }
}

function validateManifestEntry(entry, index) {
  const label = `corpus manifest entry ${index}`;
  assertPlainObject(entry, label);
  if (!Object.hasOwn(entry, "sha256")) {
    throw integrity("MISSING_DIGEST", `${label}.sha256 is required`);
  }
  assertExactKeys(entry, ["file", "sha256"], label);
  assertBoundedString(entry.file, CONTRACT_LIMITS.fileNameBytes, `${label}.file`);
  if (
    isAbsolute(entry.file) ||
    entry.file !== basename(entry.file) ||
    !/^[a-z0-9][a-z0-9.-]*\.json$/u.test(entry.file)
  ) {
    throw new Error(`${label}.file must be a contained canonical JSON file name`);
  }
  if (typeof entry.sha256 !== "string" || !LOWER_SHA256.test(entry.sha256)) {
    throw integrity("MALFORMED_DIGEST", `${label}.sha256 must be a lowercase SHA-256 digest`);
  }
}

/** Canonical bytes covered by the manifest's overall corpus digest. */
export function corpusIdentityValue(manifest) {
  return {
    schemaVersion: manifest.schemaVersion,
    corpusVersion: manifest.corpusVersion,
    scenarios: manifest.scenarios,
  };
}

/** Overall corpus identity, excluding the digest field itself. */
export function computeCorpusDigest(manifest) {
  return sha256Hex(canonicalizeJson(corpusIdentityValue(manifest)));
}

function validatePlatforms(platforms, label) {
  if (!Array.isArray(platforms) || platforms.length === 0 || platforms.length > 2) {
    throw new Error(`${label}.platforms must contain one or two platform values`);
  }
  const seen = new Set();
  for (const platform of platforms) {
    if (typeof platform !== "string" || !ALLOWED_PLATFORMS.has(platform)) {
      throw new Error(`${label}.platforms contains an unsupported platform`);
    }
    if (seen.has(platform)) {
      throw new Error(`${label}.platforms contains a duplicate platform`);
    }
    seen.add(platform);
  }
  if (seen.has("*") && seen.size !== 1) {
    throw new Error(`${label}.platforms may not combine * with another platform`);
  }
}

/** Validate an environment fixture before it reaches a probe process. */
export function validateProbeEnvironment(env, label = "probe environment") {
  assertPlainObject(env, label);
  const entries = Object.entries(env);
  if (entries.length > CONTRACT_LIMITS.envEntries) {
    throw new Error(`${label} exceeds ${CONTRACT_LIMITS.envEntries} entries`);
  }
  for (const [key, value] of entries) {
    if (
      !ENV_KEY.test(key) ||
      byteLength(key) > CONTRACT_LIMITS.envKeyBytes ||
      !ALLOWED_ENV_KEYS.has(key)
    ) {
      throw new Error(`${label} contains unsupported key ${JSON.stringify(key)}`);
    }
    if (
      typeof value !== "string" ||
      value.includes("\0") ||
      byteLength(value) > CONTRACT_LIMITS.envValueBytes
    ) {
      throw new Error(
        `${label}.${key} must be a string of at most ${CONTRACT_LIMITS.envValueBytes} UTF-8 bytes`,
      );
    }
  }
}

function validateSubjectInputs(scenario, label) {
  const platforms = new Set(scenario.platforms);
  const envKeys = new Set(Object.keys(scenario.env));
  if (scenario.subject === "version-identity") {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(`${label} version-identity inputs must use platforms ["*"] and an empty env`);
    }
    return;
  }
  if (scenario.subject === "task-contract") {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(`${label} task-contract inputs must use platforms ["*"] and an empty env`);
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.taskInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.taskInputBytes} UTF-8 bytes`);
    }
    return;
  }
  const WORKSPACE_SUBJECTS = new Set([
    "workspace-read",
    "workspace-list",
    "workspace-search",
    "workspace-revision",
    "workspace-prepare",
    "checkpoint",
    "git-inspection",
  ]);
  if (WORKSPACE_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.workspaceInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.workspaceInputBytes} UTF-8 bytes`);
    }
    return;
  }
  const LANGUAGE_SUBJECTS = new Set([
    "language-diagnostics",
    "language-structure",
    "language-definition",
  ]);
  if (LANGUAGE_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.languageInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.languageInputBytes} UTF-8 bytes`);
    }
    return;
  }
  const DOMAIN_SUBJECTS = new Set(["domain-lifecycle", "domain-capability"]);
  if (DOMAIN_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.domainInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.domainInputBytes} UTF-8 bytes`);
    }
    return;
  }
  if (platforms.size !== 1 || platforms.has("*")) {
    throw new Error(`${label} state-dir inputs must target exactly one concrete platform`);
  }
  const windows = platforms.has("windows");
  const allowed = windows ? new Set(["USERPROFILE", "HOMEDRIVE", "HOMEPATH"]) : new Set(["HOME"]);
  for (const key of envKeys) {
    if (!allowed.has(key)) {
      throw new Error(`${label} env key ${key} does not apply to its platform`);
    }
  }
  if (
    scenario.parity === "required" &&
    ((!windows && (!Object.hasOwn(scenario.env, "HOME") || scenario.env.HOME === "")) ||
      (windows && !Object.hasOwn(scenario.env, "USERPROFILE")))
  ) {
    throw new Error(`${label} required parity must fully declare its home-resolution input`);
  }
}

/** Validate one parsed scenario and return it unchanged. */
export function validateScenario(scenario, file) {
  const label = `scenario ${file}`;
  assertPlainObject(scenario, label);
  const withInput = new Set([
    "task-contract",
    "workspace-read",
    "workspace-list",
    "workspace-search",
    "workspace-revision",
    "workspace-prepare",
    "checkpoint",
    "git-inspection",
    "language-diagnostics",
    "language-structure",
    "language-definition",
    "domain-lifecycle",
    "domain-capability",
  ]);
  const expectedKeys = withInput.has(scenario.subject)
    ? ["id", "subject", "platforms", "parity", "env", "input"]
    : ["id", "subject", "platforms", "parity", "env"];
  assertExactKeys(scenario, expectedKeys, label);
  assertBoundedString(scenario.id, CONTRACT_LIMITS.identifierBytes, `${label}.id`);
  if (!IDENTIFIER.test(scenario.id)) {
    throw new Error(`${label}.id is not canonical`);
  }
  if (`${scenario.id}.json` !== file) {
    throw new Error(`${label}.id must match its file name`);
  }
  if (typeof scenario.subject !== "string" || !ALLOWED_SUBJECTS.has(scenario.subject)) {
    throw new Error(`${label}.subject is unsupported`);
  }
  validatePlatforms(scenario.platforms, label);
  if (typeof scenario.parity !== "string" || !ALLOWED_PARITY.has(scenario.parity)) {
    throw new Error(`${label}.parity is unsupported`);
  }
  validateProbeEnvironment(scenario.env, `${label}.env`);
  validateSubjectInputs(scenario, label);
  return scenario;
}

/** Load, validate, and digest-bind a complete corpus. */
export function loadValidatedCorpus(corpusDir, platform) {
  if (platform !== "windows" && platform !== "posix") {
    throw new Error(`unsupported host platform ${JSON.stringify(platform)}`);
  }
  const root = resolve(corpusDir);
  assertCorpusDirectory(root, "corpus directory");
  const manifestText = readBoundedUtf8File(
    resolve(root, "manifest.json"),
    CONTRACT_LIMITS.manifestBytes,
    "corpus manifest",
  );
  const manifest = parseJson(manifestText, "corpus manifest");
  assertPlainObject(manifest, "corpus manifest");
  if (!Object.hasOwn(manifest, "corpusSha256")) {
    throw integrity("MISSING_DIGEST", "corpus manifest.corpusSha256 is required");
  }
  assertExactKeys(
    manifest,
    ["schemaVersion", "corpusVersion", "corpusSha256", "scenarios"],
    "corpus manifest",
  );
  if (manifest.schemaVersion !== CORPUS_SCHEMA_VERSION) {
    throw integrity(
      "UNSUPPORTED_VERSION",
      `unsupported corpus schemaVersion ${JSON.stringify(manifest.schemaVersion)}`,
    );
  }
  if (manifest.corpusVersion !== CORPUS_VERSION) {
    throw integrity(
      "UNSUPPORTED_VERSION",
      `unsupported corpusVersion ${JSON.stringify(manifest.corpusVersion)}`,
    );
  }
  if (typeof manifest.corpusSha256 !== "string" || !LOWER_SHA256.test(manifest.corpusSha256)) {
    throw integrity(
      "MALFORMED_DIGEST",
      "corpus manifest.corpusSha256 must be a lowercase SHA-256 digest",
    );
  }
  if (
    !Array.isArray(manifest.scenarios) ||
    manifest.scenarios.length === 0 ||
    manifest.scenarios.length > CONTRACT_LIMITS.scenarios
  ) {
    throw new Error(`corpus manifest must contain 1-${CONTRACT_LIMITS.scenarios} scenarios`);
  }
  const files = new Set();
  for (const [index, entry] of manifest.scenarios.entries()) {
    validateManifestEntry(entry, index);
    if (files.has(entry.file)) {
      throw new Error(`corpus manifest contains duplicate file ${entry.file}`);
    }
    files.add(entry.file);
  }
  const corpusDigest = computeCorpusDigest(manifest);
  if (corpusDigest !== manifest.corpusSha256) {
    throw integrity("CONTENT_MISMATCH", "corpus manifest does not match corpusSha256");
  }

  const ids = new Set();
  const scenarios = [];
  for (const entry of manifest.scenarios) {
    const path = resolve(root, entry.file);
    if (dirname(path) !== root) {
      throw new Error(`scenario file escapes the corpus: ${entry.file}`);
    }
    const text = readBoundedUtf8File(path, CONTRACT_LIMITS.scenarioBytes, `scenario ${entry.file}`);
    const scenario = validateScenario(parseJson(text, `scenario ${entry.file}`), entry.file);
    const digest = sha256Hex(canonicalizeJson(scenario));
    if (digest !== entry.sha256) {
      throw integrity(
        "CONTENT_MISMATCH",
        `scenario ${entry.file} does not match its manifest digest`,
      );
    }
    if (ids.has(scenario.id)) {
      throw new Error(`corpus manifest contains duplicate scenario id ${scenario.id}`);
    }
    ids.add(scenario.id);
    scenarios.push({
      ...scenario,
      file: entry.file,
      applicable: scenario.platforms.includes("*") || scenario.platforms.includes(platform),
    });
  }
  return {
    manifest,
    scenarios,
    corpusDigest,
  };
}
