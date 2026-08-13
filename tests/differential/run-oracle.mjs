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
const PROBE_TIMEOUT_MS = 10_000;

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

/** The oracle record for a single scenario. */
export function runScenario(scenario, root) {
  if (scenario.subject === "state-dir") {
    return {
      scenarioId: scenario.id,
      subject: scenario.subject,
      ...runStateDirProbe(scenario.env),
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

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
