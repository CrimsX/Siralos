/**
 * Differential harness comparator and evidence audit (ADR 0033).
 *
 * Exit codes: 0 = required parity held, 1 = required deviation,
 * 2 = malformed/incomplete harness protocol.
 */
import { lstatSync, mkdirSync, readFileSync, readlinkSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { canonicalizeJson, sha256Hex } from "./shared/canonical.mjs";
import {
  ALLOWED_PARITY,
  ALLOWED_SUBJECTS,
  CONTRACT_LIMITS,
  readBoundedUtf8File,
} from "./shared/contract.mjs";
import {
  SCENARIO_OUTCOME,
  canonicalRecordDocument,
  parseCanonicalRecordDocument,
  validateOutcomeRecord,
} from "./shared/protocol.mjs";
import { loadCorpus, platformName } from "./run-oracle.mjs";

const AUDIT_SCHEMA_VERSION = 3;
const SOURCE_FILE_COUNT_LIMIT = 100_000;
const SOURCE_BYTES_LIMIT = 256 * 1024 * 1024;
const GIT_OUTPUT_LIMIT = 8 * 1024 * 1024;
const GIT_TIMEOUT_MS = 10_000;
const GIT_OBJECT_ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/u;
const FATAL_UTF8 = new TextDecoder("utf-8", { fatal: true });

function optionValue(args, name) {
  const index = args.indexOf(name);
  return index === -1 || index + 1 >= args.length ? undefined : args[index + 1];
}

function harnessError(message) {
  console.error(`comparator: ${message}`);
  process.exit(2);
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** Validate a subject-specific canonical outcome record. */
export function validateRecord(record, source, expectedScenario = undefined) {
  return validateOutcomeRecord(record, source, expectedScenario);
}

/** Parse a bounded record file and require its exact canonical byte form. */
export function parseCanonicalRecords(text, source) {
  return parseCanonicalRecordDocument(text, source);
}

function validateComparisonScenarios(scenarios) {
  if (
    !Array.isArray(scenarios) ||
    scenarios.length === 0 ||
    scenarios.length > CONTRACT_LIMITS.scenarios
  ) {
    throw new Error("comparison input has an invalid scenario set");
  }
  const ids = new Set();
  for (const scenario of scenarios) {
    if (
      !isObject(scenario) ||
      typeof scenario.id !== "string" ||
      typeof scenario.subject !== "string" ||
      !ALLOWED_SUBJECTS.has(scenario.subject) ||
      !Array.isArray(scenario.platforms) ||
      typeof scenario.parity !== "string" ||
      !ALLOWED_PARITY.has(scenario.parity)
    ) {
      throw new Error("malformed scenario in comparison input");
    }
    if (ids.has(scenario.id)) {
      throw new Error(`comparison input contains duplicate scenario ${scenario.id}`);
    }
    ids.add(scenario.id);
  }
}

function validateRecordSet(records, source, applicable) {
  if (!Array.isArray(records) || records.length !== applicable.length) {
    throw new Error(`${source} record set is incomplete or contains extra records`);
  }
  const seen = new Set();
  for (const [index, record] of records.entries()) {
    validateRecord(record, source, applicable[index]);
    if (seen.has(record.scenarioId)) {
      throw new Error(`${source} record set contains duplicate scenario ${record.scenarioId}`);
    }
    seen.add(record.scenarioId);
  }
}

function pathSegment(key) {
  return /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(key) ? `.${key}` : `[${JSON.stringify(key)}]`;
}

function valueType(value) {
  if (value === null) return "null";
  if (Array.isArray(value)) return "array";
  return typeof value;
}

/** Structured semantic differences: objects are maps, arrays preserve order. */
export function semanticDifferences(oracle, candidate, path = "$") {
  const oracleType = valueType(oracle);
  const candidateType = valueType(candidate);
  if (oracleType !== candidateType) {
    return [
      {
        path,
        kind: "TYPE_CHANGED",
        oracle,
        candidate,
        policy: "type-sensitive",
      },
    ];
  }
  if (oracleType === "array") {
    if (oracle.length !== candidate.length) {
      return [
        {
          path,
          kind: "ORDER_CHANGED",
          oracle,
          candidate,
          policy: "sequence-order-authoritative",
        },
      ];
    }
    const oracleMembers = oracle.map(canonicalizeJson).sort();
    const candidateMembers = candidate.map(canonicalizeJson).sort();
    if (
      canonicalizeJson(oracle) !== canonicalizeJson(candidate) &&
      canonicalizeJson(oracleMembers) === canonicalizeJson(candidateMembers)
    ) {
      return [
        {
          path,
          kind: "ORDER_CHANGED",
          oracle,
          candidate,
          policy: "sequence-order-authoritative",
        },
      ];
    }
    const differences = [];
    for (let index = 0; index < oracle.length; index += 1) {
      differences.push(
        ...semanticDifferences(oracle[index], candidate[index], `${path}[${index}]`),
      );
    }
    return differences;
  }
  if (oracleType === "object") {
    const differences = [];
    const oracleKeys = Object.keys(oracle).sort();
    const candidateKeys = Object.keys(candidate).sort();
    for (const key of oracleKeys) {
      const childPath = `${path}${pathSegment(key)}`;
      if (!Object.hasOwn(candidate, key)) {
        differences.push({
          path: childPath,
          kind: "MISSING_IN_CANDIDATE",
          oracle: oracle[key],
          candidate: null,
          policy: "object-key-order-insensitive",
        });
      } else {
        differences.push(...semanticDifferences(oracle[key], candidate[key], childPath));
      }
    }
    for (const key of candidateKeys) {
      if (!Object.hasOwn(oracle, key)) {
        differences.push({
          path: `${path}${pathSegment(key)}`,
          kind: "EXTRA_IN_CANDIDATE",
          oracle: null,
          candidate: candidate[key],
          policy: "object-key-order-insensitive",
        });
      }
    }
    return differences;
  }
  if (Object.is(oracle, candidate)) {
    return [];
  }
  return [
    {
      path,
      kind: path === "$.error.category" ? "ERROR_CATEGORY_CHANGED" : "VALUE_CHANGED",
      oracle,
      candidate,
      policy: "scalar-exact",
    },
  ];
}

function perSubjectCoverage(scenarios, statuses) {
  const coverage = {};
  for (const subject of [...ALLOWED_SUBJECTS].sort()) {
    const subjectScenarios = scenarios.filter((scenario) => scenario.subject === subject);
    if (subjectScenarios.length === 0) {
      continue;
    }
    const subjectStatuses = statuses.filter((status) => status.subject === subject);
    const requiredApplicable = subjectStatuses.filter(
      (status) => status.parity === "required" && status.status !== "skipped-platform",
    );
    coverage[subject] = {
      total: subjectScenarios.length,
      applicable: subjectStatuses.filter((status) => status.status !== "skipped-platform").length,
      required: subjectScenarios.filter((scenario) => scenario.parity === "required").length,
      requiredApplicable: requiredApplicable.length,
      matchedRequired: requiredApplicable.filter((status) => status.status === "parity").length,
      informational: subjectScenarios.filter((scenario) => scenario.parity === "informational")
        .length,
      parity: subjectStatuses.filter((status) => status.status === "parity").length,
      deviated: subjectStatuses.filter((status) => status.status === "deviated").length,
      informationalParity: subjectStatuses.filter(
        (status) => status.status === "informational-parity",
      ).length,
      informationalDeviated: subjectStatuses.filter(
        (status) => status.status === "informational-deviated",
      ).length,
      skippedPlatform: subjectStatuses.filter((status) => status.status === "skipped-platform")
        .length,
    };
  }
  return coverage;
}

/**
 * Compare two already parsed record sets after enforcing the complete
 * one-record-per-applicable-scenario protocol.
 */
export function runCompare({
  oracleRecords,
  candidateRecords,
  scenarios,
  platform,
  corpusVersion = null,
  corpusDigest = null,
  sourceIdentity = null,
}) {
  if (platform !== "windows" && platform !== "posix") {
    throw new Error(`unsupported comparison platform ${JSON.stringify(platform)}`);
  }
  validateComparisonScenarios(scenarios);
  validateRecordSet(oracleRecords, "oracle", scenarios);
  validateRecordSet(candidateRecords, "candidate", scenarios);

  const parity = [];
  const deviated = [];
  const informational = [];
  const informationalDeviations = [];
  const statuses = [];
  let applicableIndex = 0;
  for (const scenario of scenarios) {
    if (!scenario.platforms.includes("*") && !scenario.platforms.includes(platform)) {
      const oracle = oracleRecords[applicableIndex];
      const candidate = candidateRecords[applicableIndex];
      applicableIndex += 1;
      const expectedUnsupported =
        oracle.outcome === SCENARIO_OUTCOME.UNSUPPORTED &&
        candidate.outcome === SCENARIO_OUTCOME.UNSUPPORTED &&
        oracle.error.category === "PLATFORM_NOT_APPLICABLE" &&
        candidate.error.category === "PLATFORM_NOT_APPLICABLE";
      if (!expectedUnsupported) {
        throw new Error(
          `non-applicable scenario ${scenario.id} must be an explicit UNSUPPORTED outcome on both runners`,
        );
      }
      statuses.push({
        scenarioId: scenario.id,
        subject: scenario.subject,
        parity: scenario.parity,
        status: "skipped-platform",
      });
      continue;
    }
    const oracle = oracleRecords[applicableIndex];
    const candidate = candidateRecords[applicableIndex];
    applicableIndex += 1;
    const unavailableOutcome = [SCENARIO_OUTCOME.UNIMPLEMENTED, SCENARIO_OUTCOME.UNSUPPORTED].find(
      (outcome) => oracle.outcome === outcome || candidate.outcome === outcome,
    );
    const matches =
      unavailableOutcome === undefined && canonicalizeJson(oracle) === canonicalizeJson(candidate);
    if (matches) {
      if (scenario.parity === "informational") {
        informational.push(scenario.id);
      } else {
        parity.push(scenario.id);
      }
      statuses.push({
        scenarioId: scenario.id,
        subject: scenario.subject,
        parity: scenario.parity,
        status: scenario.parity === "informational" ? "informational-parity" : "parity",
      });
      continue;
    }
    const entry = {
      scenarioId: scenario.id,
      subject: scenario.subject,
      parity: scenario.parity,
      reason:
        unavailableOutcome === SCENARIO_OUTCOME.UNIMPLEMENTED
          ? "unimplemented"
          : unavailableOutcome === SCENARIO_OUTCOME.UNSUPPORTED
            ? "unsupported-applicable-scenario"
            : scenario.parity === "informational"
              ? "host-owned OS account fallback is outside the declared fixture authority"
              : "record-mismatch",
      authoritativeDecision: scenario.parity === "informational" ? "ADR-0033" : null,
      status: scenario.parity === "informational" ? "accepted" : "unexplained",
      referenceBehavior: oracle,
      candidateBehavior: candidate,
      differences:
        unavailableOutcome === undefined
          ? semanticDifferences(oracle, candidate)
          : [
              {
                path: "$.outcome",
                kind: "NON_COMPLETION",
                oracle: oracle.outcome,
                candidate: candidate.outcome,
                policy: "required-applicable-scenario-must-be-implemented",
              },
            ],
    };
    if (scenario.parity === "informational") {
      informationalDeviations.push(entry);
    } else {
      deviated.push(entry);
    }
    statuses.push({
      scenarioId: scenario.id,
      subject: scenario.subject,
      parity: scenario.parity,
      status: scenario.parity === "informational" ? "informational-deviated" : "deviated",
    });
  }

  const skipped = statuses
    .filter((status) => status.status === "skipped-platform")
    .map((status) => status.scenarioId)
    .sort();
  const requiredScenarios = scenarios.filter((scenario) => scenario.parity === "required");
  const requiredApplicable = requiredScenarios.filter(
    (scenario) => scenario.platforms.includes("*") || scenario.platforms.includes(platform),
  );
  const audit = {
    schemaVersion: AUDIT_SCHEMA_VERSION,
    corpusVersion,
    corpusDigest,
    sourceIdentity,
    referenceIdentity: sourceIdentity?.reference ?? null,
    candidateIdentity: sourceIdentity?.candidate ?? null,
    platform,
    totalScenarios: scenarios.length,
    applicableScenarios: statuses.filter((status) => status.status !== "skipped-platform").length,
    requiredScenarios: requiredScenarios.length,
    requiredApplicableScenarios: requiredApplicable.length,
    matchedRequiredScenarios: parity.length,
    referenceRecords: oracleRecords.length,
    candidateRecords: candidateRecords.length,
    referenceRecordsSha256: sha256Hex(canonicalRecordDocument(oracleRecords)),
    candidateRecordsSha256: sha256Hex(canonicalRecordDocument(candidateRecords)),
    perSubject: perSubjectCoverage(scenarios, statuses),
    scenarios: statuses,
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

function runGit(root, args, label) {
  const result = spawnSync(
    "git",
    [
      "--no-optional-locks",
      "-c",
      "core.fsmonitor=false",
      "-c",
      "core.untrackedCache=false",
      "-C",
      root,
      ...args,
    ],
    {
      encoding: "buffer",
      env: { ...process.env, GIT_OPTIONAL_LOCKS: "0", GIT_PAGER: "cat" },
      timeout: GIT_TIMEOUT_MS,
      maxBuffer: GIT_OUTPUT_LIMIT,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  if (result.error !== undefined || result.signal !== null || result.status !== 0) {
    throw new Error(`cannot collect ${label} for the differential audit`);
  }
  return result.stdout;
}

function frame(hash, label, bytes) {
  hash.update(`${label.length}:${label}:${bytes.length}:`, "utf8");
  hash.update(bytes);
}

/** Decode Git's NUL-delimited path protocol without lossy substitution. */
export function decodeGitPathList(bytes) {
  let pathText;
  try {
    pathText = FATAL_UTF8.decode(bytes);
  } catch {
    throw new Error("git returned a source path that is not valid UTF-8");
  }
  if (pathText.length > 0 && !pathText.endsWith("\0")) {
    throw new Error("git returned a truncated source path list");
  }
  return pathText
    .split("\0")
    .filter((path) => path.length > 0)
    .sort();
}

/** Exact identity of the commit and non-ignored source files under test. */
export function collectSourceIdentity(root) {
  const canonicalRoot = resolve(root);
  const commit = runGit(canonicalRoot, ["rev-parse", "HEAD"], "source commit")
    .toString("utf8")
    .trim();
  if (!GIT_OBJECT_ID.test(commit)) {
    throw new Error("git returned a noncanonical source commit");
  }
  const list = runGit(
    canonicalRoot,
    ["ls-files", "-z", "--cached", "--others", "--exclude-per-directory=.gitignore"],
    "source file list",
  );
  const paths = decodeGitPathList(list);
  if (paths.length === 0 || paths.length > SOURCE_FILE_COUNT_LIMIT) {
    throw new Error("source file list is outside the audit bound");
  }
  const hash = createHash("sha256");
  let aggregateBytes = 0;
  for (const path of paths) {
    if (isAbsolute(path) || path.split(/[\\/]/u).includes("..")) {
      throw new Error("git returned an unsafe source path");
    }
    const absolute = resolve(canonicalRoot, path);
    const back = relative(canonicalRoot, absolute);
    if (back.startsWith("..") || isAbsolute(back)) {
      throw new Error("source file escapes the repository");
    }
    let bytes;
    let kind;
    try {
      const metadata = lstatSync(absolute);
      if (metadata.isSymbolicLink()) {
        bytes = Buffer.from(readlinkSync(absolute), "utf8");
        kind = "symlink";
      } else if (metadata.isFile()) {
        bytes = readFileSync(absolute);
        kind = "file";
      } else if (metadata.isDirectory()) {
        bytes = Buffer.alloc(0);
        kind = "gitlink";
      } else {
        throw new Error("unsupported source file type");
      }
    } catch (error) {
      if (error?.code !== "ENOENT") {
        throw error;
      }
      bytes = Buffer.alloc(0);
      kind = "missing";
    }
    aggregateBytes += bytes.length;
    if (aggregateBytes > SOURCE_BYTES_LIMIT) {
      throw new Error("source files exceed the audit byte bound");
    }
    frame(hash, `${kind}:${path.replaceAll("\\", "/")}`, bytes);
  }
  return {
    commit,
    sourceTreeSha256: hash.digest("hex"),
    sourceFiles: paths.length,
    reference: {
      implementation: "typescript-reference",
      commit,
    },
    candidate: {
      implementation: "rust-candidate",
      commit,
    },
  };
}

function main() {
  const oraclePath = optionValue(process.argv, "--oracle");
  const candidatePath = optionValue(process.argv, "--candidate");
  const corpusDir = optionValue(process.argv, "--corpus");
  const root = optionValue(process.argv, "--root");
  const out = optionValue(process.argv, "--out");
  if (
    oraclePath === undefined ||
    candidatePath === undefined ||
    corpusDir === undefined ||
    root === undefined ||
    out === undefined
  ) {
    harnessError(
      "usage: compare.mjs --oracle <file> --candidate <file> --corpus <dir> --root <repo> --out <audit>",
    );
  }
  let audit;
  try {
    const { manifest, scenarios, corpusDigest } = loadCorpus(corpusDir);
    const oracleRecords = parseCanonicalRecords(
      readBoundedUtf8File(oraclePath, CONTRACT_LIMITS.recordsBytes, "oracle record file"),
      "oracle",
    );
    const candidateRecords = parseCanonicalRecords(
      readBoundedUtf8File(candidatePath, CONTRACT_LIMITS.recordsBytes, "candidate record file"),
      "candidate",
    );
    audit = runCompare({
      oracleRecords,
      candidateRecords,
      scenarios,
      platform: platformName(),
      corpusVersion: manifest.corpusVersion,
      corpusDigest,
      sourceIdentity: collectSourceIdentity(root),
    }).audit;
  } catch (error) {
    harnessError(String(error instanceof Error ? error.message : error));
  }
  try {
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(out, `${canonicalizeJson(audit)}\n`, "utf8");
  } catch (error) {
    harnessError(`cannot write audit: ${String(error instanceof Error ? error.message : error)}`);
  }
  if (!audit.parityHeld) {
    console.error("Differential audit: DEVIATIONS (gate red):");
    for (const deviation of audit.deviated) {
      console.error(`  - ${deviation.scenarioId} (${deviation.reason})`);
    }
    process.exit(1);
  }
  console.log(
    `Differential audit: parity held (${audit.parity.length} required parity, ${audit.skipped.length} skipped-platform, ${audit.informationalDeviations.length} informational deviations).`,
  );
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
