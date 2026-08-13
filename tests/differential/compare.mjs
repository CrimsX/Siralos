/**
 * Differential harness — comparator and audit (ADR 0033).
 *
 * Compares the oracle and candidate canonical outcome records for the
 * scenario corpus and emits the migration audit report:
 * per-scenario status (parity / deviated / skipped-platform /
 * informational), coverage, and the deviation inventory that drives
 * remediation. Deterministic: the audit contains no timestamps.
 *
 * Usage:
 *   node tests/differential/compare.mjs \
 *     --oracle <file> --candidate <file> --corpus <dir> --out <audit file>
 *
 * Exit codes: 0 = parity held, 1 = required deviation (gate red),
 * 2 = harness error.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";
import { canonicalizeJson } from "./shared/canonical.mjs";
import { loadCorpus, platformName } from "./run-oracle.mjs";

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

function harnessError(message) {
  console.error(`comparator: ${message}`);
  process.exit(2);
}

/** Validate a record's shape; throws on malformed records. */
export function validateRecord(record, source) {
  if (
    record === null ||
    typeof record !== "object" ||
    Array.isArray(record) ||
    typeof record.scenarioId !== "string" ||
    typeof record.subject !== "string" ||
    !["ok", "error"].includes(record.kind)
  ) {
    throw new Error(`malformed ${source} record: ${JSON.stringify(record)}`);
  }
}

/**
 * Pure comparison: returns `{ audit, deviations }` where deviations are
 * required-scenario failures. Never throws for deviations; throws only
 * for malformed input.
 */
export function runCompare({ oracleRecords, candidateRecords, scenarios, platform }) {
  const oracleByScenario = new Map();
  const candidateByScenario = new Map();
  for (const record of oracleRecords) {
    validateRecord(record, "oracle");
    oracleByScenario.set(record.scenarioId, record);
  }
  for (const record of candidateRecords) {
    validateRecord(record, "candidate");
    candidateByScenario.set(record.scenarioId, record);
  }
  for (const scenario of scenarios) {
    if (scenario.id === undefined || scenario.subject === undefined) {
      throw new Error("malformed scenario in comparison input");
    }
  }

  const applicable = scenarios.filter(
    (scenario) => scenario.platforms.includes("*") || scenario.platforms.includes(platform),
  );
  const parity = [];
  const deviated = [];
  const informational = [];
  const informationalDeviations = [];

  for (const scenario of applicable) {
    const oracle = oracleByScenario.get(scenario.id);
    const candidate = candidateByScenario.get(scenario.id);
    const outcome = {
      scenarioId: scenario.id,
      subject: scenario.subject,
      parity: scenario.parity,
    };
    if (oracle === undefined || candidate === undefined) {
      const entry = {
        ...outcome,
        reason: "incomplete",
        oracle: oracle === undefined ? null : canonicalizeJson(oracle),
        candidate: candidate === undefined ? null : canonicalizeJson(candidate),
      };
      if (scenario.parity === "informational") {
        informationalDeviations.push(entry);
      } else {
        deviated.push(entry);
      }
      continue;
    }
    if (canonicalizeJson(oracle) === canonicalizeJson(candidate)) {
      (scenario.parity === "informational" ? informational : parity).push(scenario.id);
    } else {
      const entry = {
        ...outcome,
        reason: "record-mismatch",
        oracle: canonicalizeJson(oracle),
        candidate: canonicalizeJson(candidate),
      };
      if (scenario.parity === "informational") {
        informationalDeviations.push(entry);
      } else {
        deviated.push(entry);
      }
    }
  }

  const applicableIds = new Set(applicable.map((scenario) => scenario.id));
  const skipped = scenarios
    .map((scenario) => scenario.id)
    .filter((id) => !applicableIds.has(id))
    .sort();

  const audit = {
    schemaVersion: 1,
    corpusVersion: null,
    platform,
    oracleRecords: oracleRecords.length,
    candidateRecords: candidateRecords.length,
    parity: parity.sort(),
    deviated,
    informational: informational.sort(),
    informationalDeviations,
    skipped,
    deviationCount: deviated.length,
    parityHeld: deviated.length === 0,
  };
  return { audit, deviations: deviated };
}

function main() {
  const oraclePath = optionValue(process.argv, "--oracle");
  const candidatePath = optionValue(process.argv, "--candidate");
  const corpusDir = optionValue(process.argv, "--corpus");
  const out = optionValue(process.argv, "--out");
  if (
    oraclePath === undefined ||
    candidatePath === undefined ||
    corpusDir === undefined ||
    out === undefined
  ) {
    harnessError(
      "usage: compare.mjs --oracle <file> --candidate <file> --corpus <dir> --out <audit>",
    );
  }
  let audit;
  try {
    const { manifest, scenarios } = loadCorpus(corpusDir);
    const oracleRecords = JSON.parse(readFileSync(oraclePath, "utf8"));
    const candidateRecords = JSON.parse(readFileSync(candidatePath, "utf8"));
    const result = runCompare({
      oracleRecords,
      candidateRecords,
      scenarios,
      platform: platformName(),
    });
    audit = { ...result.audit, corpusVersion: manifest.corpusVersion };
  } catch (error) {
    harnessError(String(error instanceof Error ? error.message : error));
  }
  try {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, canonicalizeJson(audit) + "\n", "utf8");
  } catch (error) {
    harnessError(`cannot write ${out}: ${String(error instanceof Error ? error.message : error)}`);
  }
  if (!audit.parityHeld) {
    console.error("Differential audit: DEVIATIONS (gate red):");
    for (const deviation of audit.deviated) {
      console.error(`  - ${deviation.scenarioId} (${deviation.reason})`);
    }
    process.exit(1);
  }
  console.log(
    `Differential audit: parity held (${audit.parity.length} parity, ${audit.skipped.length} skipped-platform, ${audit.informationalDeviations.length} informational deviations).`,
  );
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
