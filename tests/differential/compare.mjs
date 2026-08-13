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
import { loadCorpus, platformName } from "./run-oracle.mjs";

const AUDIT_SCHEMA_VERSION = 2;
const SOURCE_FILE_COUNT_LIMIT = 100_000;
const SOURCE_BYTES_LIMIT = 256 * 1024 * 1024;
const GIT_OUTPUT_LIMIT = 8 * 1024 * 1024;
const GIT_TIMEOUT_MS = 10_000;
const LOWER_SHA256 = /^[0-9a-f]{64}$/u;
const GIT_OBJECT_ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/u;
const VERSION =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/u;
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

function assertExactKeys(value, expected, label) {
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    throw new Error(`${label} has unknown or missing fields`);
  }
}

function subjectFields(subject) {
  if (subject === "state-dir") {
    return ["scenarioId", "subject", "kind", "stateDirSha256"];
  }
  if (subject === "version-identity") {
    return ["scenarioId", "subject", "kind", "version"];
  }
  throw new Error(`record has unsupported subject ${JSON.stringify(subject)}`);
}

/** Validate a subject-specific canonical outcome record. */
export function validateRecord(record, source, expectedScenario = undefined) {
  const label = `${source} record`;
  if (!isObject(record)) {
    throw new Error(`malformed ${label}: expected an object`);
  }
  if (typeof record.subject !== "string" || !ALLOWED_SUBJECTS.has(record.subject)) {
    throw new Error(`malformed ${label}: unsupported subject`);
  }
  assertExactKeys(record, subjectFields(record.subject), label);
  if (
    typeof record.scenarioId !== "string" ||
    record.scenarioId.length === 0 ||
    Buffer.byteLength(record.scenarioId, "utf8") > CONTRACT_LIMITS.identifierBytes
  ) {
    throw new Error(`malformed ${label}: invalid scenarioId`);
  }
  if (record.kind !== "ok" && record.kind !== "error") {
    throw new Error(`malformed ${label}: unsupported outcome kind`);
  }
  if (expectedScenario !== undefined) {
    if (record.scenarioId !== expectedScenario.id) {
      throw new Error(
        `${source} record set is out of order, incomplete, or contains an extra scenario`,
      );
    }
    if (record.subject !== expectedScenario.subject) {
      throw new Error(`${source} record ${record.scenarioId} has a subject mismatch`);
    }
  }
  if (record.subject === "state-dir") {
    const valid =
      (record.kind === "ok" &&
        typeof record.stateDirSha256 === "string" &&
        LOWER_SHA256.test(record.stateDirSha256)) ||
      (record.kind === "error" && record.stateDirSha256 === null);
    if (!valid) {
      throw new Error(`malformed ${label}: invalid state-dir outcome`);
    }
  } else {
    const valid =
      (record.kind === "ok" &&
        typeof record.version === "string" &&
        Buffer.byteLength(record.version, "utf8") <= CONTRACT_LIMITS.identifierBytes &&
        VERSION.test(record.version)) ||
      (record.kind === "error" && record.version === null);
    if (!valid) {
      throw new Error(`malformed ${label}: invalid version-identity outcome`);
    }
  }
  return record;
}

/** Parse a bounded record file and require its exact canonical byte form. */
export function parseCanonicalRecords(text, source) {
  if (Buffer.byteLength(text, "utf8") > CONTRACT_LIMITS.recordsBytes) {
    throw new Error(`${source} record file exceeds the harness byte bound`);
  }
  let records;
  try {
    records = JSON.parse(text);
  } catch (error) {
    throw new Error(
      `${source} record file is not JSON: ${error instanceof Error ? error.message : error}`,
    );
  }
  if (!Array.isArray(records) || records.length > CONTRACT_LIMITS.scenarios) {
    throw new Error(`${source} record file must be a bounded JSON array`);
  }
  if (`${canonicalizeJson(records)}\n` !== text) {
    throw new Error(`${source} record file is not exact canonical JSON with one trailing newline`);
  }
  return records;
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

function differingFields(oracle, candidate) {
  return [...new Set([...Object.keys(oracle), ...Object.keys(candidate)])]
    .sort()
    .filter((key) => canonicalizeJson(oracle[key]) !== canonicalizeJson(candidate[key]));
}

function perSubjectCoverage(scenarios, statuses) {
  const coverage = {};
  for (const subject of [...ALLOWED_SUBJECTS].sort()) {
    const subjectScenarios = scenarios.filter((scenario) => scenario.subject === subject);
    if (subjectScenarios.length === 0) {
      continue;
    }
    const subjectStatuses = statuses.filter((status) => status.subject === subject);
    coverage[subject] = {
      total: subjectScenarios.length,
      applicable: subjectStatuses.filter((status) => status.status !== "skipped-platform").length,
      required: subjectScenarios.filter((scenario) => scenario.parity === "required").length,
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
  const applicable = scenarios.filter(
    (scenario) => scenario.platforms.includes("*") || scenario.platforms.includes(platform),
  );
  validateRecordSet(oracleRecords, "oracle", applicable);
  validateRecordSet(candidateRecords, "candidate", applicable);

  const parity = [];
  const deviated = [];
  const informational = [];
  const informationalDeviations = [];
  const statuses = [];
  let applicableIndex = 0;
  for (const scenario of scenarios) {
    if (!scenario.platforms.includes("*") && !scenario.platforms.includes(platform)) {
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
    const matches = canonicalizeJson(oracle) === canonicalizeJson(candidate);
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
      reason: "record-mismatch",
      differingFields: differingFields(oracle, candidate),
      oracle: canonicalizeJson(oracle),
      candidate: canonicalizeJson(candidate),
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
  const audit = {
    schemaVersion: AUDIT_SCHEMA_VERSION,
    corpusVersion,
    corpusDigest,
    sourceIdentity,
    platform,
    oracleRecords: oracleRecords.length,
    candidateRecords: candidateRecords.length,
    oracleRecordsSha256: sha256Hex(`${canonicalizeJson(oracleRecords)}\n`),
    candidateRecordsSha256: sha256Hex(`${canonicalizeJson(candidateRecords)}\n`),
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
