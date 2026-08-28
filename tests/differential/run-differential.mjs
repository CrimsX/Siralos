/** Authoritative end-to-end R2 acceptance command (ADR 0033). Pinned mode post-TS-archive (decision 40). */
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { canonicalizeJson } from "./shared/canonical.mjs";
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
    // Pinned oracle is a canonical record document (239 records at freeze).
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

/** Execute candidate runner; oracle is either live or pinned. */
export async function runDifferential({ corpusDir, root, outDir, pinnedOracle }) {
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
    if (pinnedOracle !== undefined) {
      // Pinned mode: load oracle records from frozen evidence (post-TS-archive).
      oracleRecords = loadPinnedOracle(pinnedOracle, absoluteOut);
      // Validate that pinned set covers current corpus scenario ids (version may differ post-bump).
      const pinnedIds = new Set(oracleRecords.map((r) => r.scenarioId));
      for (const scenario of scenarios) {
        if (!pinnedIds.has(scenario.id)) {
          const failure = {
            implementation: "reference",
            scenarioId: scenario.id,
            outcome: "HARNESS_ERROR",
            category: "PINNED_ORACLE_FAILURE",
            code: "PINNED_MISMATCH",
            message: `pinned oracle does not contain scenario ${scenario.id} (freeze v32 vs current v${manifest.corpusVersion})`,
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
      }
    } else {
      oracleRecords = [];
      // Live oracle mode (historical replay only; requires TS tree at freeze SHA).
      for (const scenario of scenarios) {
        const oracleOut = join(scratch, `${scenario.id}.oracle.json`);
        const common = ["--corpus", absoluteCorpus, "--root", absoluteRoot, "--out"];
        // This path is only valid when the TS oracle tree is present (worktree at freeze SHA).
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
    if (pinnedOracle !== undefined) {
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
        candidateRecords.push(
          readSingleRecord(candidateOut, "candidate", scenario.id, absoluteOut),
        );
      }
    } else {
      // Live mode already collected oracle above; now collect candidate in same loop.
      // For live mode we already ran candidate per scenario inside the oracle loop would be double;
      // but we separated loops: pinned mode candidate loop, live mode already did both.
      // To avoid duplication, handle live candidate collection here only if not already done.
      // In live path above we pushed oracle only; candidate was not yet pushed.
      // So collect candidate now for live mode as well (second half of live loop).
      // Actually live path needs candidate too — we already collected candidate inside pinned check?
      // No, live path collected oracle only; we need candidate for live too.
      for (const scenario of scenarios) {
        const candidateOut = join(scratch, `${scenario.id}.candidate.json`);
        // If we already created candidateOut in live loop, reuse; but we didn't create candidate in live loop above,
        // we only did oracle. So create candidate now.
        const common = ["--corpus", absoluteCorpus, "--root", absoluteRoot, "--out"];
        const candidate = await superviseRunner({
          implementation: "candidate",
          scenarioId: scenario.id,
          command: runnerExecutable(absoluteRoot),
          args: ["run", ...common, candidateOut, "--scenario", scenario.id],
          cwd: absoluteRoot,
        });
        assertCompleted(candidate, absoluteOut);
        candidateRecords.push(
          readSingleRecord(candidateOut, "candidate", scenario.id, absoluteOut),
        );
      }
      // For live mode, oracleRecords already populated, candidateRecords now populated.
      // Deduplicate: we have oracle from first loop, candidate from second loop — that's correct.
    }
    // For live mode we double-collected candidate; need to ensure oracleRecords already has per-scenario.
    // For pinned mode, oracleRecords is full pinned set, candidateRecords is per-scenario.
    // Normalize: if pinned, oracleRecords is pinned set (already full); candidate already full.
    // If live, oracleRecords was built per-scenario, but candidate was built in second loop — good.
    // However our pinned branch collected candidate in its own loop, live branch collected candidate in second loop.
    // So both branches have correct arrays. For live we need to ensure we didn't double push oracle.
    // Live oracleRecords already pushed per-scenario in first loop; candidate pushed in second loop — fine.

    const oraclePath = join(absoluteOut, "oracle.json");
    const candidatePath = join(absoluteOut, "candidate.json");
    const auditPath = join(absoluteOut, "audit.json");
    // In pinned mode, oraclePath is a copy of pinned file (but re-serialized canonically).
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
  const pinnedOracle = optionValue(process.argv, "--pinned-oracle");
  if (corpusDir === undefined || root === undefined || outDir === undefined) {
    console.error(
      "usage: run-differential.mjs --corpus <dir> --root <repo> --out-dir <directory> [--pinned-oracle <file>]",
    );
    process.exit(2);
  }
  // If no explicit pinned oracle but evidence exists at the frozen location, use it implicitly
  // to keep `npm run check:differential` working after TS removal without requiring flag.
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
  try {
    const audit = await runDifferential({ corpusDir, root, outDir, pinnedOracle: effectivePinned });
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
