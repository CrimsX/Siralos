/**
 * Frozen differential evidence integrity and provenance helpers.
 *
 * The migration gate treats the pinned oracle, its historical candidate, the
 * freeze manifest, and the freeze audit as evidence rather than ordinary
 * fixtures. These helpers fail closed on malformed or ambiguous evidence before
 * a candidate is built. Diagnostics contain labels, codes, and scenario ids,
 * never record bodies or secret values.
 */
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { canonicalizeJson, sha256Hex } from "./canonical.mjs";
import { computeCorpusDigest, CONTRACT_LIMITS, readBoundedUtf8File } from "./contract.mjs";
import {
  canonicalRecordDocument,
  parseCanonicalRecordDocument,
  validateOutcomeRecord,
} from "./protocol.mjs";

const SHA256 = /^[0-9a-f]{64}$/u;
const COMMIT_ID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/u;
const FREEZE_KEYS = [
  "freezeCommit",
  "corpusVersion",
  "corpusSha256",
  "referenceRecordsSha256",
  "candidateRecordsSha256",
  "audit",
  "pinnedOracle",
  "pinnedCandidate",
  "manifest",
  "note",
];
const MANIFEST_KEYS = ["schemaVersion", "corpusVersion", "corpusSha256", "scenarios"];
const FILE_NAME = /^[a-z0-9][a-z0-9.-]*\.json$/u;
const FREEZE_MAX_BYTES = 64 * 1024;
const FREEZE_TEXT_MAX_BYTES = 4 * 1024;
const MANIFEST_ENTRY_MAX_BYTES = 160;

/** A named, value-free refusal for a frozen evidence boundary. */
export class EvidenceIntegrityError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "EvidenceIntegrityError";
    this.code = code;
  }
}

function fail(code, message) {
  throw new EvidenceIntegrityError(code, message);
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function exactKeys(value, expected, label) {
  if (!isObject(value)) {
    fail("EVIDENCE_MALFORMED", `${label} must be an object`);
  }
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (actual.length !== wanted.length || actual.some((key, index) => key !== wanted[index])) {
    fail("EVIDENCE_MALFORMED", `${label} has unknown or missing fields`);
  }
}

function boundedString(value, maximumBytes, label) {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    Buffer.byteLength(value, "utf8") > maximumBytes ||
    value.includes("\u0000")
  ) {
    fail("EVIDENCE_MALFORMED", `${label} is not a bounded non-empty string`);
  }
  return value;
}

function safeBasename(value, label) {
  boundedString(value, MANIFEST_ENTRY_MAX_BYTES, label);
  if (
    isAbsolute(value) ||
    value !== basename(value) ||
    value.includes("/") ||
    value.includes("\\") ||
    !FILE_NAME.test(value)
  ) {
    fail("EVIDENCE_MALFORMED", `${label} must be a contained canonical JSON file name`);
  }
  return value;
}

function declaredFilename(value, expected, label) {
  boundedString(value, FREEZE_TEXT_MAX_BYTES, label);
  if (value !== expected && !value.startsWith(`${expected} `)) {
    fail("EVIDENCE_MALFORMED", `${label} must name the expected evidence file`);
  }
  return expected;
}

function readJson(path, label, maximumBytes) {
  let text;
  try {
    text = readBoundedUtf8File(path, maximumBytes, label);
  } catch {
    fail("EVIDENCE_READ_FAILURE", `${label} could not be read as a bounded regular file`);
  }
  try {
    return { text, value: JSON.parse(text) };
  } catch {
    fail("EVIDENCE_MALFORMED", `${label} is not valid JSON`);
  }
}

function validateRecordDocument(text, source, label) {
  try {
    return parseCanonicalRecordDocument(text, source);
  } catch {
    fail("EVIDENCE_PROTOCOL_MALFORMED", `${label} is not a canonical record document`);
  }
}

/**
 * Reject duplicate records before any Set/Map normalization can hide them.
 * When `requiredIds` is supplied, the raw set must be exactly that set. When
 * `allowedIds` is supplied, every record must belong to that set but omissions
 * are allowed (used for post-freeze expectations).
 */
export function validateRawRecordSet(
  records,
  source,
  { requiredIds = null, allowedIds = null, orderedIds = null, scenarioById = null } = {},
) {
  if (!Array.isArray(records)) {
    fail("EVIDENCE_PROTOCOL_MALFORMED", `${source} records must be an array`);
  }
  const seen = new Set();
  for (const record of records) {
    try {
      validateOutcomeRecord(record, source);
    } catch {
      fail("EVIDENCE_PROTOCOL_MALFORMED", `${source} contains a malformed record`);
    }
    const id = record.scenarioId;
    if (seen.has(id)) {
      fail("EVIDENCE_DUPLICATE_RECORD", `${source} contains a duplicate scenario record`);
    }
    seen.add(id);
    if (scenarioById !== null) {
      const scenario = scenarioById.get(id);
      if (scenario === undefined) {
        fail("EVIDENCE_UNKNOWN_RECORD", `${source} contains a scenario absent from the corpus`);
      }
      try {
        validateOutcomeRecord(record, source, scenario);
      } catch {
        fail(
          "EVIDENCE_PROTOCOL_MALFORMED",
          `${source} contains a record with mismatched scenario data`,
        );
      }
    }
  }
  if (allowedIds !== null) {
    for (const id of seen) {
      if (!allowedIds.has(id)) {
        fail("EVIDENCE_UNKNOWN_RECORD", `${source} contains a scenario absent from the corpus`);
      }
    }
  }
  if (requiredIds !== null) {
    const required = new Set(requiredIds);
    // Report an extra id before a missing id so an unlisted record cannot be
    // hidden behind a less-specific truncation diagnostic.
    for (const id of seen) {
      if (!required.has(id)) {
        fail("EVIDENCE_UNKNOWN_RECORD", `${source} contains a scenario absent from its evidence`);
      }
    }
    for (const id of required) {
      if (!seen.has(id)) {
        fail("EVIDENCE_MISSING_RECORD", `${source} is missing a declared scenario record`);
      }
    }
  }
  if (orderedIds !== null) {
    for (const [index, id] of orderedIds.entries()) {
      if (records[index]?.scenarioId !== id) {
        fail("EVIDENCE_RECORD_ORDER", `${source} is not in its declared evidence order`);
      }
    }
  }
  return records;
}

/** Validate a post-freeze expectation document against the current corpus. */
export function validateExpectationRecords(records, scenarios) {
  const scenarioById = new Map(scenarios.map((scenario) => [scenario.id, scenario]));
  const allowedIds = new Set(scenarioById.keys());
  return validateRawRecordSet(records, "post-freeze expectation", {
    allowedIds,
    scenarioById,
  });
}

/** Validate the manifest carried beside a historical freeze. */
export function validateFreezeManifest(manifest, expectedVersion) {
  exactKeys(manifest, MANIFEST_KEYS, "freeze manifest");
  if (manifest.schemaVersion !== 3) {
    fail("EVIDENCE_VERSION_MISMATCH", "freeze manifest has an unsupported schema version");
  }
  if (
    !Number.isSafeInteger(manifest.corpusVersion) ||
    manifest.corpusVersion < 1 ||
    manifest.corpusVersion !== expectedVersion
  ) {
    fail("EVIDENCE_VERSION_MISMATCH", "freeze manifest corpus version does not match FREEZE.json");
  }
  if (typeof manifest.corpusSha256 !== "string" || !SHA256.test(manifest.corpusSha256)) {
    fail("EVIDENCE_DIGEST_MISMATCH", "freeze manifest has an invalid corpus digest");
  }
  if (
    !Array.isArray(manifest.scenarios) ||
    manifest.scenarios.length === 0 ||
    manifest.scenarios.length > CONTRACT_LIMITS.scenarios
  ) {
    fail("EVIDENCE_MALFORMED", "freeze manifest has an invalid scenario inventory");
  }
  const files = new Set();
  const scenarioIds = [];
  for (const [index, entry] of manifest.scenarios.entries()) {
    const label = `freeze manifest entry ${index}`;
    exactKeys(entry, ["file", "sha256"], label);
    safeBasename(entry.file, `${label}.file`);
    if (typeof entry.sha256 !== "string" || !SHA256.test(entry.sha256)) {
      fail("EVIDENCE_DIGEST_MISMATCH", `${label} has an invalid scenario digest`);
    }
    if (files.has(entry.file)) {
      fail("EVIDENCE_DUPLICATE_RECORD", "freeze manifest repeats a scenario entry");
    }
    files.add(entry.file);
    // The historical freeze manifest predates an explicit id field. The
    // canonical corpus contract names each scenario file by its id stem; the
    // exact set/order comparison below binds that convention to the records.
    scenarioIds.push(entry.file.slice(0, -5));
  }
  if (new Set(scenarioIds).size !== scenarioIds.length) {
    fail("EVIDENCE_DUPLICATE_RECORD", "freeze manifest repeats a scenario id");
  }
  let digest;
  try {
    digest = computeCorpusDigest(manifest);
  } catch {
    fail("EVIDENCE_MALFORMED", "freeze manifest could not be canonicalized");
  }
  if (digest !== manifest.corpusSha256) {
    fail("EVIDENCE_DIGEST_MISMATCH", "freeze manifest does not match its declared corpus digest");
  }
  return scenarioIds;
}

/** Validate the exact shape of FREEZE.json without exposing its prose values. */
export function validateFreezeDocument(document) {
  exactKeys(document, FREEZE_KEYS, "FREEZE.json");
  if (typeof document.freezeCommit !== "string" || !COMMIT_ID.test(document.freezeCommit)) {
    fail("EVIDENCE_MALFORMED", "FREEZE.json has an invalid freeze commit");
  }
  if (!Number.isSafeInteger(document.corpusVersion) || document.corpusVersion < 1) {
    fail("EVIDENCE_VERSION_MISMATCH", "FREEZE.json has an invalid corpus version");
  }
  for (const field of ["corpusSha256", "referenceRecordsSha256", "candidateRecordsSha256"]) {
    if (typeof document[field] !== "string" || !SHA256.test(document[field])) {
      fail("EVIDENCE_DIGEST_MISMATCH", `FREEZE.json has an invalid ${field}`);
    }
  }
  declaredFilename(document.audit, "audit.json", "FREEZE.json.audit");
  boundedString(document.note, FREEZE_TEXT_MAX_BYTES, "FREEZE.json.note");
  declaredFilename(document.pinnedOracle, "oracle.json", "FREEZE.json.pinnedOracle");
  declaredFilename(document.pinnedCandidate, "candidate.json", "FREEZE.json.pinnedCandidate");
  declaredFilename(document.manifest, "manifest.json", "FREEZE.json.manifest");
  return document;
}

/**
 * Load and verify the complete historical evidence bundle beside a pinned
 * oracle. The returned provenance contains only digests, ids, and counts.
 */
export function loadPinnedEvidence(pinnedOraclePath, currentScenarios = null) {
  const oraclePath = resolve(pinnedOraclePath);
  const directory = dirname(oraclePath);
  const freezePath = join(directory, "FREEZE.json");
  const freezeDocument = readJson(freezePath, "FREEZE.json", FREEZE_MAX_BYTES);
  const freeze = validateFreezeDocument(freezeDocument.value);
  const oracleFilename = declaredFilename(
    freeze.pinnedOracle,
    "oracle.json",
    "FREEZE.json.pinnedOracle",
  );
  const candidateFilename = declaredFilename(
    freeze.pinnedCandidate,
    "candidate.json",
    "FREEZE.json.pinnedCandidate",
  );
  const manifestFilename = declaredFilename(
    freeze.manifest,
    "manifest.json",
    "FREEZE.json.manifest",
  );
  if (basename(oraclePath) !== oracleFilename) {
    fail("EVIDENCE_MALFORMED", "pinned oracle does not match FREEZE.json.pinnedOracle");
  }

  const manifestDocument = readJson(
    join(directory, manifestFilename),
    "freeze manifest",
    FREEZE_MAX_BYTES,
  );
  const scenarioIds = validateFreezeManifest(manifestDocument.value, freeze.corpusVersion);
  if (freeze.corpusSha256 !== manifestDocument.value.corpusSha256) {
    fail("EVIDENCE_DIGEST_MISMATCH", "FREEZE.json and its manifest disagree on the corpus digest");
  }

  const oracleDocument = readJson(
    join(directory, oracleFilename),
    "pinned oracle",
    CONTRACT_LIMITS.recordsBytes,
  );
  const candidateDocument = readJson(
    join(directory, candidateFilename),
    "pinned candidate",
    CONTRACT_LIMITS.recordsBytes,
  );
  const oracleRecords = validateRecordDocument(
    oracleDocument.text,
    "pinned oracle",
    "pinned oracle",
  );
  const candidateRecords = validateRecordDocument(
    candidateDocument.text,
    "pinned candidate",
    "pinned candidate",
  );
  const requiredIds = scenarioIds;
  validateRawRecordSet(oracleRecords, "pinned oracle", { requiredIds, orderedIds: requiredIds });
  validateRawRecordSet(candidateRecords, "pinned candidate", {
    requiredIds,
    orderedIds: requiredIds,
  });
  if (currentScenarios !== null) {
    const scenarioById = new Map(currentScenarios.map((scenario) => [scenario.id, scenario]));
    validateRawRecordSet(oracleRecords, "pinned oracle", { scenarioById });
  }

  const oracleRecordsSha256 = sha256Hex(canonicalRecordDocument(oracleRecords));
  const candidateRecordsSha256 = sha256Hex(canonicalRecordDocument(candidateRecords));
  if (oracleRecordsSha256 !== freeze.referenceRecordsSha256) {
    fail("EVIDENCE_DIGEST_MISMATCH", "pinned oracle does not match FREEZE.json reference digest");
  }
  if (candidateRecordsSha256 !== freeze.candidateRecordsSha256) {
    fail(
      "EVIDENCE_DIGEST_MISMATCH",
      "pinned candidate does not match FREEZE.json candidate digest",
    );
  }

  const auditPath = join(directory, "audit.json");
  const auditDocument = readJson(auditPath, "freeze audit", FREEZE_MAX_BYTES);
  const audit = auditDocument.value;
  if (
    !isObject(audit) ||
    audit.schemaVersion !== 3 ||
    audit.corpusVersion !== freeze.corpusVersion ||
    audit.corpusDigest !== freeze.corpusSha256 ||
    audit.referenceRecordsSha256 !== oracleRecordsSha256 ||
    audit.candidateRecordsSha256 !== candidateRecordsSha256 ||
    audit.referenceIdentity?.commit !== freeze.freezeCommit ||
    audit.referenceIdentity?.implementation !== "typescript-reference" ||
    audit.candidateIdentity?.commit !== freeze.freezeCommit ||
    audit.candidateIdentity?.implementation !== "rust-candidate" ||
    audit.parityHeld !== true ||
    audit.deviationCount !== 0
  ) {
    fail("EVIDENCE_AUDIT_MISMATCH", "freeze audit does not bind the committed freeze evidence");
  }

  return {
    oracleRecords,
    candidateRecords,
    scenarioIds,
    freeze,
    freezeSha256: sha256Hex(freezeDocument.text),
    provenance: {
      freezeSha256: sha256Hex(freezeDocument.text),
      freezeCommit: freeze.freezeCommit,
      corpusVersion: freeze.corpusVersion,
      corpusSha256: freeze.corpusSha256,
      frozenScenarioCount: scenarioIds.length,
      frozenScenarioIdsSha256: sha256Hex(canonicalizeJson(scenarioIds)),
      oracleRecordsSha256,
      candidateRecordsSha256,
      auditSha256: sha256Hex(auditDocument.text),
    },
  };
}
