/** Authoritative end-to-end R2 acceptance command (ADR 0033). */
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { canonicalizeJson } from "./shared/canonical.mjs";
import { CorpusIntegrityError } from "./shared/contract.mjs";
import { canonicalRecordDocument, parseCanonicalRecordDocument } from "./shared/protocol.mjs";
import { RUNNER_PROCESS_LIMITS, superviseRunner } from "./shared/runner-process.mjs";
import { collectSourceIdentity, runCompare } from "./compare.mjs";
import { loadCorpus, platformName } from "./run-oracle.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

function runnerExecutable(root) {
  return join(
    root,
    "target",
    "debug",
    process.platform === "win32" ? "siralos-harness.exe" : "siralos-harness",
  );
}

function failurePath(outDir) {
  return join(outDir, "failure.json");
}

function writeFailure(outDir, failure) {
  mkdirSync(outDir, { recursive: true });
  writeFileSync(failurePath(outDir), `${canonicalizeJson(failure)}\n`, "utf8");
}

function assertCompleted(result, outDir) {
  if (result.outcome === "COMPLETED") return;
  writeFailure(outDir, {
    schemaVersion: 1,
    parityHeld: false,
    runnerFailure: result,
  });
  const error = new Error(
    `${result.implementation} ${result.outcome} for ${result.scenarioId}: ${result.message}`,
  );
  error.exitCode = 2;
  throw error;
}

function readSingleRecord(path, implementation, scenarioId, outDir) {
  try {
    const records = parseCanonicalRecordDocument(readFileSync(path, "utf8"), implementation);
    if (records.length !== 1 || records[0].scenarioId !== scenarioId) {
      throw new Error(`${implementation} emitted an incomplete per-scenario protocol document`);
    }
    return records[0];
  } catch (error) {
    const failure = {
      implementation,
      scenarioId,
      outcome: "PROTOCOL_ERROR",
      category: "RUNNER_PROTOCOL_ERROR",
      code: "MALFORMED_RUNNER_PROTOCOL",
      message: String(error instanceof Error ? error.message : error),
    };
    writeFailure(outDir, { schemaVersion: 1, parityHeld: false, runnerFailure: failure });
    const protocolError = new Error(`${implementation} PROTOCOL_ERROR for ${scenarioId}`);
    protocolError.exitCode = 2;
    throw protocolError;
  }
}

/** Execute both runners under identical harness-level supervision. */
export async function runDifferential({ corpusDir, root, outDir }) {
  const absoluteRoot = resolve(root);
  const absoluteCorpus = resolve(corpusDir);
  const absoluteOut = resolve(outDir);
  mkdirSync(absoluteOut, { recursive: true });
  for (const name of ["oracle.json", "candidate.json", "audit.json", "failure.json"]) {
    rmSync(join(absoluteOut, name), { force: true });
  }
  let corpus;
  try {
    corpus = loadCorpus(absoluteCorpus, platformName());
  } catch (error) {
    const failure = {
      implementation: "harness",
      scenarioId: "<corpus>",
      outcome: "HARNESS_ERROR",
      category: "CORPUS_INTEGRITY_FAILURE",
      code: error instanceof CorpusIntegrityError ? error.code : "MALFORMED_CORPUS",
      message: String(error instanceof Error ? error.message : error),
    };
    writeFailure(absoluteOut, { schemaVersion: 1, parityHeld: false, runnerFailure: failure });
    const corpusError = new Error(`${failure.category}: ${failure.message}`);
    corpusError.exitCode = 2;
    throw corpusError;
  }
  const { manifest, scenarios, corpusDigest } = corpus;
  const scratch = mkdtempSync(join(tmpdir(), "siralos-r2-"));
  try {
    const build = await superviseRunner({
      implementation: "candidate-build",
      scenarioId: "<build>",
      command: "cargo",
      args: [
        "build",
        "--quiet",
        "--locked",
        "--features",
        "differential-harness",
        "--bin",
        "siralos-harness",
      ],
      cwd: absoluteRoot,
      timeoutMs: RUNNER_PROCESS_LIMITS.buildTimeoutMs,
    });
    if (build.outcome !== "COMPLETED") {
      build.outcome = "HARNESS_ERROR";
      build.category = "CANDIDATE_BUILD_FAILURE";
      build.code = build.code ?? "BUILD_FAILED";
      build.message = "candidate harness binary could not be built";
    }
    assertCompleted(build, absoluteOut);

    const oracleRecords = [];
    const candidateRecords = [];
    for (const scenario of scenarios) {
      const oracleOut = join(scratch, `${scenario.id}.oracle.json`);
      const candidateOut = join(scratch, `${scenario.id}.candidate.json`);
      const common = ["--corpus", absoluteCorpus, "--root", absoluteRoot, "--out"];
      const reference = await superviseRunner({
        implementation: "reference",
        scenarioId: scenario.id,
        command: process.execPath,
        args: [join(HERE, "run-oracle.mjs"), ...common, oracleOut, "--scenario", scenario.id],
        cwd: absoluteRoot,
      });
      assertCompleted(reference, absoluteOut);
      const candidate = await superviseRunner({
        implementation: "candidate",
        scenarioId: scenario.id,
        command: runnerExecutable(absoluteRoot),
        args: ["run", ...common, candidateOut, "--scenario", scenario.id],
        cwd: absoluteRoot,
      });
      assertCompleted(candidate, absoluteOut);
      oracleRecords.push(readSingleRecord(oracleOut, "reference", scenario.id, absoluteOut));
      candidateRecords.push(readSingleRecord(candidateOut, "candidate", scenario.id, absoluteOut));
    }

    const oraclePath = join(absoluteOut, "oracle.json");
    const candidatePath = join(absoluteOut, "candidate.json");
    const auditPath = join(absoluteOut, "audit.json");
    writeFileSync(oraclePath, canonicalRecordDocument(oracleRecords), "utf8");
    writeFileSync(candidatePath, canonicalRecordDocument(candidateRecords), "utf8");
    const sourceIdentity = collectSourceIdentity(absoluteRoot);
    const { audit } = runCompare({
      oracleRecords,
      candidateRecords,
      scenarios,
      platform: platformName(),
      corpusVersion: manifest.corpusVersion,
      corpusDigest,
      sourceIdentity,
    });
    writeFileSync(auditPath, `${canonicalizeJson(audit)}\n`, "utf8");
    if (!audit.parityHeld) {
      const error = new Error("required differential deviations remain");
      error.exitCode = 1;
      throw error;
    }
    return audit;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

async function main() {
  const corpusDir = optionValue(process.argv, "--corpus");
  const root = optionValue(process.argv, "--root");
  const outDir = optionValue(process.argv, "--out-dir");
  if (corpusDir === undefined || root === undefined || outDir === undefined) {
    console.error("usage: run-differential.mjs --corpus <dir> --root <repo> --out-dir <directory>");
    process.exit(2);
  }
  try {
    const audit = await runDifferential({ corpusDir, root, outDir });
    console.log(
      `Differential audit: parity held (${audit.matchedRequiredScenarios}/${audit.requiredApplicableScenarios} applicable required scenarios; ${audit.skipped.length} explicit platform skips; ${audit.informationalDeviations.length} accepted informational deviations).`,
    );
  } catch (error) {
    console.error(`Differential audit failed: ${error instanceof Error ? error.message : error}`);
    process.exit(error?.exitCode === 1 ? 1 : 2);
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
