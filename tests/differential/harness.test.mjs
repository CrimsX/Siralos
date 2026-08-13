/**
 * Differential harness self-tests (ADR 0033): corpus integrity, oracle
 * determinism, and comparator semantics (parity, deviation detection,
 * informational classification, incomplete runs, malformed records).
 */
import { cpSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, describe, expect, it } from "vitest";
import { canonicalizeJson, sha256Hex } from "./shared/canonical.mjs";
import { CONTRACT_LIMITS } from "./shared/contract.mjs";
import { loadCorpus, runOracle, runStateDirProbe } from "./run-oracle.mjs";
import {
  collectSourceIdentity,
  decodeGitPathList,
  parseCanonicalRecords,
  runCompare,
} from "./compare.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CORPUS = join(HERE, "corpus");
const ROOT = join(HERE, "..", "..");
const tempDirectories = [];

function tempDirectory(prefix) {
  const directory = mkdtempSync(join(tmpdir(), prefix));
  tempDirectories.push(directory);
  return directory;
}

function mutableCorpus() {
  const directory = tempDirectory("siralos-corpus-");
  cpSync(CORPUS, directory, { recursive: true });
  return directory;
}

function mutateJson(path, mutate) {
  const value = JSON.parse(readFileSync(path, "utf8"));
  mutate(value);
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}

afterAll(() => {
  for (const directory of tempDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

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
      'quote"backslash\\unicode\u00e9',
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
    expect(manifest.corpusVersion).toBe(3);
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
    const { scenarios, corpusDigest } = loadCorpus(CORPUS);
    const applicable = scenarios.filter((scenario) => scenario.applicable);
    expect(applicable.length).toBeGreaterThanOrEqual(1);
    expect(corpusDigest).toMatch(/^[0-9a-f]{64}$/u);
  });

  it("covers every platform contract value", () => {
    const { scenarios } = loadCorpus(CORPUS);
    const platforms = new Set(scenarios.flatMap((scenario) => scenario.platforms));
    expect(platforms.has("*") || platforms.has("windows") || platforms.has("posix")).toBe(true);
  });

  it("classifies OS-account fallbacks as informational instead of deterministic fixtures", () => {
    const { scenarios } = loadCorpus(CORPUS);
    for (const id of [
      "state-dir.fallback.posix",
      "state-dir.unset.windows",
      "state-dir.unset.posix",
    ]) {
      expect(scenarios.find((scenario) => scenario.id === id)?.parity).toBe("informational");
    }
  });

  it("rejects a scenario whose canonical digest does not match the manifest", () => {
    const corpus = mutableCorpus();
    mutateJson(join(corpus, "state-dir.set.posix.json"), (scenario) => {
      scenario.env.HOME = "/tampered";
    });
    expect(() => loadCorpus(corpus, "posix")).toThrow(/does not match its manifest digest/u);
  });

  it("rejects unsupported corpus and schema versions and unknown manifest fields", () => {
    for (const [field, value, expected] of [
      ["schemaVersion", 2, /unsupported corpus schemaVersion/u],
      ["corpusVersion", 4, /unsupported corpusVersion/u],
      ["unexpected", true, /unknown or missing fields/u],
    ]) {
      const corpus = mutableCorpus();
      mutateJson(join(corpus, "manifest.json"), (manifest) => {
        manifest[field] = value;
      });
      expect(() => loadCorpus(corpus, "posix")).toThrow(expected);
    }
  });

  it("rejects traversal, duplicate files, and id-to-file mismatches", () => {
    const traversal = mutableCorpus();
    mutateJson(join(traversal, "manifest.json"), (manifest) => {
      manifest.scenarios[0].file = "../outside.json";
    });
    expect(() => loadCorpus(traversal, "posix")).toThrow(/canonical JSON file name/u);

    const duplicate = mutableCorpus();
    mutateJson(join(duplicate, "manifest.json"), (manifest) => {
      manifest.scenarios[1] = structuredClone(manifest.scenarios[0]);
    });
    expect(() => loadCorpus(duplicate, "posix")).toThrow(/duplicate file/u);

    const mismatch = mutableCorpus();
    mutateJson(join(mismatch, "state-dir.set.posix.json"), (scenario) => {
      scenario.id = "state-dir.other.posix";
    });
    expect(() => loadCorpus(mismatch, "posix")).toThrow(/id must match its file name/u);
  });

  it("rejects a symlinked corpus root", () => {
    const corpus = mutableCorpus();
    const parent = tempDirectory("siralos-corpus-link-");
    const alias = join(parent, "alias");
    symlinkSync(corpus, alias, process.platform === "win32" ? "junction" : "dir");
    expect(() => loadCorpus(alias, "posix")).toThrow(/may not be a symlink/u);
  });

  it("rejects unknown scenario fields and unsupported environment authority", () => {
    const unknown = mutableCorpus();
    mutateJson(join(unknown, "state-dir.set.posix.json"), (scenario) => {
      scenario.expected = "not-an-input";
    });
    expect(() => loadCorpus(unknown, "posix")).toThrow(/unknown or missing fields/u);

    const environment = mutableCorpus();
    mutateJson(join(environment, "state-dir.set.posix.json"), (scenario) => {
      scenario.env.NODE_OPTIONS = "--require=untrusted.js";
    });
    expect(() => loadCorpus(environment, "posix")).toThrow(/unsupported key/u);
  });

  it("rejects required parity backed by an OS-owned home fallback", () => {
    const corpus = mutableCorpus();
    mutateJson(join(corpus, "state-dir.set.posix.json"), (scenario) => {
      scenario.env.HOME = "";
    });
    expect(() => loadCorpus(corpus, "posix")).toThrow(/must fully declare/u);
  });

  it("rejects over-limit scenario files before parsing", () => {
    const corpus = mutableCorpus();
    writeFileSync(
      join(corpus, "state-dir.set.posix.json"),
      " ".repeat(CONTRACT_LIMITS.scenarioBytes + 1),
      "utf8",
    );
    expect(() => loadCorpus(corpus, "posix")).toThrow(/exceeds .* bytes/u);
  });
});

describe("oracle determinism", () => {
  it("produces byte-identical records on consecutive runs", () => {
    const first = runOracle(CORPUS, ROOT);
    const second = runOracle(CORPUS, ROOT);
    expect(first).toBe(second);
  });

  it("emits only canonical sorted-key JSON", () => {
    const text = runOracle(CORPUS, ROOT);
    const records = parseCanonicalRecords(text, "oracle");
    expect(text).toBe(`${canonicalizeJson(records)}\n`);
  });

  it("treats probe timeout, nonzero exit, and empty output as harness errors", () => {
    const directory = tempDirectory("siralos-probes-");
    const timeout = join(directory, "timeout.mjs");
    const nonzero = join(directory, "nonzero.mjs");
    const empty = join(directory, "empty.mjs");
    writeFileSync(timeout, "setTimeout(() => {}, 10_000);\n", "utf8");
    writeFileSync(nonzero, "process.exit(7);\n", "utf8");
    writeFileSync(empty, "", "utf8");
    expect(() => runStateDirProbe({}, { probe: timeout, timeoutMs: 100 })).toThrow(/timed out/u);
    expect(() => runStateDirProbe({}, { probe: nonzero })).toThrow(/exited unsuccessfully/u);
    expect(() => runStateDirProbe({}, { probe: empty })).toThrow(/emitted no outcome/u);
  });

  it("refuses undeclared environment keys before spawning a probe", () => {
    expect(() => runStateDirProbe({ NODE_OPTIONS: "--require=untrusted.js" })).toThrow(
      /unsupported key/u,
    );
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

  it("rejects missing, extra, duplicate, and out-of-order records", () => {
    const records = [record("a", "a".repeat(64)), record("b", "b".repeat(64))];
    for (const candidateRecords of [
      [record("b", "b".repeat(64))],
      [...records, record("unexpected", "c".repeat(64))],
      [records[0], records[0]],
      [records[1], records[0]],
    ]) {
      expect(() =>
        runCompare({
          oracleRecords: records,
          candidateRecords,
          scenarios,
          platform: POSIX,
        }),
      ).toThrow(/record set/u);
    }
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
        oracleRecords: [{ scenarioId: 42 }, record("b", "b".repeat(64))],
        candidateRecords: [record("a", "a".repeat(64)), record("b", "b".repeat(64))],
        scenarios,
        platform: POSIX,
      }),
    ).toThrow(/malformed oracle record/);
  });

  it("rejects subject mismatches and subject-specific malformed fields", () => {
    const valid = [record("a", "a".repeat(64)), record("b", "b".repeat(64))];
    const wrongSubject = structuredClone(valid);
    wrongSubject[0] = {
      scenarioId: "a",
      subject: "version-identity",
      kind: "ok",
      version: "0.0.0",
    };
    expect(() =>
      runCompare({
        oracleRecords: valid,
        candidateRecords: wrongSubject,
        scenarios,
        platform: POSIX,
      }),
    ).toThrow(/subject mismatch/u);

    const missingHash = structuredClone(valid);
    delete missingHash[0].stateDirSha256;
    expect(() =>
      runCompare({
        oracleRecords: missingHash,
        candidateRecords: valid,
        scenarios,
        platform: POSIX,
      }),
    ).toThrow(/unknown or missing fields/u);

    const extraField = structuredClone(valid);
    extraField[0].unexpected = true;
    expect(() =>
      runCompare({
        oracleRecords: extraField,
        candidateRecords: valid,
        scenarios,
        platform: POSIX,
      }),
    ).toThrow(/unknown or missing fields/u);
  });

  it("emits per-subject and per-scenario coverage", () => {
    const records = [record("a", "a".repeat(64)), record("b", "b".repeat(64))];
    const { audit } = runCompare({
      oracleRecords: records,
      candidateRecords: structuredClone(records),
      scenarios,
      platform: POSIX,
      corpusVersion: 3,
      corpusDigest: "c".repeat(64),
      sourceIdentity: { commit: "d".repeat(40) },
    });
    expect(audit.schemaVersion).toBe(2);
    expect(audit.perSubject["state-dir"]).toMatchObject({
      total: 3,
      applicable: 2,
      parity: 1,
      informationalParity: 1,
      skippedPlatform: 1,
    });
    expect(audit.scenarios.map((entry) => entry.status)).toEqual([
      "parity",
      "informational-parity",
      "skipped-platform",
    ]);
  });

  it("rejects record files that are not exact canonical bytes", () => {
    const records = [record("a", "a".repeat(64))];
    const canonical = `${canonicalizeJson(records)}\n`;
    expect(parseCanonicalRecords(canonical, "oracle")).toEqual(records);
    expect(() => parseCanonicalRecords(canonical.trim(), "oracle")).toThrow(/not exact canonical/u);
    expect(() => parseCanonicalRecords(`${JSON.stringify(records, null, 2)}\n`, "oracle")).toThrow(
      /not exact canonical/u,
    );
  });

  it("binds audit provenance to commit and directly hashed source bytes", () => {
    const identity = collectSourceIdentity(ROOT);
    expect(Object.keys(identity).sort()).toEqual(["commit", "sourceFiles", "sourceTreeSha256"]);
    expect(identity.commit).toMatch(/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/u);
    expect(identity.sourceTreeSha256).toMatch(/^[0-9a-f]{64}$/u);
    expect(identity.sourceFiles).toBeGreaterThan(0);
  });

  it("rejects non-UTF-8 and truncated Git path inventories", () => {
    expect(() => decodeGitPathList(Buffer.from([0xff, 0x00]))).toThrow(/not valid UTF-8/u);
    expect(() => decodeGitPathList(Buffer.from("tracked.txt", "utf8"))).toThrow(/truncated/u);
    expect(decodeGitPathList(Buffer.from("b\0a\0", "utf8"))).toEqual(["a", "b"]);
  });
});
