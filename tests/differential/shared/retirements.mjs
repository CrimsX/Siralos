/**
 * Digest-bound retired-subjects list (ADR 0037 §7 option A).
 *
 * A retirement ends execution and matching for one pinned scenario id
 * whose record stays byte-identical in the frozen oracle: the scenario
 * file left the corpus (its subject is gone), so there is nothing to run
 * and nothing to compare, but the trust anchor must still name exactly
 * which bytes authorised the absence. It mirrors the supersession list
 * key-for-key discipline with one inverted membership rule: a retired id
 * must be present in the PINNED ORACLE and ABSENT from the current
 * corpus. A scenario that still exists is a supersession case, and
 * retiring a bare subject name instead of a scenario id, is refused.
 *
 * Two phases, because the pinned-evidence loader needs the retired ids
 * before it can exempt them from the corpus cross-check while the digest
 * verification needs the oracle records the loader returns:
 *   1. parseRetirementsDocument — structural checks only (keys, schema
 *      and corpus versions, bounds, self-digest, entry shapes, stamps,
 *      duplicates). No corpus or oracle needed.
 *   2. verifyRetirementEntries — membership (oracle yes, corpus no) and
 *      digest checks against the loaded oracle records and scenarios.
 */
import { canonicalizeJson, sha256Hex } from "./canonical.mjs";
import { canonicalRecordDocument } from "./protocol.mjs";

const SHA256 = /^[0-9a-f]{64}$/u;
const DOCUMENT_KEYS = ["schemaVersion", "corpusVersion", "entriesSha256", "retirements"];
const ENTRY_KEYS = [
  "scenarioId",
  "reason",
  "retiredIn",
  "decision",
  "oracleRecordSha256",
  // Disclosure-only: nothing can recheck a record that no longer
  // executes. Kept for provenance — the digest binds the last
  // executed candidate output named in the entry's decision.
  "candidateRecordSha256",
];
const MAX_ENTRIES = 64;
const MAX_PROSE_BYTES = 512;

function refusal(code, message) {
  const error = new Error(message);
  error.code = code;
  throw error;
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/**
 * Parse and structurally validate a retirements document. Returns the
 * entries, the retired id set, and both document digests. Throws an
 * Error with a RETIREMENTS_* code on any refusal.
 */
export function parseRetirementsDocument(text, corpusVersion) {
  let document;
  try {
    document = JSON.parse(text);
  } catch {
    refusal("RETIREMENTS_MALFORMED", "retirements is not valid JSON");
  }
  if (!isObject(document)) {
    refusal("RETIREMENTS_MALFORMED", "retirements must be a JSON object");
  }
  const actualKeys = Object.keys(document).sort();
  if (actualKeys.join(",") !== [...DOCUMENT_KEYS].sort().join(",")) {
    refusal(
      "RETIREMENTS_MALFORMED",
      `retirements must carry exactly ${DOCUMENT_KEYS.join(", ")}; found ${actualKeys.join(", ")}`,
    );
  }
  if (document.schemaVersion !== 1) {
    refusal(
      "RETIREMENTS_MALFORMED",
      `unsupported retirements schemaVersion ${JSON.stringify(document.schemaVersion)}`,
    );
  }
  if (document.corpusVersion !== corpusVersion) {
    refusal(
      "RETIREMENTS_CORPUS_MISMATCH",
      `retirements declare corpus version ${JSON.stringify(document.corpusVersion)} but the corpus is v${corpusVersion}`,
    );
  }
  if (!Array.isArray(document.retirements) || document.retirements.length > MAX_ENTRIES) {
    refusal("RETIREMENTS_MALFORMED", "retirements must be an array of at most 64 entries");
  }
  const entriesSha256 = sha256Hex(canonicalizeJson(document.retirements));
  if (document.entriesSha256 !== entriesSha256) {
    refusal(
      "RETIREMENTS_SELF_DIGEST_MISMATCH",
      `retirements declare entriesSha256 ${JSON.stringify(document.entriesSha256)} but their entries hash to ${entriesSha256}; re-stamp the document after editing an entry`,
    );
  }
  const byId = new Map();
  for (const entry of document.retirements) {
    if (!isObject(entry)) {
      refusal("RETIREMENTS_MALFORMED", "each retirement must be an object");
    }
    const keys = Object.keys(entry).sort();
    if (keys.join(",") !== [...ENTRY_KEYS].sort().join(",")) {
      const label = typeof entry.scenarioId === "string" ? entry.scenarioId : "<retirements>";
      refusal(
        "RETIREMENTS_MALFORMED",
        `retirement ${label} must carry exactly ${ENTRY_KEYS.join(", ")}; found ${keys.join(", ")}`,
      );
    }
    const { scenarioId } = entry;
    if (typeof scenarioId !== "string" || scenarioId.length === 0) {
      refusal("RETIREMENTS_MALFORMED", "a retirement needs a scenario id");
    }
    if (byId.has(scenarioId)) {
      refusal("RETIREMENTS_DUPLICATE", `scenario ${scenarioId} is retired more than once`);
    }
    for (const field of ["reason", "decision"]) {
      if (typeof entry[field] !== "string" || entry[field].trim().length === 0) {
        refusal("RETIREMENTS_MALFORMED", `retirement ${scenarioId} needs a non-empty ${field}`);
      }
      if (entry[field].length > MAX_PROSE_BYTES) {
        refusal(
          "RETIREMENTS_MALFORMED",
          `retirement ${scenarioId} ${field} exceeds 512 characters`,
        );
      }
    }
    if (entry.retiredIn !== corpusVersion) {
      refusal(
        "RETIREMENTS_CORPUS_MISMATCH",
        `retirement ${scenarioId} is stamped for corpus version ${JSON.stringify(entry.retiredIn)}, not v${corpusVersion}`,
      );
    }
    for (const digest of ["oracleRecordSha256", "candidateRecordSha256"]) {
      if (typeof entry[digest] !== "string" || !SHA256.test(entry[digest])) {
        refusal("RETIREMENTS_MALFORMED", `retirement ${scenarioId} carries an invalid ${digest}`);
      }
    }
    byId.set(scenarioId, entry);
  }
  return {
    entries: document.retirements,
    retiredIds: new Set(byId.keys()),
    documentSha256: sha256Hex(text),
    entriesSha256,
  };
}

/**
 * Verify parsed retirement entries against the current scenarios and
 * the pinned oracle records. Returns a Map from scenario id to the
 * disclosure fields the audit prints. Throws an Error with a
 * RETIREMENTS_* code on any refusal.
 */
export function verifyRetirementEntries(entries, { scenarios, oracleRecords }) {
  const scenarioById = new Map(scenarios.map((scenario) => [scenario.id, scenario]));
  const oracleById = new Map(oracleRecords.map((record) => [record.scenarioId, record]));
  const verified = new Map();
  for (const entry of entries) {
    const { scenarioId } = entry;
    if (scenarioById.has(scenarioId)) {
      refusal(
        "RETIREMENTS_STILL_PRESENT",
        `retirement names scenario ${scenarioId}, which the corpus still defines: a scenario that still exists is a supersession case, never a retirement`,
      );
    }
    const oracleRecord = oracleById.get(scenarioId);
    if (oracleRecord === undefined) {
      refusal(
        "RETIREMENTS_UNKNOWN_SCENARIO",
        `retirement names scenario ${scenarioId}, which the pinned oracle does not cover: retirements name exact scenario ids, never subjects or unknown ids`,
      );
    }
    const frozenSha256 = sha256Hex(canonicalRecordDocument([oracleRecord]));
    if (entry.oracleRecordSha256 !== frozenSha256) {
      refusal(
        "RETIREMENTS_ORACLE_MISMATCH",
        `retirement ${scenarioId} claims frozen record ${JSON.stringify(entry.oracleRecordSha256)} but the pinned oracle record hashes to ${frozenSha256}`,
      );
    }
    verified.set(scenarioId, { entry, oracleRecord });
  }
  return verified;
}
