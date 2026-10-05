/**
 * Adversarial suite for the digest-bound supersession list (W3.1).
 *
 * A supersession retires one frozen oracle record. The mechanism is only worth
 * anything if every way of abusing it stops the run, so this suite executes the
 * real runner against deliberately broken lists and requires an exit code of 2
 * with the named refusal code. It never rewrites the corpus, the oracle, or the
 * real supersession list; each case runs against a fixture under
 * `tests/differential/evidence/post-freeze/negative/` and writes to a temporary
 * output directory.
 *
 * Usage: node tests/differential/check-supersessions.mjs [--root <repo>]
 */
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { loadValidatedCorpus } from "./shared/contract.mjs";
import {
  EvidenceIntegrityError,
  loadPinnedEvidence,
  validateExpectationRecords,
  validateFreezeManifest,
  validateRawRecordSet,
} from "./shared/evidence.mjs";
import { parseCanonicalRecordDocument } from "./shared/protocol.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

/** Each broken list and the refusal code it must provoke. */
const CASES = [
  ["unknown-scenario.json", "SUPERSESSIONS_UNKNOWN_SCENARIO"],
  ["oracle-mismatch.json", "SUPERSESSIONS_ORACLE_MISMATCH"],
  ["class-change.json", "SUPERSESSIONS_CLASS_CHANGE"],
  ["no-change.json", "SUPERSESSIONS_NO_CHANGE"],
  ["duplicate.json", "SUPERSESSIONS_DUPLICATE"],
  ["malformed.json", "SUPERSESSIONS_MALFORMED"],
  ["corpus-mismatch.json", "SUPERSESSIONS_CORPUS_MISMATCH"],
  ["self-digest.json", "SUPERSESSIONS_SELF_DIGEST_MISMATCH"],
];

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

function main() {
  const root = resolve(optionValue(process.argv, "--root") ?? join(HERE, "..", ".."));
  const failures = [];
  for (const [fixture, expectedCode] of CASES) {
    const outDir = mkdtempSync(join(tmpdir(), "siralos-supersession-"));
    try {
      const result = spawnSync(
        process.execPath,
        [
          join(HERE, "run-differential.mjs"),
          "--corpus",
          join(HERE, "corpus"),
          "--root",
          root,
          "--out-dir",
          outDir,
          "--supersessions",
          join(HERE, "evidence", "post-freeze", "negative", fixture),
        ],
        { cwd: root, encoding: "utf8", timeout: 360_000 },
      );
      if (result.status !== 2) {
        failures.push(`${fixture}: expected exit 2, observed ${result.status}`);
        continue;
      }
      let code;
      try {
        const failure = JSON.parse(readFileSync(join(outDir, "failure.json"), "utf8"));
        code = failure.runnerFailure?.code;
      } catch (error) {
        failures.push(`${fixture}: no readable failure record (${error.message})`);
        continue;
      }
      if (code !== expectedCode) {
        failures.push(`${fixture}: expected ${expectedCode}, observed ${code}`);
      }
    } finally {
      rmSync(outDir, { recursive: true, force: true });
    }
  }
  if (failures.length > 0) {
    console.error("Supersession guard violations:");
    for (const failure of failures) {
      console.error(`  - ${failure}`);
    }
    console.error(
      `  ${failures.length} of ${CASES.length} adversarial cases did not fail closed. The supersession list is not safe to use.`,
    );
    process.exit(1);
  }
  console.log(
    `Supersession guards held: ${CASES.length} adversarial lists each refused with their named code (exit 2).`,
  );
}

function expectEvidenceCode(label, operation, expectedCode) {
  try {
    operation();
    return `${label}: unexpectedly passed`;
  } catch (error) {
    if (error instanceof EvidenceIntegrityError && error.code === expectedCode) {
      return null;
    }
    const code = error instanceof EvidenceIntegrityError ? error.code : "unexpected-error";
    return `${label}: expected ${expectedCode}, observed ${code}`;
  }
}

function runPureEvidenceChecks() {
  const failures = [];
  const evidenceDir = join(HERE, "evidence", "typescript-freeze-v32");
  const corpus = loadValidatedCorpus(join(HERE, "corpus"), "posix");
  const pinned = loadPinnedEvidence(join(evidenceDir, "oracle.json"), corpus.scenarios);
  const requiredIds = pinned.scenarioIds;
  const pinnedRecords = pinned.oracleRecords;
  const provenanceKeys = [
    "auditSha256",
    "candidateRecordsSha256",
    "corpusSha256",
    "corpusVersion",
    "freezeCommit",
    "freezeSha256",
    "frozenScenarioCount",
    "frozenScenarioIdsSha256",
    "oracleRecordsSha256",
  ];
  if (
    Object.keys(pinned.provenance).sort().join(",") !== provenanceKeys.sort().join(",") ||
    Object.values(pinned.provenance).some(
      (value) => typeof value !== "string" && typeof value !== "number",
    )
  ) {
    failures.push("pinned provenance contains an unexpected or non-scalar field");
  }
  const freezeManifest = JSON.parse(readFileSync(join(evidenceDir, "manifest.json"), "utf8"));
  failures.push(
    expectEvidenceCode(
      "tampered freeze manifest digest",
      () =>
        validateFreezeManifest(
          { ...freezeManifest, corpusSha256: "0".repeat(64) },
          freezeManifest.corpusVersion,
        ),
      "EVIDENCE_DIGEST_MISMATCH",
    ),
  );

  try {
    validateRawRecordSet(pinnedRecords, "pinned oracle", {
      requiredIds,
      orderedIds: requiredIds,
    });
  } catch (error) {
    failures.push(`valid pinned evidence was rejected: ${error?.code ?? "unexpected-error"}`);
  }

  const duplicate = [...pinnedRecords.slice(0, -1), pinnedRecords[0]];
  failures.push(
    expectEvidenceCode(
      "duplicate pinned record",
      () =>
        validateRawRecordSet(duplicate, "pinned oracle", {
          requiredIds,
          orderedIds: requiredIds,
        }),
      "EVIDENCE_DUPLICATE_RECORD",
    ),
  );

  const unknown = pinnedRecords.map((record, index) =>
    index === pinnedRecords.length - 1 ? { ...record, scenarioId: "unknown.scenario" } : record,
  );
  failures.push(
    expectEvidenceCode(
      "unknown pinned record",
      () =>
        validateRawRecordSet(unknown, "pinned oracle", {
          requiredIds,
          orderedIds: requiredIds,
        }),
      "EVIDENCE_UNKNOWN_RECORD",
    ),
  );

  failures.push(
    expectEvidenceCode(
      "missing pinned record",
      () =>
        validateRawRecordSet(pinnedRecords.slice(1), "pinned oracle", {
          requiredIds,
          orderedIds: requiredIds,
        }),
      "EVIDENCE_MISSING_RECORD",
    ),
  );

  const scenarioById = new Map(corpus.scenarios.map((scenario) => [scenario.id, scenario]));
  const mismatchedSubject = pinnedRecords.map((record, index) =>
    index === 0 ? { ...record, subject: "workspace-read" } : record,
  );
  failures.push(
    expectEvidenceCode(
      "pinned scenario subject mismatch",
      () =>
        validateRawRecordSet(mismatchedSubject, "pinned oracle", {
          requiredIds,
          orderedIds: requiredIds,
          scenarioById,
        }),
      "EVIDENCE_PROTOCOL_MALFORMED",
    ),
  );
  const expectationText = readFileSync(
    join(HERE, "evidence", "post-freeze", "expectations.json"),
    "utf8",
  );
  const expectations = parseCanonicalRecordDocument(expectationText, "expectation");
  try {
    validateExpectationRecords(expectations, corpus.scenarios);
  } catch (error) {
    failures.push(`valid expectations were rejected: ${error?.code ?? "unexpected-error"}`);
  }
  const expectationDuplicate = [...expectations.slice(0, -1), expectations[0]];
  failures.push(
    expectEvidenceCode(
      "duplicate expectation",
      () => validateExpectationRecords(expectationDuplicate, corpus.scenarios),
      "EVIDENCE_DUPLICATE_RECORD",
    ),
  );
  const expectationUnknown = expectations.map((record, index) =>
    index === expectations.length - 1 ? { ...record, scenarioId: "unknown.scenario" } : record,
  );
  failures.push(
    expectEvidenceCode(
      "unknown expectation",
      () => validateExpectationRecords(expectationUnknown, corpus.scenarios),
      "EVIDENCE_UNKNOWN_RECORD",
    ),
  );

  return failures.filter(Boolean);
}

let pureEvidenceFailures;
try {
  pureEvidenceFailures = runPureEvidenceChecks();
} catch (error) {
  console.error(`Evidence boundary guard could not start: ${error?.code ?? "unexpected-error"}`);
  process.exit(1);
}
if (pureEvidenceFailures.length > 0) {
  console.error("Evidence boundary guard violations:");
  for (const failure of pureEvidenceFailures) {
    console.error(`  - ${failure}`);
  }
  process.exit(1);
}

main();
