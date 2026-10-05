/** Authoritative end-to-end R2 acceptance command (ADR 0033). Pinned mode post-TS-archive (decision 40). */
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { canonicalizeJson, sha256Hex } from "./shared/canonical.mjs";
import {
  CONTRACT_LIMITS,
  CorpusIntegrityError,
  loadValidatedCorpus,
  readBoundedUtf8File,
} from "./shared/contract.mjs";
import {
  EvidenceIntegrityError,
  loadPinnedEvidence,
  validateExpectationRecords,
} from "./shared/evidence.mjs";
import { canonicalRecordDocument, parseCanonicalRecordDocument } from "./shared/protocol.mjs";
import { RUNNER_PROCESS_LIMITS, superviseRunner } from "./shared/runner-process.mjs";
import { collectSourceIdentity, runCompare, validateRecord } from "./compare.mjs";

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
    const records = parseCanonicalRecordDocument(
      readBoundedUtf8File(path, CONTRACT_LIMITS.recordsBytes, "runner record file"),
      implementation,
    );
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

function loadExpectations(path, outDir, scenarios) {
  try {
    const text = readBoundedUtf8File(
      resolve(path),
      CONTRACT_LIMITS.recordsBytes,
      "post-freeze expectations",
    );
    const records = parseCanonicalRecordDocument(text, "expectation");
    validateExpectationRecords(records, scenarios);
    return { records, sha256: sha256Hex(text) };
  } catch (error) {
    const code =
      error instanceof EvidenceIntegrityError
        ? error.code
        : error instanceof CorpusIntegrityError
          ? error.code
          : "EXPECTATIONS_READ_FAILURE";
    const detail =
      error instanceof EvidenceIntegrityError
        ? error.message
        : "post-freeze expectations could not be loaded";
    const failure = {
      implementation: "reference",
      scenarioId: "<post-freeze-expectations>",
      outcome: "HARNESS_ERROR",
      category: "PINNED_ORACLE_FAILURE",
      code,
      message: detail,
    };
    writeFailure(outDir, { schemaVersion: 1, parityHeld: false, runnerFailure: failure });
    const e = new Error(detail);
    e.exitCode = 2;
    throw e;
  }
}

function loadPinnedEvidenceForRun(path, outDir, scenarios) {
  try {
    return loadPinnedEvidence(path, scenarios);
  } catch (error) {
    const code = error instanceof EvidenceIntegrityError ? error.code : "PINNED_EVIDENCE_FAILURE";
    const detail =
      error instanceof EvidenceIntegrityError
        ? error.message
        : "pinned evidence could not be loaded";
    const failure = {
      implementation: "reference",
      scenarioId: "<pinned-evidence>",
      outcome: "HARNESS_ERROR",
      category: "PINNED_ORACLE_FAILURE",
      code,
      message: detail,
    };
    writeFailure(outDir, { schemaVersion: 1, parityHeld: false, runnerFailure: failure });
    const e = new Error(detail);
    e.exitCode = 2;
    throw e;
  }
}

/**
 * Load and validate the digest-bound supersession list.
 *
 * A supersession retires one FROZEN oracle record and replaces it with a
 * candidate-authored record, for the one case a frozen reference cannot express:
 * a value that legitimately changed with the product (the release version).
 * It removes nothing from `oracle.json` — the frozen evidence keeps the value it
 * recorded — and every refusal below is a hard stop (exit 2), so a supersession
 * can never half-apply:
 *
 *   - the document must carry exactly the declared keys and schema version;
 *   - `entriesSha256` must match the canonical form of its own `supersessions`
 *     array, so a hand-edited entry that forgot to re-stamp the document stops
 *     the run instead of passing review by accident;
 *   - a superseded id must exist in the corpus AND in the pinned oracle;
 *   - `oracleRecordSha256` must match the frozen record it claims to retire, so
 *     the list cannot silently outlive the evidence it was written against;
 *   - the replacement must be a valid record for that scenario, must keep the
 *     same outcome class (no UNSUPPORTED -> COMPLETED laundering), and must
 *     actually differ from the record it replaces;
 *   - `supersededIn` must equal the corpus version in force;
 *   - an id may be superseded once, and never also covered by a post-freeze
 *     expectation.
 */
function loadSupersessions(path, outDir, scenarios, oracleRecords, corpusVersion) {
  let documentSha256 = null;
  const fail = (code, scenarioId, message) => {
    writeFailure(outDir, {
      schemaVersion: 1,
      parityHeld: false,
      runnerFailure: {
        implementation: "reference",
        scenarioId,
        outcome: "HARNESS_ERROR",
        category: "PINNED_ORACLE_FAILURE",
        code,
        message,
        // The digest of the refused document travels with the refusal, so a
        // failure names exactly which bytes were rejected.
        supersessionsSha256: documentSha256,
      },
    });
    const error = new Error(message);
    error.exitCode = 2;
    throw error;
  };
  const documentKeys = ["schemaVersion", "corpusVersion", "entriesSha256", "supersessions"];
  const entryKeys = [
    "scenarioId",
    "reason",
    "supersededIn",
    "decision",
    "oracleRecordSha256",
    "record",
  ];
  let document;
  let text;
  try {
    text = readBoundedUtf8File(
      resolve(path),
      CONTRACT_LIMITS.recordsBytes,
      "supersessions evidence",
    );
  } catch (error) {
    fail(
      "SUPERSESSIONS_READ_FAILURE",
      "<supersessions>",
      `supersessions could not be read: ${error instanceof Error ? error.message : error}`,
    );
  }
  documentSha256 = sha256Hex(text);
  try {
    document = JSON.parse(text);
  } catch (error) {
    fail(
      "SUPERSESSIONS_MALFORMED",
      "<supersessions>",
      `supersessions is not valid JSON: ${error instanceof Error ? error.message : error}`,
    );
  }
  if (document === null || typeof document !== "object" || Array.isArray(document)) {
    fail("SUPERSESSIONS_MALFORMED", "<supersessions>", "supersessions must be a JSON object");
  }
  const actualKeys = Object.keys(document).sort();
  if (actualKeys.join(",") !== [...documentKeys].sort().join(",")) {
    fail(
      "SUPERSESSIONS_MALFORMED",
      "<supersessions>",
      `supersessions must carry exactly ${documentKeys.join(", ")}; found ${actualKeys.join(", ")}`,
    );
  }
  if (document.schemaVersion !== 1) {
    fail(
      "SUPERSESSIONS_MALFORMED",
      "<supersessions>",
      `unsupported supersessions schemaVersion ${JSON.stringify(document.schemaVersion)}`,
    );
  }
  if (document.corpusVersion !== corpusVersion) {
    fail(
      "SUPERSESSIONS_CORPUS_MISMATCH",
      "<supersessions>",
      `supersessions declare corpus version ${JSON.stringify(document.corpusVersion)} but the corpus is v${corpusVersion}`,
    );
  }
  if (!Array.isArray(document.supersessions) || document.supersessions.length > 64) {
    fail(
      "SUPERSESSIONS_MALFORMED",
      "<supersessions>",
      "supersessions must be an array of at most 64 entries",
    );
  }
  const entriesSha256 = sha256Hex(canonicalizeJson(document.supersessions));
  if (document.entriesSha256 !== entriesSha256) {
    fail(
      "SUPERSESSIONS_SELF_DIGEST_MISMATCH",
      "<supersessions>",
      `supersessions declare entriesSha256 ${JSON.stringify(document.entriesSha256)} but their entries hash to ${entriesSha256}; re-stamp the document after editing an entry`,
    );
  }
  const scenarioById = new Map(scenarios.map((scenario) => [scenario.id, scenario]));
  const oracleById = new Map(oracleRecords.map((record) => [record.scenarioId, record]));
  const byId = new Map();
  for (const entry of document.supersessions) {
    if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
      fail("SUPERSESSIONS_MALFORMED", "<supersessions>", "each supersession must be an object");
    }
    const keys = Object.keys(entry).sort();
    if (keys.join(",") !== [...entryKeys].sort().join(",")) {
      fail(
        "SUPERSESSIONS_MALFORMED",
        typeof entry.scenarioId === "string" ? entry.scenarioId : "<supersessions>",
        `each supersession must carry exactly ${entryKeys.join(", ")}; found ${keys.join(", ")}`,
      );
    }
    const { scenarioId } = entry;
    if (typeof scenarioId !== "string" || scenarioId.length === 0) {
      fail("SUPERSESSIONS_MALFORMED", "<supersessions>", "a supersession needs a scenario id");
    }
    if (byId.has(scenarioId)) {
      fail(
        "SUPERSESSIONS_DUPLICATE",
        scenarioId,
        `scenario ${scenarioId} is superseded more than once`,
      );
    }
    const scenario = scenarioById.get(scenarioId);
    if (scenario === undefined) {
      fail(
        "SUPERSESSIONS_UNKNOWN_SCENARIO",
        scenarioId,
        `supersession names scenario ${scenarioId}, which the corpus does not define`,
      );
    }
    const oracleRecord = oracleById.get(scenarioId);
    if (oracleRecord === undefined) {
      fail(
        "SUPERSESSIONS_UNKNOWN_SCENARIO",
        scenarioId,
        `supersession names scenario ${scenarioId}, which the pinned oracle does not cover`,
      );
    }
    for (const field of ["reason", "decision"]) {
      if (typeof entry[field] !== "string" || entry[field].trim().length === 0) {
        fail(
          "SUPERSESSIONS_MALFORMED",
          scenarioId,
          `supersession ${scenarioId} needs a non-empty ${field}`,
        );
      }
      if (entry[field].length > 512) {
        fail(
          "SUPERSESSIONS_MALFORMED",
          scenarioId,
          `supersession ${scenarioId} ${field} exceeds 512 characters`,
        );
      }
    }
    if (entry.supersededIn !== corpusVersion) {
      fail(
        "SUPERSESSIONS_CORPUS_MISMATCH",
        scenarioId,
        `supersession ${scenarioId} is stamped for corpus version ${JSON.stringify(entry.supersededIn)}, not v${corpusVersion}`,
      );
    }
    const frozenSha256 = sha256Hex(canonicalRecordDocument([oracleRecord]));
    if (entry.oracleRecordSha256 !== frozenSha256) {
      fail(
        "SUPERSESSIONS_ORACLE_MISMATCH",
        scenarioId,
        `supersession ${scenarioId} claims to retire oracle record ${JSON.stringify(entry.oracleRecordSha256)} but the pinned oracle record hashes to ${frozenSha256}`,
      );
    }
    let replacement;
    try {
      replacement = validateRecord(entry.record, "supersession", scenario);
    } catch (error) {
      fail(
        "SUPERSESSIONS_MALFORMED",
        scenarioId,
        `supersession ${scenarioId} carries an invalid replacement record: ${error instanceof Error ? error.message : error}`,
      );
    }
    if (replacement.scenarioId !== scenarioId) {
      fail(
        "SUPERSESSIONS_MALFORMED",
        scenarioId,
        `supersession ${scenarioId} carries a replacement for ${replacement.scenarioId}`,
      );
    }
    if (replacement.outcome !== oracleRecord.outcome) {
      fail(
        "SUPERSESSIONS_CLASS_CHANGE",
        scenarioId,
        `supersession ${scenarioId} changes the outcome class from ${oracleRecord.outcome} to ${replacement.outcome}`,
      );
    }
    if (canonicalizeJson(replacement) === canonicalizeJson(oracleRecord)) {
      fail(
        "SUPERSESSIONS_NO_CHANGE",
        scenarioId,
        `supersession ${scenarioId} replaces a record with an identical one`,
      );
    }
    byId.set(scenarioId, { entry, oracleRecord, record: replacement });
  }
  return { byId, documentSha256, entriesSha256 };
}

/** Execute candidate runner; oracle is either live (historical replay) or pinned. */
export async function runDifferential({
  corpusDir,
  root,
  outDir,
  pinnedOracle,
  expectationsPath,
  supersessionsPath,
}) {
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
  let oracleRecords;
  let expectationScenarioIds = null;
  let expectationRecordsSha256 = null;
  let supersededDisclosure = null;
  let supersessionsSha256 = null;
  let supersessionsEntriesSha256 = null;
  let pinnedEvidence = null;
  let frozenOracleRecords = [];
  let expectationRecords = [];
  let supersessions = { byId: new Map() };

  if (pinnedOracle !== undefined) {
    // Verify the complete historical bundle before launching a candidate build.
    // This keeps a changed oracle, manifest, freeze audit, or candidate from
    // being silently treated as a current-source result.
    pinnedEvidence = loadPinnedEvidenceForRun(pinnedOracle, absoluteOut, scenarios);
    frozenOracleRecords = pinnedEvidence.oracleRecords;
    const pinnedIds = new Set(frozenOracleRecords.map((record) => record.scenarioId));
    if (expectationsPath !== undefined) {
      const loadedExpectations = loadExpectations(expectationsPath, absoluteOut, scenarios);
      expectationRecords = loadedExpectations.records;
      expectationRecordsSha256 = loadedExpectations.sha256;
    }
    const expectationIds = new Set(expectationRecords.map((record) => record.scenarioId));
    if (supersessionsPath !== undefined && existsSync(resolve(supersessionsPath))) {
      supersessions = loadSupersessions(
        supersessionsPath,
        absoluteOut,
        scenarios,
        frozenOracleRecords,
        manifest.corpusVersion,
      );
      supersessionsSha256 = supersessions.documentSha256;
      supersessionsEntriesSha256 = supersessions.entriesSha256;
    }
    const supersededIds = new Set(supersessions.byId.keys());
    const overlapping = [...pinnedIds].filter((id) => expectationIds.has(id));
    const uncovered = scenarios.filter(
      (scenario) => !pinnedIds.has(scenario.id) && !expectationIds.has(scenario.id),
    );
    const currentIds = new Set(scenarios.map((scenario) => scenario.id));
    const orphanFrozen = pinnedEvidence.scenarioIds.filter(
      (id) => !currentIds.has(id) && !supersededIds.has(id),
    );
    if (overlapping.length > 0 || uncovered.length > 0 || orphanFrozen.length > 0) {
      const failure = {
        implementation: "reference",
        scenarioId: "<evidence-coverage>",
        outcome: "HARNESS_ERROR",
        category: "PINNED_ORACLE_FAILURE",
        code: overlapping.length > 0 ? "EXPECTATIONS_OVERLAP" : "PINNED_MISMATCH",
        message:
          overlapping.length > 0
            ? `post-freeze expectations overlap the pinned freeze in ${overlapping.length} scenario(s)`
            : orphanFrozen.length > 0
              ? `pinned freeze contains ${orphanFrozen.length} scenario(s) absent from the current corpus without a supersession`
              : `${uncovered.length} current scenario(s) lack both a pinned record and a post-freeze expectation (freeze v32 vs current v${manifest.corpusVersion})`,
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
        // The harness lives in its own excluded workspace; build it by
        // manifest path and keep its artifacts in the shared target dir so
        // runnerExecutable() above still finds the binary.
        "--manifest-path",
        "harness/Cargo.toml",
        "--target-dir",
        "target",
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

    if (pinnedOracle !== undefined) {
      const expectationIds = new Set(expectationRecords.map((record) => record.scenarioId));
      const supersededIds = new Set(supersessions.byId.keys());
      // Reference records in exact corpus order: frozen oracle records, minus any
      // record a supersession retires, plus digest-bound post-freeze expectation
      // records and the superseding replacements. The audit discloses which
      // scenarios rely on candidate-authored expectations AND which retire a
      // frozen record, with the value, the reason, and both digests.
      const retainedOracleRecords = frozenOracleRecords.filter(
        (record) => !supersededIds.has(record.scenarioId),
      );
      const replacementRecords = [...supersessions.byId.values()].map(({ record }) => record);
      const recordsById = new Map(
        [...retainedOracleRecords, ...expectationRecords, ...replacementRecords].map((record) => [
          record.scenarioId,
          record,
        ]),
      );
      supersededDisclosure = [...supersessions.byId.values()].map(
        ({ entry, oracleRecord, record }) => ({
          scenarioId: entry.scenarioId,
          supersededIn: entry.supersededIn,
          decision: entry.decision,
          reason: entry.reason,
          oracleRecordSha256: entry.oracleRecordSha256,
          replacementRecordSha256: sha256Hex(canonicalRecordDocument([record])),
          oracleValue: oracleRecord,
          replacementValue: record,
          // Each printed supersession is self-contained: it names the exact list
          // document it came from and that document's own entry digest.
          supersessionsSha256: supersessions.documentSha256,
          entriesSha256: supersessions.entriesSha256,
        }),
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
      supersededDisclosure,
      supersessionsSha256,
      supersessionsEntriesSha256,
      pinnedEvidence: pinnedEvidence?.provenance ?? null,
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
  const supersessionsArg = optionValue(process.argv, "--supersessions");
  if (corpusDir === undefined || root === undefined || outDir === undefined) {
    console.error(
      "usage: run-differential.mjs --corpus <dir> --root <repo> --out-dir <directory> [--pinned-oracle <file>] [--expectations <file>] [--supersessions <file>]",
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
  let effectiveSupersessions = supersessionsArg;
  if (effectiveSupersessions === undefined) {
    const defaultSupersessions = resolve(
      root,
      "tests/differential/evidence/post-freeze/supersessions.json",
    );
    if (existsSync(defaultSupersessions)) {
      effectiveSupersessions = defaultSupersessions;
    }
  }
  try {
    const audit = await runDifferential({
      corpusDir,
      root,
      outDir,
      pinnedOracle: effectivePinned,
      expectationsPath: effectiveExpectations,
      supersessionsPath: effectiveSupersessions,
    });
    console.log(
      `Differential audit: parity held (${audit.matchedRequiredScenarios}/${audit.requiredApplicableScenarios} applicable required scenarios; ${audit.skipped.length} explicit platform skips; ${audit.informationalDeviations.length} accepted informational deviations).`,
    );
    if (audit.superseded.length > 0) {
      console.log(
        `(superseded reference records: ${audit.superseded.length} — ${audit.superseded
          .map((entry) => `${entry.scenarioId} [${entry.decision}]`)
          .join(", ")})`,
      );
    }
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
