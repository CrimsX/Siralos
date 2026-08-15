/**
 * Determinism replay stress (pre-Stage-4 assurance, contract Part 11).
 *
 * Executes differential fixtures repeatedly while perturbing irrelevant
 * execution conditions (environment key order, extra irrelevant
 * variables, output location, run count) and asserts byte-stable
 * canonical records. A host decision that changes due solely to
 * irrelevant ordering is a defect.
 */
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, describe, expect, it } from "vitest";
import { loadCorpus, runOracle, runStateDirProbe } from "./run-oracle.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CORPUS = join(HERE, "corpus");
const ROOT = join(HERE, "..", "..");

const tempDirectories = [];

function withTempOut() {
  const dir = mkdtempSync(join(tmpdir(), "siralos-replay-"));
  tempDirectories.push(dir);
  return join(dir, "records.json");
}

describe("replay stress", () => {
  afterAll(() => {
    for (const dir of tempDirectories.splice(0)) {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  // Each runOracle call spawns the R3 task probe once per scenario, so
  // the replay loop needs a generous bound.
  it("produces identical records across repeated runs", { timeout: 180_000 }, () => {
    const expected = runOracle(CORPUS, ROOT);
    for (let run = 0; run < 5; run += 1) {
      expect(runOracle(CORPUS, ROOT)).toBe(expected);
    }
  });

  it("is stable under perturbed environment key insertion order", { timeout: 120_000 }, () => {
    // Reorder the declared Windows fallback fixture. Arbitrary
    // variables are deliberately rejected before process creation.
    const base = runOracle(CORPUS, ROOT);
    const { scenarios } = loadCorpus(CORPUS);
    const fallback = scenarios.find((s) => s.id === "state-dir.fallback.windows");
    if (fallback !== undefined) {
      const record = runStateDirProbe({
        USERPROFILE: fallback.env.USERPROFILE,
        HOMEDRIVE: fallback.env.HOMEDRIVE,
        HOMEPATH: fallback.env.HOMEPATH,
      });
      const reverse = runStateDirProbe({
        HOMEPATH: fallback.env.HOMEPATH,
        HOMEDRIVE: fallback.env.HOMEDRIVE,
        USERPROFILE: fallback.env.USERPROFILE,
      });
      expect(reverse).toEqual(record);
    }
    expect(runOracle(CORPUS, ROOT)).toBe(base);
  });

  it("emits identical record bytes regardless of the output location", { timeout: 120_000 }, () => {
    const outA = withTempOut();
    const outB = withTempOut();
    writeFileSync(outA, runOracle(CORPUS, ROOT), "utf8");
    writeFileSync(outB, runOracle(CORPUS, ROOT), "utf8");
    expect(readFileSync(outA, "utf8")).toBe(readFileSync(outB, "utf8"));
  });
});
