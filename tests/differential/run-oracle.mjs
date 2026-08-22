/**
 * Differential harness — TypeScript oracle runner (ADR 0033).
 *
 * Executes each applicable scenario from the corpus against the Siralos
 * TypeScript reference implementation and emits canonical outcome
 * records. Deterministic: two runs with the same corpus and repository
 * produce byte-identical output.
 *
 * Usage:
 *   node tests/differential/run-oracle.mjs --corpus <dir> --root <repo> --out <file>
 *
 * Exit codes: 0 = success, 2 = harness error.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { sha256HexBytes } from "./shared/canonical.mjs";
import {
  CONTRACT_LIMITS,
  CorpusIntegrityError,
  loadValidatedCorpus,
  readBoundedUtf8File,
  validateProbeEnvironment,
} from "./shared/contract.mjs";
import {
  SCENARIO_OUTCOME,
  canonicalRecordDocument,
  harnessDiagnostic,
} from "./shared/protocol.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const PROBE = join(HERE, "probes", "state-dir-oracle.mjs");
const TASK_PROBE = join(HERE, "probes", "task-oracle.mjs");
const TS_REMAP_LOADER = join(HERE, "shared", "ts-remap-loader.mjs");
const PROBE_TIMEOUT_MS = 10_000;
const TASK_PROBE_TIMEOUT_MS = 15_000;
const WORKSPACE_PROBE_TIMEOUT_MS = 30_000;
const WORKSPACE_PROBE = join(HERE, "probes", "workspace-oracle.mjs");
const REVISION_PROBE = join(HERE, "probes", "revision-oracle.mjs");
const CHECKPOINT_PROBE = join(HERE, "probes", "checkpoint-oracle.mjs");
const LANGUAGE_DIAGNOSTICS_PROBE = join(HERE, "probes", "language-diagnostics-oracle.mjs");
const LANGUAGE_STRUCTURE_PROBE = join(HERE, "probes", "language-structure-oracle.mjs");
const LANGUAGE_DEFINITION_PROBE = join(HERE, "probes", "language-definition-oracle.mjs");
const DOMAIN_LIFECYCLE_PROBE = join(HERE, "probes", "domain-lifecycle-oracle.mjs");
const DOMAIN_CAPABILITY_PROBE = join(HERE, "probes", "domain-capability-oracle.mjs");
const PROVIDER_TURN_PROBE = join(HERE, "probes", "provider-turn-oracle.mjs");
const TOOL_LOOP_PROBE = join(HERE, "probes", "tool-loop-oracle.mjs");
const CONTEXT_PROJECTION_PROBE = join(HERE, "probes", "context-projection-oracle.mjs");
const USER_CONFIG_PROBE = join(HERE, "probes", "user-config-oracle.mjs");
const GODOT_SCENE_RESOLVE_PROBE = join(HERE, "probes", "godot-scene-resolve-oracle.mjs");
const GODOT_DISCOVERY_PROBE = join(HERE, "probes", "godot-discovery-oracle.mjs");
const GODOT_KNOWLEDGE_PROBE = join(HERE, "probes", "godot-knowledge-oracle.mjs");
const GODOT_DIAGNOSTICS_PROBE = join(HERE, "probes", "godot-diagnostics-oracle.mjs");
const GODOT_LSP_PROBE = join(HERE, "probes", "godot-lsp-oracle.mjs");
const GODOT_PROBES = new Map([
  ["godot-scene-resolve", GODOT_SCENE_RESOLVE_PROBE],
  ["godot-discovery", GODOT_DISCOVERY_PROBE],
  ["godot-knowledge", GODOT_KNOWLEDGE_PROBE],
  ["godot-diagnostics", GODOT_DIAGNOSTICS_PROBE],
  ["godot-lsp", GODOT_LSP_PROBE],
]);

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

function harnessError(category, code, message) {
  console.error(`oracle runner: ${message}`);
  console.error(harnessDiagnostic(category, code, message));
  process.exit(2);
}

/** The platform name used by scenario `platforms` fields. */
export function platformName(platform = process.platform) {
  return platform === "win32" ? "windows" : "posix";
}

/** Read and structurally validate the corpus manifest and scenarios. */
export function loadCorpus(corpusDir, platform = platformName()) {
  return loadValidatedCorpus(corpusDir, platform);
}

/** Run the state-dir probe under the scenario's scrubbed environment. */
export function runStateDirProbe(env, { probe = PROBE, timeoutMs = PROBE_TIMEOUT_MS } = {}) {
  validateProbeEnvironment(env);
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > PROBE_TIMEOUT_MS) {
    throw new Error("state-dir probe timeout is outside the harness bound");
  }
  const result = spawnSync(process.execPath, [probe], {
    env,
    encoding: "buffer",
    timeout: timeoutMs,
    maxBuffer: CONTRACT_LIMITS.probeOutputBytes,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error?.code === "ETIMEDOUT") {
    throw new Error("state-dir probe timed out");
  }
  if (result.error !== undefined) {
    throw new Error(`state-dir probe could not execute: ${result.error.code ?? "unknown error"}`);
  }
  if (result.signal !== null) {
    throw new Error("state-dir probe terminated by a signal");
  }
  if (result.status !== 0) {
    throw new Error(`state-dir probe exited unsuccessfully (${String(result.status)})`);
  }
  const output = result.stdout;
  if (!Buffer.isBuffer(output) || output.length === 0) {
    throw new Error("state-dir probe emitted no outcome");
  }
  if (output.length > CONTRACT_LIMITS.probeOutputBytes) {
    throw new Error("state-dir probe output exceeded the harness bound");
  }
  if (output.toString("utf8") === "ERR") {
    return {
      outcome: SCENARIO_OUTCOME.PRODUCT_FAILURE,
      error: { category: "NO_HOME_DIRECTORY" },
    };
  }
  return {
    outcome: SCENARIO_OUTCOME.COMPLETED,
    result: { stateDirSha256: sha256HexBytes(output) },
  };
}

/**
 * Run the task-contract probe against the real TypeScript reference task
 * runtime. The probe receives the scenario input JSON on stdin (bounded)
 * and emits the canonical R3 observation object on stdout. Node's native
 * type stripping executes the reference source directly; the remap loader
 * resolves the reference's .js import specifiers to their .ts files.
 */
export function runTaskProbe(
  input,
  { probe = TASK_PROBE, loader = TS_REMAP_LOADER, timeoutMs = TASK_PROBE_TIMEOUT_MS } = {},
) {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > TASK_PROBE_TIMEOUT_MS) {
    throw new Error("task probe timeout is outside the harness bound");
  }
  // No encoding option: spawnSync returns raw Buffers by default, and
  // the probe output is validated as exact bytes below. The loader must
  // be a file:// URL: on Windows, --import rejects drive-letter paths.
  const result = spawnSync(process.execPath, ["--import", pathToFileURL(loader).href, probe], {
    input: JSON.stringify(input),
    timeout: timeoutMs,
    maxBuffer: CONTRACT_LIMITS.probeOutputBytes,
    stdio: ["pipe", "pipe", "pipe"],
  });
  if (result.error?.code === "ETIMEDOUT") {
    throw new Error("task probe timed out");
  }
  if (result.error !== undefined) {
    throw new Error(`task probe could not execute: ${result.error.code ?? "unknown error"}`);
  }
  if (result.signal !== null) {
    throw new Error("task probe terminated by a signal");
  }
  if (result.status !== 0) {
    throw new Error(`task probe exited unsuccessfully (${String(result.status)})`);
  }
  const output = result.stdout;
  if (!Buffer.isBuffer(output) || output.length === 0) {
    throw new Error("task probe emitted no outcome");
  }
  if (output.length > CONTRACT_LIMITS.probeOutputBytes) {
    throw new Error("task probe output exceeded the harness bound");
  }
  let parsed;
  try {
    parsed = JSON.parse(output.toString("utf8"));
  } catch {
    throw new Error("task probe emitted malformed JSON");
  }
  return parsed;
}

/**
 * Run one Stage 3R R4 probe against the real TypeScript reference with
 * the scenario input on stdin, mirroring the task-probe lifecycle.
 */
function runWorkspaceProbe(probe, subject, input, timeoutMs = WORKSPACE_PROBE_TIMEOUT_MS) {
  const result = spawnSync(
    process.execPath,
    ["--import", pathToFileURL(TS_REMAP_LOADER).href, probe],
    {
      input: JSON.stringify({ subject, ...input }),
      timeout: timeoutMs,
      maxBuffer: CONTRACT_LIMITS.probeOutputBytes,
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
  if (result.error?.code === "ETIMEDOUT") {
    throw new Error(`${subject} probe timed out`);
  }
  if (result.error !== undefined) {
    throw new Error(`${subject} probe could not execute: ${result.error.code ?? "unknown error"}`);
  }
  if (result.signal !== null) {
    throw new Error(`${subject} probe terminated by a signal`);
  }
  if (result.status !== 0) {
    throw new Error(`${subject} probe exited unsuccessfully (${String(result.status)})`);
  }
  const output = result.stdout;
  if (!Buffer.isBuffer(output) || output.length === 0) {
    throw new Error(`${subject} probe emitted no outcome`);
  }
  if (output.length > CONTRACT_LIMITS.probeOutputBytes) {
    throw new Error(`${subject} probe output exceeded the harness bound`);
  }
  let parsed;
  try {
    parsed = JSON.parse(output.toString("utf8"));
  } catch {
    throw new Error(`${subject} probe emitted malformed JSON`);
  }
  return parsed;
}

const WORKSPACE_FILE_SUBJECTS = new Set([
  "workspace-read",
  "workspace-list",
  "workspace-search",
  "workspace-prepare",
  "git-inspection",
]);
/** The oracle record for a single scenario. */
export function runScenario(scenario, root) {
  if (scenario.subject === "state-dir") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      ...runStateDirProbe(scenario.env),
    };
  }
  if (scenario.subject === "task-contract") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runTaskProbe(scenario.input),
    };
  }
  if (WORKSPACE_FILE_SUBJECTS.has(scenario.subject)) {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(WORKSPACE_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "workspace-revision") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(REVISION_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "checkpoint") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(CHECKPOINT_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "language-diagnostics") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(LANGUAGE_DIAGNOSTICS_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "language-structure") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(LANGUAGE_STRUCTURE_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "language-definition") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(LANGUAGE_DEFINITION_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "domain-lifecycle") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(DOMAIN_LIFECYCLE_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "domain-capability") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(DOMAIN_CAPABILITY_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "provider-turn") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(PROVIDER_TURN_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "tool-loop") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(TOOL_LOOP_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "context-projection") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(CONTEXT_PROJECTION_PROBE, scenario.subject, scenario.input),
    };
  }
  if (scenario.subject === "user-config") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(USER_CONFIG_PROBE, scenario.subject, scenario.input),
    };
  }
  if (GODOT_PROBES.has(scenario.subject)) {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: runWorkspaceProbe(
        GODOT_PROBES.get(scenario.subject),
        scenario.subject,
        scenario.input,
      ),
    };
  }
  if (scenario.subject === "version-identity") {
    try {
      const packageJson = JSON.parse(
        readBoundedUtf8File(
          join(root, "package.json"),
          CONTRACT_LIMITS.manifestBytes,
          "package.json",
        ),
      );
      if (typeof packageJson.version !== "string") {
        return {
          scenarioId: scenario.id,
          subject: scenario.subject,
          outcome: SCENARIO_OUTCOME.PRODUCT_FAILURE,
          error: { category: "VERSION_UNAVAILABLE" },
        };
      }
      return {
        scenarioId: scenario.id,
        subject: scenario.subject,
        outcome: SCENARIO_OUTCOME.COMPLETED,
        result: { version: packageJson.version },
      };
    } catch {
      return {
        scenarioId: scenario.id,
        subject: scenario.subject,
        outcome: SCENARIO_OUTCOME.PRODUCT_FAILURE,
        error: { category: "VERSION_UNAVAILABLE" },
      };
    }
  }
  throw new Error(`unknown subject ${scenario.subject}`);
}

/** Emit canonical records for all applicable scenarios. */
export function runOracle(corpusDir, root, platform = platformName(), scenarioId = undefined) {
  const { scenarios } = loadCorpus(corpusDir, platform);
  const records = [];
  for (const scenario of scenarios) {
    if (scenarioId !== undefined && scenario.id !== scenarioId) {
      continue;
    }
    if (!scenario.applicable) {
      records.push({
        scenarioId: scenario.id,
        subject: scenario.subject,
        outcome: SCENARIO_OUTCOME.UNSUPPORTED,
        error: { category: "PLATFORM_NOT_APPLICABLE" },
      });
    } else {
      records.push(runScenario(scenario, root));
    }
  }
  if (scenarioId !== undefined && records.length === 0) {
    throw new CorpusIntegrityError(
      "UNKNOWN_SCENARIO",
      "requested scenario is not present in the digest-bound corpus",
    );
  }
  return canonicalRecordDocument(records);
}

function main() {
  const corpus = optionValue(process.argv, "--corpus");
  const root = optionValue(process.argv, "--root");
  const out = optionValue(process.argv, "--out");
  const scenario = optionValue(process.argv, "--scenario");
  if (corpus === undefined || root === undefined || out === undefined) {
    harnessError(
      "HARNESS_INVOCATION_FAILURE",
      "INVALID_ARGUMENTS",
      "usage: run-oracle.mjs --corpus <dir> --root <repo> --out <file> [--scenario <id>]",
    );
  }
  let output;
  try {
    output = runOracle(corpus, root, platformName(), scenario);
  } catch (error) {
    const message = String(error instanceof Error ? error.message : error);
    harnessError(
      error instanceof CorpusIntegrityError
        ? "CORPUS_INTEGRITY_FAILURE"
        : "HARNESS_INTERNAL_FAILURE",
      error instanceof CorpusIntegrityError ? error.code : "RUNNER_EXECUTION_FAILURE",
      message,
    );
  }
  try {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, output, "utf8");
  } catch (error) {
    console.error(`oracle runner: cannot write ${out}`);
    harnessError(
      "HARNESS_INTERNAL_FAILURE",
      "OUTPUT_WRITE_FAILURE",
      `oracle runner could not write its protocol document: ${String(error instanceof Error ? error.message : error)}`,
    );
  }
  process.stdout.write(`oracle: wrote ${out}\n`);
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
