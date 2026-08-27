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
export const CORPUS_VERSION = 30;
export const ALLOWED_SUBJECTS = new Set([
  "state-dir",
  "version-identity",
  "task-contract",
  "workspace-read",
  "workspace-list",
  "workspace-search",
  "workspace-revision",
  "workspace-prepare",
  "workspace-apply",
  "checkpoint",
  "git-inspection",
  "language-diagnostics",
  "language-structure",
  "language-definition",
  "domain-lifecycle",
  "domain-capability",
  "provider-turn",
  "tool-loop",
  "context-projection",
  "user-config",
  "security-permissions",
  "command-catalog",
  "instructions-resolution",
  "knowledge-revisions",
  "reference-identity",
  "research-policy",
  "planning-runtime",
  "executor-brief",
  "capability-doctor",
  "godot-scene-resolve",
  "godot-discovery",
  "godot-knowledge",
  "godot-diagnostics",
  "godot-lsp",
  "godot-review-context",
  "godot-mutation-prepare",
  "godot-develop-plan",
  "content-identity-artifact-digest",
  "content-identity-contract-digest",
  "content-identity-manifests",
  "content-identity-delta",
  "determinism-replay",
  "icm.phase-contract",
  "icm.dependency-manifests",
  "runtime-readiness.identity",
  "runtime-readiness.budgets",
  "runtime-readiness.lifecycle",
  "runtime-readiness.doctor",
  "recovery-taxonomy",
  "cli-session",
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
  recordsBytes: 2 * 1024 * 1024,
  taskInputBytes: 8 * 1024,
  workspaceInputBytes: 64 * 1024,
  languageInputBytes: 64 * 1024,
  domainInputBytes: 64 * 1024,
  providerInputBytes: 64 * 1024,
  toolLoopInputBytes: 64 * 1024,
  contextProjectionInputBytes: 64 * 1024,
  userConfigInputBytes: 64 * 1024,
  r13AuthorityInputBytes: 64 * 1024,
  r13GuidanceInputBytes: 64 * 1024,
  r13ExternalKnowledgeInputBytes: 64 * 1024,
  r13PlanningBriefingInputBytes: 64 * 1024,
  cliSessionInputBytes: 64 * 1024,
  godotInputBytes: 64 * 1024,
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
      { cause: error },
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

/** A valid deterministic '$repeat' materialization marker. */
function isRepeatMarker(value) {
  if (!isPlainRecord(value) || !Object.hasOwn(value, "$repeat")) {
    return false;
  }
  const repeat = value.$repeat;
  if (
    !isPlainRecord(repeat) ||
    !Object.hasOwn(repeat, "character") ||
    !Object.hasOwn(repeat, "count")
  ) {
    return false;
  }
  if (typeof repeat.character !== "string" || [...repeat.character].length !== 1) {
    return false;
  }
  return Number.isSafeInteger(repeat.count) && repeat.count >= 0 && repeat.count <= 1_048_576;
}

/** Validate the strict provider-turn scenario input shape. */
function validateProviderTurnInput(input, label) {
  assertExactKeys(input, ["cases"], `${label}.input`);
  if (!Array.isArray(input.cases) || input.cases.length === 0 || input.cases.length > 32) {
    throw new Error(`${label}.input.cases must contain 1-32 entries`);
  }
  for (const [index, entry] of input.cases.entries()) {
    const caseLabel = `${label}.input.cases[${index}]`;
    assertPlainObject(entry, caseLabel);
    const hasTurn = Object.hasOwn(entry, "turn");
    const hasDetach = Object.hasOwn(entry, "detach");
    if (hasTurn === hasDetach) {
      throw new Error(`${caseLabel} must have exactly one of turn/detach`);
    }
    if (hasTurn) {
      validateProviderTurnCase(entry.turn, caseLabel);
    } else {
      validateProviderDetachCase(entry.detach, caseLabel);
    }
  }
}

function validateProviderTurnCase(caseValue, label) {
  assertPlainObject(caseValue, label);
  const allowed = new Set(["provider", "messages", "tools", "cancelAfterEvents"]);
  for (const key of Object.keys(caseValue)) {
    if (!allowed.has(key)) {
      throw new Error(`${label} has an unknown field ${JSON.stringify(key)}`);
    }
  }
  for (const required of ["provider", "messages", "tools"]) {
    if (!Object.hasOwn(caseValue, required)) {
      throw new Error(`${label} requires the ${required} field`);
    }
  }
  if (Object.hasOwn(caseValue, "cancelAfterEvents")) {
    if (
      !Number.isSafeInteger(caseValue.cancelAfterEvents) ||
      caseValue.cancelAfterEvents < 0 ||
      caseValue.cancelAfterEvents > 1024
    ) {
      throw new Error(`${label}.cancelAfterEvents must be an integer from 0 to 1024`);
    }
  }
  const provider = caseValue.provider;
  assertPlainObject(provider, `${label}.provider`);
  if (provider.kind === "fake") {
    assertExactKeys(provider, ["kind"], `${label}.provider`);
  } else if (provider.kind === "scripted") {
    assertExactKeys(provider, ["kind", "events"], `${label}.provider`);
    if (!Array.isArray(provider.events) || provider.events.length > 4096) {
      throw new Error(`${label}.provider.events must be a bounded array`);
    }
    // Events are untrusted raw data: any JSON value is admissible and
    // the production collector validation decides malformed events.
  } else {
    throw new Error(`${label}.provider.kind must be fake or scripted`);
  }
  if (!Array.isArray(caseValue.messages) || caseValue.messages.length > 128) {
    throw new Error(`${label}.messages must be a bounded array`);
  }
  for (const [index, item] of caseValue.messages.entries()) {
    validateProviderMessage(item, `${label}.messages[${index}]`);
  }
  if (!Array.isArray(caseValue.tools) || caseValue.tools.length > 128) {
    throw new Error(`${label}.tools must be a bounded array`);
  }
  for (const [index, tool] of caseValue.tools.entries()) {
    assertExactKeys(tool, ["name", "description", "inputSchema"], `${label}.tools[${index}]`);
    if (typeof tool.name !== "string" || typeof tool.description !== "string") {
      throw new Error(`${label}.tools[${index}] name and description must be strings`);
    }
  }
}

function validMessageString(value) {
  return typeof value === "string" || isRepeatMarker(value);
}

function validateProviderMessage(item, label) {
  assertPlainObject(item, label);
  const itemType = item.type;
  if (itemType === "user_message" || itemType === "assistant_message") {
    assertExactKeys(item, ["type", "content"], label);
    if (!validMessageString(item.content)) {
      throw new Error(`${label}.content must be a string or repeat marker`);
    }
    return;
  }
  if (itemType === "assistant_tool_call") {
    assertExactKeys(item, ["type", "callId", "toolName", "input"], label);
    if (!validMessageString(item.callId) || !validMessageString(item.toolName)) {
      throw new Error(`${label} requires string callId and toolName`);
    }
    return;
  }
  if (itemType === "tool_result") {
    assertExactKeys(item, ["type", "callId", "toolName", "result"], label);
    if (!validMessageString(item.callId) || !validMessageString(item.toolName)) {
      throw new Error(`${label} requires string callId and toolName`);
    }
    if (!isPlainRecord(item.result)) {
      throw new Error(`${label}.result must be an object`);
    }
    return;
  }
  throw new Error(`${label} has an unknown type ${JSON.stringify(itemType)}`);
}

function validateProviderDetachCase(detach, label) {
  assertExactKeys(detach, ["value", "maxBytes", "actor"], label);
  if (
    !Number.isSafeInteger(detach.maxBytes) ||
    detach.maxBytes < 1 ||
    detach.maxBytes > 1_048_576
  ) {
    throw new Error(`${label}.maxBytes must be an integer from 1 to 1048576`);
  }
  if (
    typeof detach.actor !== "string" ||
    detach.actor.length === 0 ||
    byteLength(detach.actor) > 128
  ) {
    throw new Error(`${label}.actor must be a non-empty string of at most 128 UTF-8 bytes`);
  }
}

const TOOL_LOOP_TOOL_NAMES = new Set([
  "workspace.read",
  "stub.success",
  "stub.invalid_input",
  "stub.denied",
  "stub.failed",
  "stub.cancelled",
  "b.tool",
  "a.tool",
]);
const TOOL_LOOP_CAPABILITY = /^[a-z0-9]+(?:[._-][a-z0-9]+)*$/u;

function validateBoundedToolNameArray(value, maximum, label) {
  if (!Array.isArray(value) || value.length > maximum) {
    throw new Error(`${label} must be a bounded array`);
  }
  for (const name of value) {
    if (typeof name !== "string" || name.length === 0 || byteLength(name) > 256) {
      throw new Error(`${label} entries must be non-empty tool names`);
    }
  }
}

function validateToolLoopProvider(provider, label) {
  assertPlainObject(provider, label);
  if (provider.kind === "fake") {
    assertExactKeys(provider, ["kind"], label);
    return;
  }
  if (provider.kind === "scripted") {
    assertExactKeys(provider, ["kind", "events"], label);
    let eventEntries = provider.events;
    if (isPlainRecord(provider.events) && Object.hasOwn(provider.events, "$eventsRepeat")) {
      const repeat = provider.events.$eventsRepeat;
      if (
        !isPlainRecord(repeat) ||
        !Array.isArray(repeat.events) ||
        repeat.events.length === 0 ||
        !Number.isSafeInteger(repeat.count) ||
        repeat.count < 1 ||
        repeat.count > 4096 ||
        repeat.events.length * repeat.count > 4096
      ) {
        throw new Error(`${label}.events $eventsRepeat marker is invalid`);
      }
      eventEntries = repeat.events;
    }
    if (!Array.isArray(eventEntries) || eventEntries.length === 0 || eventEntries.length > 4096) {
      throw new Error(`${label}.events must contain 1-4096 raw events`);
    }
    for (const [index, event] of eventEntries.entries()) {
      if (!isPlainRecord(event)) continue;
      if (event.type === "provider_error") {
        assertExactKeys(event, ["type", "message"], `${label}.events[${index}]`);
        if (typeof event.message !== "string" || event.message.length === 0) {
          throw new Error(`${label}.events[${index}].message must be a non-empty string`);
        }
      } else if (event.type === "tool_call" && Object.hasOwn(event, "inputJson")) {
        if (!validMessageString(event.inputJson)) {
          throw new Error(`${label}.events[${index}].inputJson must be a string or repeat marker`);
        }
      }
    }
    return;
  }
  throw new Error(`${label}.kind must be fake or scripted`);
}

/** Validate the strict tool-loop scenario input shape. */
function validateToolLoopInput(input, label) {
  assertExactKeys(input, ["cases"], `${label}.input`);
  if (!Array.isArray(input.cases) || input.cases.length === 0 || input.cases.length > 64) {
    throw new Error(`${label}.input.cases must contain 1-64 entries`);
  }
  for (const [index, entry] of input.cases.entries()) {
    const caseLabel = `${label}.input.cases[${index}]`;
    assertPlainObject(entry, caseLabel);
    const allowed = new Set([
      "prompt",
      "maxToolRounds",
      "tools",
      "rules",
      "visibleTools",
      "provider",
      "cancelAfterCompletedToolCalls",
    ]);
    for (const key of Object.keys(entry)) {
      if (!allowed.has(key)) {
        throw new Error(`${caseLabel} has an unknown field ${JSON.stringify(key)}`);
      }
    }
    if (!validMessageString(entry.prompt)) {
      throw new Error(`${caseLabel}.prompt must be a string or repeat marker`);
    }
    if (Object.hasOwn(entry, "maxToolRounds")) {
      const value = entry.maxToolRounds;
      const validNumber = typeof value === "number" && Number.isFinite(value);
      const validDefault = value === null || value === "non-finite";
      if (!validNumber && !validDefault) {
        throw new Error(
          `${caseLabel}.maxToolRounds must be a finite number, null, or "non-finite"`,
        );
      }
    }
    if (!Object.hasOwn(entry, "tools")) {
      throw new Error(`${caseLabel} requires the tools field`);
    }
    validateBoundedToolNameArray(entry.tools, 32, `${caseLabel}.tools`);
    for (const name of entry.tools) {
      if (!TOOL_LOOP_TOOL_NAMES.has(name)) {
        throw new Error(`${caseLabel}.tools contains unsupported tool ${JSON.stringify(name)}`);
      }
    }
    if (Object.hasOwn(entry, "rules")) {
      if (!Array.isArray(entry.rules) || entry.rules.length > 32) {
        throw new Error(`${caseLabel}.rules must be a bounded array`);
      }
      for (const [ruleIndex, rule] of entry.rules.entries()) {
        assertExactKeys(rule, ["capability", "decision"], `${caseLabel}.rules[${ruleIndex}]`);
        if (
          typeof rule.capability !== "string" ||
          rule.capability.length === 0 ||
          byteLength(rule.capability) > 64 ||
          !TOOL_LOOP_CAPABILITY.test(rule.capability)
        ) {
          throw new Error(`${caseLabel}.rules[${ruleIndex}].capability is invalid`);
        }
        if (!["allow", "ask", "deny"].includes(rule.decision)) {
          throw new Error(`${caseLabel}.rules[${ruleIndex}].decision is invalid`);
        }
      }
    }
    if (Object.hasOwn(entry, "visibleTools")) {
      validateBoundedToolNameArray(entry.visibleTools, 32, `${caseLabel}.visibleTools`);
    }
    if (Object.hasOwn(entry, "cancelAfterCompletedToolCalls")) {
      if (
        !Number.isSafeInteger(entry.cancelAfterCompletedToolCalls) ||
        entry.cancelAfterCompletedToolCalls < 0 ||
        entry.cancelAfterCompletedToolCalls > 128
      ) {
        throw new Error(
          `${caseLabel}.cancelAfterCompletedToolCalls must be an integer from 0 to 128`,
        );
      }
    }
    if (!Object.hasOwn(entry, "provider")) {
      throw new Error(`${caseLabel} requires the provider field`);
    }
    validateToolLoopProvider(entry.provider, `${caseLabel}.provider`);
  }
}

/** Validate the bounded R7.4 user-configuration scenario matrix. */
function validateUserConfigInput(input, label) {
  assertExactKeys(input, ["cases"], `${label}.input`);
  if (!Array.isArray(input.cases) || input.cases.length === 0 || input.cases.length > 64) {
    throw new Error(`${label}.input.cases must contain 1-64 entries`);
  }
  const modes = new Set([
    "full",
    "unknown-top",
    "unknown-nested",
    "invalid-profile",
    "invalid-backend",
    "invalid-edition",
    "installations-bound",
    "references-bound",
    "invalid-godot-path",
    "invalid-provider",
    "invalid-json",
    "exact-boundary",
    "over-boundary",
    "directory",
    "symlink",
    "missing",
    "invalid-reference-path",
    "invalid-repository",
  ]);
  for (const [index, entry] of input.cases.entries()) {
    const caseLabel = `${label}.input.cases[${index}]`;
    assertExactKeys(entry, ["name", "mode"], caseLabel);
    assertBoundedString(entry.name, 64, `${caseLabel}.name`);
    if (typeof entry.mode !== "string" || !modes.has(entry.mode)) {
      throw new Error(`${caseLabel}.mode is unsupported`);
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
    "workspace-apply",
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
  if (scenario.subject === "provider-turn") {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(`${label} provider-turn inputs must use platforms ["*"] and an empty env`);
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.providerInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.providerInputBytes} UTF-8 bytes`);
    }
    validateProviderTurnInput(scenario.input, label);
    return;
  }
  if (scenario.subject === "context-projection") {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} context-projection inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (
      byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.contextProjectionInputBytes
    ) {
      throw new Error(
        `${label}.input exceeds ${CONTRACT_LIMITS.contextProjectionInputBytes} UTF-8 bytes`,
      );
    }
    return;
  }
  if (scenario.subject === "tool-loop") {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(`${label} tool-loop inputs must use platforms ["*"] and an empty env`);
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.toolLoopInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.toolLoopInputBytes} UTF-8 bytes`);
    }
    validateToolLoopInput(scenario.input, label);
    return;
  }
  if (scenario.subject === "user-config") {
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.userConfigInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.userConfigInputBytes} UTF-8 bytes`);
    }
    validateUserConfigInput(scenario.input, label);
    const posixSymlinkOnly =
      platforms.size === 1 &&
      platforms.has("posix") &&
      scenario.input.cases.every((entry) => entry.mode === "symlink");
    if (platforms.size !== 1 || (!platforms.has("*") && !posixSymlinkOnly) || envKeys.size !== 0) {
      throw new Error(
        `${label} user-config inputs must use platforms ["*"] or a POSIX-only symlink case and an empty env`,
      );
    }
    return;
  }
  const R13_AUTHORITY_SUBJECTS = new Set([
    "security-permissions",
    "command-catalog",
    "capability-doctor",
  ]);
  if (R13_AUTHORITY_SUBJECTS.has(scenario.subject)) {
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.r13AuthorityInputBytes) {
      throw new Error(
        `${label}.input exceeds ${CONTRACT_LIMITS.r13AuthorityInputBytes} UTF-8 bytes`,
      );
    }
    if (!Array.isArray(scenario.input.cases) || scenario.input.cases.length === 0) {
      throw new Error(`${label} ${scenario.subject} input must contain a non-empty cases array`);
    }
    if (scenario.subject === "capability-doctor") {
      const runtime = scenario.input.runtime;
      if (
        !isPlainRecord(runtime) ||
        typeof runtime.version !== "string" ||
        !Number.isInteger(runtime.nodeMajor) ||
        typeof runtime.platform !== "string"
      ) {
        throw new Error(
          `${label} capability-doctor input.runtime must be an injected identity object`,
        );
      }
    }
    for (const entry of scenario.input.cases) {
      if (!isPlainRecord(entry) || typeof entry.name !== "string" || entry.name.length === 0) {
        throw new Error(`${label} ${scenario.subject} cases must carry a non-empty name`);
      }
    }
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    return;
  }
  const R13_GUIDANCE_SUBJECTS = new Set(["instructions-resolution", "knowledge-revisions"]);
  if (R13_GUIDANCE_SUBJECTS.has(scenario.subject)) {
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.r13GuidanceInputBytes) {
      throw new Error(
        `${label}.input exceeds ${CONTRACT_LIMITS.r13GuidanceInputBytes} UTF-8 bytes`,
      );
    }
    const cases = scenario.input.cases;
    if (!Array.isArray(cases) || cases.length === 0) {
      throw new Error(`${label} ${scenario.subject} input must contain a non-empty cases array`);
    }
    for (const entry of cases) {
      if (!isPlainRecord(entry) || typeof entry.name !== "string" || entry.name.length === 0) {
        throw new Error(`${label} ${scenario.subject} cases must carry a non-empty name`);
      }
    }
    if (scenario.subject === "knowledge-revisions") {
      const runtime = scenario.input;
      if (
        !Number.isSafeInteger(runtime.nowMs) ||
        !Array.isArray(runtime.secrets) ||
        runtime.secrets.some((secret) => typeof secret !== "string") ||
        !Array.isArray(runtime.knownFiles) ||
        runtime.knownFiles.some(
          (entry) =>
            !Array.isArray(entry) ||
            entry.length !== 2 ||
            typeof entry[0] !== "string" ||
            typeof entry[1] !== "string",
        ) ||
        !Array.isArray(runtime.knownResearchEvidence)
      ) {
        throw new Error(
          `${label} knowledge-revisions input must inject the clock, secrets, known files, and research evidence`,
        );
      }
    }
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    return;
  }
  const R13_EXTERNAL_KNOWLEDGE_SUBJECTS = new Set(["reference-identity", "research-policy"]);
  if (R13_EXTERNAL_KNOWLEDGE_SUBJECTS.has(scenario.subject)) {
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (
      byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.r13ExternalKnowledgeInputBytes
    ) {
      throw new Error(
        `${label}.input exceeds ${CONTRACT_LIMITS.r13ExternalKnowledgeInputBytes} UTF-8 bytes`,
      );
    }
    if (!Number.isSafeInteger(scenario.input.nowMs) || scenario.input.nowMs < 0) {
      throw new Error(`${label} ${scenario.subject} input must inject a non-negative nowMs clock`);
    }
    const cases = scenario.input.cases;
    if (!Array.isArray(cases) || cases.length === 0 || cases.length > 16) {
      throw new Error(
        `${label} ${scenario.subject} input must contain a bounded non-empty cases array`,
      );
    }
    for (const entry of cases) {
      if (!isPlainRecord(entry) || typeof entry.name !== "string" || entry.name.length === 0) {
        throw new Error(`${label} ${scenario.subject} cases must carry a non-empty name`);
      }
    }
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    return;
  }
  const R13_PLANNING_BRIEFING_SUBJECTS = new Set(["planning-runtime", "executor-brief"]);
  if (R13_PLANNING_BRIEFING_SUBJECTS.has(scenario.subject)) {
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (
      byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.r13PlanningBriefingInputBytes
    ) {
      throw new Error(
        `${label}.input exceeds ${CONTRACT_LIMITS.r13PlanningBriefingInputBytes} UTF-8 bytes`,
      );
    }
    if (!Number.isSafeInteger(scenario.input.nowMs) || scenario.input.nowMs < 0) {
      throw new Error(`${label} ${scenario.subject} input must inject a non-negative nowMs clock`);
    }
    const cases = scenario.input.cases;
    if (!Array.isArray(cases) || cases.length === 0 || cases.length > 16) {
      throw new Error(
        `${label} ${scenario.subject} input must contain a bounded non-empty cases array`,
      );
    }
    for (const entry of cases) {
      if (!isPlainRecord(entry) || typeof entry.name !== "string" || entry.name.length === 0) {
        throw new Error(`${label} ${scenario.subject} cases must carry a non-empty name`);
      }
    }
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    return;
  }
  if (scenario.subject === "cli-session") {
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (
      byteLength(canonicalizeJson(scenario.input)) >
      CONTRACT_LIMITS.cliSessionInputBytes
    ) {
      throw new Error(
        `${label}.input exceeds ${CONTRACT_LIMITS.cliSessionInputBytes} UTF-8 bytes`,
      );
    }
    const cases = scenario.input.cases;
    if (!Array.isArray(cases) || cases.length === 0 || cases.length > 16) {
      throw new Error(
        `${label} cli-session input must contain a bounded non-empty cases array`,
      );
    }
    for (const entry of cases) {
      if (
        !isPlainRecord(entry) ||
        typeof entry.name !== "string" ||
        entry.name.length === 0
      ) {
        throw new Error(
          `${label} cli-session cases must carry a non-empty name`,
        );
      }
    }
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} cli-session inputs must use platforms ["*"] and an empty env`,
      );
    }
    return;
  }
  const GODOT_SUBJECTS = new Set([
    "godot-scene-resolve",
    "godot-discovery",
    "godot-knowledge",
    "godot-diagnostics",
    "godot-lsp",
    "godot-review-context",
    "godot-mutation-prepare",
    "godot-develop-plan",
  ]);
  if (GODOT_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > CONTRACT_LIMITS.godotInputBytes) {
      throw new Error(`${label}.input exceeds ${CONTRACT_LIMITS.godotInputBytes} UTF-8 bytes`);
    }
    return;
  }
  const R10A_SUBJECTS = new Set([
    "content-identity-artifact-digest",
    "content-identity-contract-digest",
    "content-identity-manifests",
    "content-identity-delta",
    "determinism-replay",
  ]);
  if (R10A_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > 64 * 1024) {
      throw new Error(`${label}.input exceeds ${64 * 1024} UTF-8 bytes`);
    }
    return;
  }
  const R10B_SUBJECTS = new Set(["icm.phase-contract", "icm.dependency-manifests"]);
  if (R10B_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > 64 * 1024) {
      throw new Error(`${label}.input exceeds ${64 * 1024} UTF-8 bytes`);
    }
    return;
  }
  const R10C_SUBJECTS = new Set([
    "runtime-readiness.identity",
    "runtime-readiness.budgets",
    "runtime-readiness.lifecycle",
    "runtime-readiness.doctor",
  ]);
  if (R10C_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > 64 * 1024) {
      throw new Error(`${label}.input exceeds ${64 * 1024} UTF-8 bytes`);
    }
    return;
  }
  const R11_SUBJECTS = new Set(["recovery-taxonomy"]);
  if (R11_SUBJECTS.has(scenario.subject)) {
    if (platforms.size !== 1 || !platforms.has("*") || envKeys.size !== 0) {
      throw new Error(
        `${label} ${scenario.subject} inputs must use platforms ["*"] and an empty env`,
      );
    }
    if (!Object.hasOwn(scenario, "input") || !isPlainRecord(scenario.input)) {
      throw new Error(`${label}.input must be a plain object`);
    }
    if (byteLength(canonicalizeJson(scenario.input)) > 64 * 1024) {
      throw new Error(`${label}.input exceeds ${64 * 1024} UTF-8 bytes`);
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
    "workspace-apply",
    "checkpoint",
    "git-inspection",
    "language-diagnostics",
    "language-structure",
    "language-definition",
    "domain-lifecycle",
    "domain-capability",
    "provider-turn",
    "tool-loop",
    "context-projection",
    "user-config",
    "security-permissions",
    "command-catalog",
    "capability-doctor",
    "instructions-resolution",
    "knowledge-revisions",
    "reference-identity",
    "research-policy",
    "planning-runtime",
    "executor-brief",
    "godot-scene-resolve",
    "godot-discovery",
    "godot-knowledge",
    "godot-diagnostics",
    "godot-lsp",
    "godot-review-context",
    "godot-mutation-prepare",
    "godot-develop-plan",
    "content-identity-artifact-digest",
    "content-identity-contract-digest",
    "content-identity-manifests",
    "content-identity-delta",
    "determinism-replay",
    "icm.phase-contract",
    "icm.dependency-manifests",
    "runtime-readiness.identity",
    "runtime-readiness.budgets",
    "runtime-readiness.lifecycle",
    "runtime-readiness.doctor",
    "recovery-taxonomy",
    "cli-session",
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
