/**
 * Differential harness self-tests (ADR 0033): corpus integrity, oracle
 * determinism, and comparator semantics (parity, deviation detection,
 * informational classification, incomplete runs, malformed records).
 */
import { cpSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, describe, expect, it } from "vitest";
import { canonicalizeJson, sha256Hex } from "./shared/canonical.mjs";
import { CONTRACT_LIMITS, computeCorpusDigest } from "./shared/contract.mjs";
import { SCENARIO_OUTCOME, canonicalRecordDocument } from "./shared/protocol.mjs";
import { superviseRunner } from "./shared/runner-process.mjs";
import { loadCorpus, runOracle, runStateDirProbe } from "./run-oracle.mjs";
import {
  collectSourceIdentity,
  decodeGitPathList,
  parseCanonicalRecords,
  runCompare,
  semanticDifferences,
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

describe("module entry guards", () => {
  it("allows the comparator API to be imported without an argv entry", () => {
    const result = spawnSync(
      process.execPath,
      ["--input-type=module", "--eval", "await import('./tests/differential/compare.mjs')"],
      {
        cwd: ROOT,
        encoding: "utf8",
        maxBuffer: 16 * 1024,
        timeout: 10_000,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );

    expect(result.error).toBeUndefined();
    expect(result.signal).toBeNull();
    expect(result.status).toBe(0);
    expect(result.stderr).toBe("");
  });
});

describe("corpus integrity", () => {
  it("validates every manifest entry against the recomputed digest", () => {
    const manifest = JSON.parse(readFileSync(join(CORPUS, "manifest.json"), "utf8"));
    expect(manifest.schemaVersion).toBe(3);
    expect(manifest.corpusVersion).toBe(5);
    expect(manifest.corpusSha256).toBe(computeCorpusDigest(manifest));
    expect(manifest.scenarios.length).toBeGreaterThanOrEqual(6);
    for (const entry of manifest.scenarios) {
      const scenario = JSON.parse(readFileSync(join(CORPUS, entry.file), "utf8"));
      expect(entry.sha256).toBe(sha256Hex(canonicalizeJson(scenario)));
      expect(entry.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(scenario.id).toBe(entry.file.replace(/\.json$/, ""));
      expect(["state-dir", "version-identity", "task-contract"]).toContain(scenario.subject);
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

  it("distinguishes missing, malformed, and mismatched fixture digests", () => {
    const missingCorpus = mutableCorpus();
    mutateJson(join(missingCorpus, "manifest.json"), (manifest) => {
      delete manifest.corpusSha256;
    });
    expect(() => loadCorpus(missingCorpus, "posix")).toThrow(/corpusSha256 is required/u);

    const malformedCorpus = mutableCorpus();
    mutateJson(join(malformedCorpus, "manifest.json"), (manifest) => {
      manifest.corpusSha256 = "not-a-digest";
    });
    expect(() => loadCorpus(malformedCorpus, "posix")).toThrow(/lowercase SHA-256/u);

    const mismatchedCorpus = mutableCorpus();
    mutateJson(join(mismatchedCorpus, "manifest.json"), (manifest) => {
      manifest.corpusSha256 = "0".repeat(64);
    });
    expect(() => loadCorpus(mismatchedCorpus, "posix")).toThrow(/does not match corpusSha256/u);

    const missingScenario = mutableCorpus();
    mutateJson(join(missingScenario, "manifest.json"), (manifest) => {
      delete manifest.scenarios[0].sha256;
    });
    expect(() => loadCorpus(missingScenario, "posix")).toThrow(/sha256 is required/u);

    const malformedScenario = mutableCorpus();
    mutateJson(join(malformedScenario, "manifest.json"), (manifest) => {
      manifest.scenarios[0].sha256 = "invalid";
    });
    expect(() => loadCorpus(malformedScenario, "posix")).toThrow(/lowercase SHA-256/u);
  });

  it("rejects unsupported corpus and schema versions and unknown manifest fields", () => {
    for (const [field, value, expected] of [
      ["schemaVersion", 4, /unsupported corpus schemaVersion/u],
      ["corpusVersion", 6, /unsupported corpusVersion/u],
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
  it("produces byte-identical records on consecutive runs", { timeout: 120_000 }, () => {
    const first = runOracle(CORPUS, ROOT);
    const second = runOracle(CORPUS, ROOT);
    expect(first).toBe(second);
  });

  it("emits only canonical sorted-key JSON", { timeout: 120_000 }, () => {
    const text = runOracle(CORPUS, ROOT);
    const records = parseCanonicalRecords(text, "oracle");
    expect(text).toBe(canonicalRecordDocument(records));
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

describe("runner lifecycle classification", () => {
  const crash = (implementation) =>
    superviseRunner({
      implementation,
      scenarioId: "fixture.crash",
      command: process.execPath,
      args: ["-e", "process.exit(7)"],
      cwd: ROOT,
    });
  const timeout = (implementation) =>
    superviseRunner({
      implementation,
      scenarioId: "fixture.timeout",
      command: process.execPath,
      args: ["-e", "setTimeout(() => {}, 10_000)"],
      cwd: ROOT,
      timeoutMs: 100,
    });

  it("classifies a reference process crash with fixture and implementation identity", async () => {
    await expect(crash("reference")).resolves.toMatchObject({
      outcome: "PROCESS_CRASHED",
      implementation: "reference",
      scenarioId: "fixture.crash",
      exitCode: 7,
    });
  });

  it("classifies a candidate process crash with fixture and implementation identity", async () => {
    await expect(crash("candidate")).resolves.toMatchObject({
      outcome: "PROCESS_CRASHED",
      implementation: "candidate",
      scenarioId: "fixture.crash",
      exitCode: 7,
    });
  });

  it("classifies and terminates a reference timeout", async () => {
    await expect(timeout("reference")).resolves.toMatchObject({
      outcome: "TIMED_OUT",
      implementation: "reference",
      scenarioId: "fixture.timeout",
    });
  });

  it("classifies and terminates a candidate timeout", async () => {
    await expect(timeout("candidate")).resolves.toMatchObject({
      outcome: "TIMED_OUT",
      implementation: "candidate",
      scenarioId: "fixture.timeout",
    });
  });

  it("keeps typed harness failure distinct from a process crash", async () => {
    const diagnostic = JSON.stringify({
      category: "CORPUS_INTEGRITY_FAILURE",
      code: "CONTENT_MISMATCH",
      message: "fixture digest mismatch",
    });
    await expect(
      superviseRunner({
        implementation: "reference",
        scenarioId: "fixture.corrupt",
        command: process.execPath,
        args: [
          "-e",
          `process.stderr.write(${JSON.stringify(`SIRALOS_HARNESS_ERROR ${diagnostic}\n`)}); process.exit(2)`,
        ],
        cwd: ROOT,
      }),
    ).resolves.toMatchObject({
      outcome: "HARNESS_ERROR",
      category: "CORPUS_INTEGRITY_FAILURE",
      code: "CONTENT_MISMATCH",
    });
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
    outcome: SCENARIO_OUTCOME.COMPLETED,
    result: { stateDirSha256: hash },
  });
  const unsupported = (id) => ({
    scenarioId: id,
    subject: "state-dir",
    outcome: SCENARIO_OUTCOME.UNSUPPORTED,
    error: { category: "PLATFORM_NOT_APPLICABLE" },
  });
  const records = (a = "a", b = "b") => [
    record("a", a.repeat(64)),
    record("b", b.repeat(64)),
    unsupported("c"),
  ];
  const POSIX = "posix";

  it("holds parity when records match exactly", () => {
    const matching = records();
    const { audit, deviations } = runCompare({
      oracleRecords: matching,
      candidateRecords: structuredClone(matching),
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(true);
    expect(deviations).toEqual([]);
    expect(audit.skipped).toEqual(["c"]);
  });

  it("flags a required record mismatch as a deviation", () => {
    const { audit, deviations } = runCompare({
      oracleRecords: records(),
      candidateRecords: records("d", "b"),
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(false);
    expect(deviations).toHaveLength(1);
    expect(deviations[0]).toMatchObject({ scenarioId: "a", reason: "record-mismatch" });
    expect(deviations[0].differences).toEqual([
      {
        path: "$.result.stateDirSha256",
        kind: "VALUE_CHANGED",
        oracle: "a".repeat(64),
        candidate: "d".repeat(64),
        policy: "scalar-exact",
      },
    ]);
    expect(audit.deviationCount).toBe(1);
  });

  it("reports scalar, missing, extra, ordered-sequence, and error-category differences", () => {
    expect(semanticDifferences({ value: 1 }, { value: 2 })[0]).toMatchObject({
      path: "$.value",
      kind: "VALUE_CHANGED",
    });
    expect(semanticDifferences({ value: 1 }, {})[0]).toMatchObject({
      path: "$.value",
      kind: "MISSING_IN_CANDIDATE",
    });
    expect(semanticDifferences({}, { value: 1 })[0]).toMatchObject({
      path: "$.value",
      kind: "EXTRA_IN_CANDIDATE",
    });
    expect(semanticDifferences({ events: ["a", "b"] }, { events: ["b", "a"] })).toEqual([
      {
        path: "$.events",
        kind: "ORDER_CHANGED",
        oracle: ["a", "b"],
        candidate: ["b", "a"],
        policy: "sequence-order-authoritative",
      },
    ]);
    expect(
      semanticDifferences(
        { error: { category: "PARSE_ERROR" } },
        { error: { category: "IO_ERROR" } },
      )[0],
    ).toMatchObject({ path: "$.error.category", kind: "ERROR_CATEGORY_CHANGED" });
    expect(semanticDifferences({ map: { b: 2, a: 1 } }, { map: { a: 1, b: 2 } })).toEqual([]);
  });

  it("keeps explicit UNIMPLEMENTED outcomes visible and gate-failing", () => {
    const unimplemented = (id) => ({
      scenarioId: id,
      subject: "state-dir",
      outcome: SCENARIO_OUTCOME.UNIMPLEMENTED,
      error: { category: "SUBSYSTEM_NOT_PORTED" },
    });
    const candidate = records();
    candidate[0] = unimplemented("a");
    const { audit } = runCompare({
      oracleRecords: records(),
      candidateRecords: candidate,
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(false);
    expect(audit.deviated[0]).toMatchObject({ scenarioId: "a", reason: "unimplemented" });
  });

  it("keeps matching product failures distinct from runner failures", () => {
    const productFailure = {
      scenarioId: "a",
      subject: "state-dir",
      outcome: SCENARIO_OUTCOME.PRODUCT_FAILURE,
      error: { category: "NO_HOME_DIRECTORY" },
    };
    const oracle = records();
    const candidate = records();
    oracle[0] = productFailure;
    candidate[0] = structuredClone(productFailure);

    const { audit } = runCompare({
      oracleRecords: oracle,
      candidateRecords: candidate,
      scenarios,
      platform: POSIX,
    });

    expect(audit.parityHeld).toBe(true);
    expect(audit.parity).toContain("a");
  });

  it("keeps applicable UNSUPPORTED outcomes visible and gate-failing", () => {
    const candidate = records();
    candidate[0] = {
      scenarioId: "a",
      subject: "state-dir",
      outcome: SCENARIO_OUTCOME.UNSUPPORTED,
      error: { category: "SUBSYSTEM_UNSUPPORTED" },
    };

    const { audit } = runCompare({
      oracleRecords: records(),
      candidateRecords: candidate,
      scenarios,
      platform: POSIX,
    });

    expect(audit.parityHeld).toBe(false);
    expect(audit.deviated[0]).toMatchObject({
      scenarioId: "a",
      reason: "unsupported-applicable-scenario",
    });
  });

  it("records but never fails informational deviations", () => {
    const { audit, deviations } = runCompare({
      oracleRecords: records("a", "a"),
      candidateRecords: records("a", "d"),
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(true);
    expect(deviations).toEqual([]);
    expect(audit.informationalDeviations).toHaveLength(1);
  });

  it("rejects missing, extra, duplicate, and out-of-order records", () => {
    const matching = records();
    for (const candidateRecords of [
      matching.slice(1),
      [...matching, record("unexpected", "c".repeat(64))],
      [matching[0], matching[0], matching[2]],
      [matching[1], matching[0], matching[2]],
    ]) {
      expect(() =>
        runCompare({
          oracleRecords: matching,
          candidateRecords,
          scenarios,
          platform: POSIX,
        }),
      ).toThrow(/record set/u);
    }
  });

  it("derives skipped scenarios that neither side ran", () => {
    const matching = records();
    const { audit } = runCompare({
      oracleRecords: matching,
      candidateRecords: structuredClone(matching),
      scenarios,
      platform: POSIX,
    });
    expect(audit.parityHeld).toBe(true);
    expect(audit.skipped).toEqual(["c"]);
  });

  it("rejects malformed records", () => {
    expect(() =>
      runCompare({
        oracleRecords: [{ scenarioId: 42 }, record("b", "b".repeat(64)), unsupported("c")],
        candidateRecords: records(),
        scenarios,
        platform: POSIX,
      }),
    ).toThrow(/malformed oracle record/);
  });

  it("rejects subject mismatches and subject-specific malformed fields", () => {
    const valid = records();
    const wrongSubject = structuredClone(valid);
    wrongSubject[0] = {
      scenarioId: "a",
      subject: "version-identity",
      outcome: SCENARIO_OUTCOME.COMPLETED,
      result: { version: "0.0.0" },
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
    delete missingHash[0].result.stateDirSha256;
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
    const matching = records();
    const { audit } = runCompare({
      oracleRecords: matching,
      candidateRecords: structuredClone(matching),
      scenarios,
      platform: POSIX,
      corpusVersion: 4,
      corpusDigest: "c".repeat(64),
      sourceIdentity: { commit: "d".repeat(40) },
    });
    expect(audit.schemaVersion).toBe(3);
    expect(audit.referenceRecords).toBe(3);
    expect(audit.referenceRecordsSha256).toMatch(/^[0-9a-f]{64}$/u);
    expect(audit.candidateRecordsSha256).toMatch(/^[0-9a-f]{64}$/u);
    expect(audit.perSubject["state-dir"]).toMatchObject({
      total: 3,
      applicable: 2,
      requiredApplicable: 1,
      matchedRequired: 1,
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
    const matching = [record("a", "a".repeat(64))];
    const canonical = canonicalRecordDocument(matching);
    expect(parseCanonicalRecords(canonical, "oracle")).toEqual(matching);
    expect(() => parseCanonicalRecords(canonical.trim(), "oracle")).toThrow(/not exact canonical/u);
    expect(() =>
      parseCanonicalRecords(
        `${JSON.stringify({ schemaVersion: 1, records: matching }, null, 2)}\n`,
        "oracle",
      ),
    ).toThrow(/not exact canonical/u);
  });

  it("rejects a malformed or unsupported runner protocol", () => {
    expect(() => parseCanonicalRecords('{"records":[],"schemaVersion":2}\n', "candidate")).toThrow(
      /unsupported schemaVersion/u,
    );
    expect(() => parseCanonicalRecords('{"schemaVersion":1}\n', "candidate")).toThrow(
      /unknown or missing fields/u,
    );
  });

  it("binds audit provenance to commit and directly hashed source bytes", () => {
    const identity = collectSourceIdentity(ROOT);
    expect(Object.keys(identity).sort()).toEqual([
      "candidate",
      "commit",
      "reference",
      "sourceFiles",
      "sourceTreeSha256",
    ]);
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
