/** Authoritative end-to-end R2 acceptance command (ADR 0033). Pinned mode post-TS-archive (decision 40). */
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { canonicalizeJson, sha256Hex } from "./shared/canonical.mjs";
import { CorpusIntegrityError, loadValidatedCorpus } from "./shared/contract.mjs";
import { canonicalRecordDocument, parseCanonicalRecordDocument } from "./shared/protocol.mjs";
import { RUNNER_PROCESS_LIMITS, superviseRunner } from "./shared/runner-process.mjs";
import { collectSourceIdentity, runCompare } from "./compare.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

/** Platform name used by scenario `platforms` fields. */
export function platformName(platform = process.platform) {
  return platform === "win32" ? "windows" : "posix";
}

/** Read and structurally validate the corpus manifest and scenarios. */
export function loadCorpus(corpusDir, platform = platformName()) {
  return loadValidatedCorpus(corpusDir, platform);
}

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

function loadPinnedOracle(pinnedPath, outDir) {
  try {
    const text = readFileSync(resolve(pinnedPath), "utf8");
    return parseCanonicalRecordDocument(text, "reference");
  } catch (error) {
    const failure = {
      implementation: "reference",
      scenarioId: "<pinned-oracle>",
      outcome: "HARNESS_ERROR",
      category: "PINNED_ORACLE_FAILURE",
      code: error instanceof CorpusIntegrityError ? error.code : "PINNED_READ_FAILURE",
      message: String(error instanceof Error ? error.message : error),
    };
    writeFailure(outDir, { schemaVersion: 1, parityHeld: false, runnerFailure: failure });
    const e = new Error(`pinned oracle could not be loaded: ${failure.message}`);
    e.exitCode = 2;
    throw e;
  }
}

/** Load the digest-bound post-freeze expectation records (decision 40 C7). */
function loadExpectations(path, outDir) {
  try {
    return parseCanonicalRecordDocument(readFileSync(resolve(path), "utf8"), "expectation");
  } catch (error) {
    const failure = {
      implementation: "reference",
      scenarioId: "<post-freeze-expectations>",
      outcome: "HARNESS_ERROR",
      category: "PINNED_ORACLE_FAILURE",
      code: error instanceof CorpusIntegrityError ? error.code : "EXPECTATIONS_READ_FAILURE",
      message: `post-freeze expectations could not be loaded: ${
        error instanceof Error ? error.message : error
      }`,
    };
    writeFailure(outDir, { schemaVersion: 1, parityHeld: false, runnerFailure: failure });
    const e = new Error(failure.message);
    e.exitCode = 2;
    throw e;
  }
}

/** Execute candidate runner; oracle is either live (historical replay) or pinned. */
export async function runDifferential({ corpusDir, root, outDir, pinnedOracle, expectationsPath }) {
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

    let oracleRecords;
    let expectationScenarioIds = null;
    let expectationRecordsSha256 = null;
    if (pinnedOracle !== undefined) {
      oracleRecords = loadPinnedOracle(pinnedOracle, absoluteOut);
      const pinnedIds = new Set(oracleRecords.map((r) => r.scenarioId));
      let expectationRecords = [];
      if (expectationsPath !== undefined) {
        expectationRecords = loadExpectations(expectationsPath, absoluteOut);
        expectationRecordsSha256 = sha256Hex(readFileSync(resolve(expectationsPath)));
      }
      const expectationIds = new Set(expectationRecords.map((r) => r.scenarioId));
      const overlapping = [...pinnedIds].filter((id) => expectationIds.has(id));
      const uncovered = scenarios.filter(
        (scenario) => !pinnedIds.has(scenario.id) && !expectationIds.has(scenario.id),
      );
      if (overlapping.length > 0 || uncovered.length > 0) {
        const failure = {
          implementation: "reference",
          scenarioId: uncovered[0]?.id ?? overlapping[0] ?? "<coverage>",
          outcome: "HARNESS_ERROR",
          category: "PINNED_ORACLE_FAILURE",
          code: overlapping.length > 0 ? "EXPECTATIONS_OVERLAP" : "PINNED_MISMATCH",
          message:
            overlapping.length > 0
              ? `scenarios covered by both the pinned freeze-v32 oracle and the post-freeze expectations: ${overlapping.join(", ")}`
              : `pinned oracle does not contain scenario ${uncovered[0].id} (freeze v32 vs current v${manifest.corpusVersion}); post-freeze scenarios require explicit digest-bound expectation records (decision 40 C7, decision 41 C5)`,
        };
        writeFailure(absoluteOut, {
          schemaVersion: 1,
          parityHeld: false,
          runnerFailure: failure,
        });
        const e = new Error(failure.message);
        e.exitCode = 2;
        throw e;
      }
      // Reference records in exact corpus order: frozen oracle records plus
      // digest-bound post-freeze expectation records. The audit discloses
      // which scenarios rely on candidate-authored expectations.
      const recordsById = new Map(
        [...oracleRecords, ...expectationRecords].map((record) => [record.scenarioId, record]),
      );
      oracleRecords = scenarios.map((scenario) => {
        const record = recordsById.get(scenario.id);
        if (record === undefined) {
          const failure = {
            implementation: "reference",
            scenarioId: scenario.id,
            outcome: "HARNESS_ERROR",
            category: "PINNED_ORACLE_FAILURE",
            code: "PINNED_MISMATCH",
            message: `pinned reference set does not contain scenario ${scenario.id}`,
          };
          writeFailure(absoluteOut, {
            schemaVersion: 1,
            parityHeld: false,
            runnerFailure: failure,
          });
          const e = new Error(failure.message);
          e.exitCode = 2;
          throw e;
        }
        return record;
      });
      expectationScenarioIds = [...expectationIds].sort();
    } else {
      oracleRecords = [];
      for (const scenario of scenarios) {
        const oracleOut = join(scratch, `${scenario.id}.oracle.json`);
        const common = ["--corpus", absoluteCorpus, "--root", absoluteRoot, "--out"];
        const liveOracle = join(HERE, "run-oracle.mjs");
        if (!existsSync(liveOracle)) {
          const failure = {
            implementation: "reference",
            scenarioId: scenario.id,
            outcome: "HARNESS_ERROR",
            category: "LIVE_ORACLE_UNAVAILABLE",
            code: "LIVE_ORACLE_REMOVED",
            message:
              "live TypeScript oracle is not available in this tree (pinned mode required; use --pinned-oracle or checkout freeze worktree)",
          };
          writeFailure(absoluteOut, {
            schemaVersion: 1,
            parityHeld: false,
            runnerFailure: failure,
          });
          const e = new Error(failure.message);
          e.exitCode = 2;
          throw e;
        }
        const reference = await superviseRunner({
          implementation: "reference",
          scenarioId: scenario.id,
          command: process.execPath,
          args: [liveOracle, ...common, oracleOut, "--scenario", scenario.id],
          cwd: absoluteRoot,
        });
        assertCompleted(reference, absoluteOut);
        oracleRecords.push(readSingleRecord(oracleOut, "reference", scenario.id, absoluteOut));
      }
    }

    const candidateRecords = [];
    for (const scenario of scenarios) {
      const candidateOut = join(scratch, `${scenario.id}.candidate.json`);
      const common = ["--corpus", absoluteCorpus, "--root", absoluteRoot, "--out"];
      const candidate = await superviseRunner({
        implementation: "candidate",
        scenarioId: scenario.id,
        command: runnerExecutable(absoluteRoot),
        args: ["run", ...common, candidateOut, "--scenario", scenario.id],
        cwd: absoluteRoot,
      });
      assertCompleted(candidate, absoluteOut);
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
      expectationScenarioIds,
      expectationRecordsSha256,
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
  const pinnedOracle = optionValue(process.argv, "--pinned-oracle");
  const expectationsArg = optionValue(process.argv, "--expectations");
  if (corpusDir === undefined || root === undefined || outDir === undefined) {
    console.error(
      "usage: run-differential.mjs --corpus <dir> --root <repo> --out-dir <directory> [--pinned-oracle <file>] [--expectations <file>]",
    );
    process.exit(2);
  }
  let effectivePinned = pinnedOracle;
  if (effectivePinned === undefined) {
    const frozenDefault = resolve(
      root,
      "tests/differential/evidence/typescript-freeze-v32/oracle.json",
    );
    if (existsSync(frozenDefault)) {
      effectivePinned = frozenDefault;
    }
  }
  let effectiveExpectations = expectationsArg;
  if (effectiveExpectations === undefined) {
    const defaultExpectations = resolve(
      root,
      "tests/differential/evidence/post-freeze/expectations.json",
    );
    if (existsSync(defaultExpectations)) {
      effectiveExpectations = defaultExpectations;
    }
  }
  try {
    const audit = await runDifferential({
      corpusDir,
      root,
      outDir,
      pinnedOracle: effectivePinned,
      expectationsPath: effectiveExpectations,
    });
    console.log(
      `Differential audit: parity held (${audit.matchedRequiredScenarios}/${audit.requiredApplicableScenarios} applicable required scenarios; ${audit.skipped.length} explicit platform skips; ${audit.informationalDeviations.length} accepted informational deviations).`,
    );
    if (effectivePinned !== undefined) {
      console.log(`(pinned oracle: ${effectivePinned})`);
    }
  } catch (error) {
    console.error(`Differential audit failed: ${error instanceof Error ? error.message : error}`);
    process.exit(error?.exitCode === 1 ? 1 : 2);
  }
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
