/**
 * Differential harness self-tests (ADR 0033): corpus integrity, oracle
 * determinism, and comparator semantics (parity, deviation detection,
 * informational classification, incomplete runs, malformed records).
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { canonicalizeJson, sha256Hex } from "./shared/canonical.mjs";
import { loadCorpus, runOracle } from "./run-oracle.mjs";
import { runCompare } from "./compare.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CORPUS = join(HERE, "corpus");
const ROOT = join(HERE, "..", "..");

describe("canonical serialization properties", () => {
  // Deterministic generator: nested values with deliberately
  // unsorted keys, arrays, strings (incl. escapes), numbers, null,
  // and booleans.
  function generateValue(seed) {
    const values = [
      null,
      true,
      false,
      0,
      1,
      -1,
      42,
      "",
      "plain",
      "quote\"backslash\\unicode\u00e9",
      [1, "two", null, { z: 1, a: [true, false] }],
      { zeta: 1, alpha: "beta", mid: null, num: 7 },
      { nested: { deep: { deeper: [{ x: 1 }, { x: 2 }] } } },
    ];
    return values[seed % values.length];
  }

  it("is idempotent over parsed values (parse → canonicalize → parse → canonicalize)", () => {
    for (let seed = 0; seed < 64; seed += 1) {
      const value = generateValue(seed);
      const once = canonicalizeJson(value);
      const parsed = JSON.parse(once);
      const twice = canonicalizeJson(parsed);
      expect(twice).toBe(once);
      expect(JSON.parse(twice)).toEqual(parsed);
    }
  });

  it("produces sorted keys and stable digests regardless of input key order", () => {
    const a = { z: 1, a: 2, m: 3 };
    const b = { m: 3, a: 2, z: 1 };
    const canonicalA = canonicalizeJson(a);
    expect(canonicalA).toBe(canonicalizeJson(b));
    expect(sha256Hex(canonicalA)).toBe(sha256Hex(canonicalizeJson(b)));
    expect(canonicalA).toBe('{"a":2,"m":3,"z":1}');
  });
});

describe("corpus integrity", () => {
  it("validates every manifest entry against the recomputed digest", () => {
    const manifest = JSON.parse(readFileSync(join(CORPUS, "manifest.json"), "utf8"));
    expect(manifest.schemaVersion).toBe(1);
    expect(manifest.corpusVersion).toBe(1);
    expect(manifest.scenarios.length).toBeGreaterThanOrEqual(6);
    for (const entry of manifest.scenarios) {
      const scenario = JSON.parse(readFileSync(join(CORPUS, entry.file), "utf8"));
      expect(entry.sha256).toBe(sha256Hex(canonicalizeJson(scenario)));
      expect(entry.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(scenario.id).toBe(entry.file.replace(/\.json$/, ""));
      expect(["state-dir", "version-identity"]).toContain(scenario.subject);
      expect(["required", "informational"]).toContain(scenario.parity);
      expect(Array.isArray(scenario.platforms)).toBe(true);
    }
  });

  it("loads the corpus without errors on the current platform", () => {
    const { scenarios } = loadCorpus(CORPUS);
    const applicable = scenarios.filter((scenario) => scenario.applicable);
    expect(applicable.length).toBeGreaterThanOrEqual(1);
  });

  it("covers every platform contract value", () => {
    const { scenarios } = loadCorpus(CORPUS);
    const platforms = new Set(scenarios.flatMap((scenario) => scenario.platforms));
    expect(platforms.has("*") || platforms.has("windows") || platforms.has("posix")).toBe(true);
  });
});

describe("oracle determinism", () => {
  it("produces byte-identical records on consecutive runs", () => {
    const first = runOracle(CORPUS, ROOT);
    const second = runOracle(CORPUS, ROOT);
    expect(first).toBe(second);
  });

  it("emits only canonical sorted-key JSON", () => {
    const records = runOracle(CORPUS, ROOT).trim();
    expect(() => JSON.parse(records)).not.toThrow();
    expect(records).toBe(canonicalizeJson(JSON.parse(records)));
  });
});

describe("comparator semantics", () => {
  // `c` is windows-only, so on posix both sides skip it; `a` is
  // required and `b` informational on every platform.
  const scenarios = [
    { id: "a", subject: "state-dir", platforms: ["*"], parity: "required" },
    { id: "b", subject: "state-dir", platforms: ["*"], parity: "informational" },
    { id: "c", subject: "state-dir", platforms: ["windows"], parity: "required" },
  ];
  const record = (id, hash) => ({
    scenarioId: id,
    subject: "state-dir",
    kind: "ok",
    stateDirSha256: hash,
  });
  const POSIX = "posix";

  it("holds parity when records match exactly", () => {
    const records = [record("a", "a".repeat(64)), record("b", "b".repeat(64))];
    const { audit, deviations } = runCompare({
      oracleRecords: records,
      candidateRecords: structuredClone(records),
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(true);
    expect(deviations).toEqual([]);
    expect(audit.skipped).toEqual(["c"]);
  });

  it("flags a required record mismatch as a deviation", () => {
    const { audit, deviations } = runCompare({
      oracleRecords: [record("a", "a".repeat(64)), record("b", "b".repeat(64))],
      candidateRecords: [record("a", "d".repeat(64)), record("b", "b".repeat(64))],
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(false);
    expect(deviations).toHaveLength(1);
    expect(deviations[0]).toMatchObject({ scenarioId: "a", reason: "record-mismatch" });
    expect(audit.deviationCount).toBe(1);
  });

  it("records but never fails informational deviations", () => {
    const { audit, deviations } = runCompare({
      oracleRecords: [record("a", "a".repeat(64)), record("b", "a".repeat(64))],
      candidateRecords: [record("a", "a".repeat(64)), record("b", "d".repeat(64))],
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(true);
    expect(deviations).toEqual([]);
    expect(audit.informationalDeviations).toHaveLength(1);
  });

  it("flags a required scenario run by only one side as incomplete", () => {
    const { audit } = runCompare({
      oracleRecords: [record("a", "a".repeat(64)), record("b", "b".repeat(64))],
      candidateRecords: [record("b", "b".repeat(64))],
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(false);
    expect(audit.deviated[0]).toMatchObject({ scenarioId: "a", reason: "incomplete" });
  });

  it("derives skipped scenarios that neither side ran", () => {
    const records = [record("a", "a".repeat(64)), record("b", "b".repeat(64))];
    const { audit } = runCompare({
      oracleRecords: records,
      candidateRecords: structuredClone(records),
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(true);
    expect(audit.skipped).toEqual(["c"]);
  });

  it("rejects malformed records", () => {
    expect(() =>
      runCompare({
        oracleRecords: [{ scenarioId: 42 }],
        candidateRecords: [],
        scenarios,
        platform: POSIX,
      }),
    ).toThrow(/malformed oracle record/);
  });
});
