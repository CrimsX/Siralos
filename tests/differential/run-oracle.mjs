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
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { canonicalizeJson, sha256HexBytes } from "./shared/canonical.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const PROBE = join(HERE, "probes", "state-dir-oracle.mjs");
const PROBE_TIMEOUT_MS = 10_000;

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

function harnessError(message) {
  console.error(`oracle runner: ${message}`);
  process.exit(2);
}

/** The platform name used by scenario `platforms` fields. */
export function platformName(platform = process.platform) {
  return platform === "win32" ? "windows" : "posix";
}

/** Read and structurally validate the corpus manifest and scenarios. */
export function loadCorpus(corpusDir, platform = platformName()) {
  const manifest = JSON.parse(readFileSync(join(corpusDir, "manifest.json"), "utf8"));
  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.scenarios)) {
    throw new Error("malformed corpus manifest");
  }
  const scenarios = [];
  for (const entry of manifest.scenarios) {
    if (typeof entry.file !== "string" || typeof entry.sha256 !== "string") {
      throw new Error(`malformed corpus manifest entry: ${JSON.stringify(entry)}`);
    }
    const scenario = JSON.parse(readFileSync(join(corpusDir, entry.file), "utf8"));
    if (
      typeof scenario.id !== "string" ||
      typeof scenario.subject !== "string" ||
      !Array.isArray(scenario.platforms) ||
      !["required", "informational"].includes(scenario.parity) ||
      typeof scenario.env !== "object" ||
      scenario.env === null
    ) {
      throw new Error(`malformed scenario ${entry.file}`);
    }
    const applicable = scenario.platforms.includes("*") || scenario.platforms.includes(platform);
    scenarios.push({ ...scenario, file: entry.file, applicable });
  }
  return { manifest, scenarios };
}

/** Run the state-dir probe under the scenario's scrubbed environment. */
function runStateDirProbe(env) {
  const result = spawnSync(process.execPath, [PROBE], {
    env,
    encoding: "buffer",
    timeout: PROBE_TIMEOUT_MS,
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error !== undefined || result.status === null) {
    throw new Error(`state-dir probe failed: ${String(result.error ?? "timeout")}`);
  }
  const output = result.stdout;
  if (output.toString("utf8") === "ERR") {
    return { kind: "error", stateDirSha256: null };
  }
  return { kind: "ok", stateDirSha256: sha256HexBytes(output) };
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
      const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
      if (typeof packageJson.version !== "string") {
        return { scenarioId: scenario.id, subject: scenario.subject, kind: "error", version: null };
      }
      return {
        scenarioId: scenario.id,
        subject: scenario.subject,
        kind: "ok",
        version: packageJson.version,
      };
    } catch {
      return { scenarioId: scenario.id, subject: scenario.subject, kind: "error", version: null };
    }
  }
  throw new Error(`unknown subject ${scenario.subject}`);
}

/** Emit canonical records for all applicable scenarios. */
export function runOracle(corpusDir, root, platform = platformName()) {
  const { scenarios } = loadCorpus(corpusDir, platform);
  const records = [];
  for (const scenario of scenarios) {
    if (!scenario.applicable) {
      continue;
    }
    records.push(runScenario(scenario, root));
  }
  return canonicalizeJson(records) + "\n";
}

function main() {
  const corpus = optionValue(process.argv, "--corpus");
  const root = optionValue(process.argv, "--root");
  const out = optionValue(process.argv, "--out");
  if (corpus === undefined || root === undefined || out === undefined) {
    harnessError("usage: run-oracle.mjs --corpus <dir> --root <repo> --out <file>");
  }
  let output;
  try {
    output = runOracle(corpus, root);
  } catch (error) {
    harnessError(String(error instanceof Error ? error.message : error));
  }
  try {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, output, "utf8");
  } catch (error) {
    harnessError(`cannot write ${out}: ${String(error instanceof Error ? error.message : error)}`);
  }
  process.stdout.write(`oracle: wrote ${out}\n`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
