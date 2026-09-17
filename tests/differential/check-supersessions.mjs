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
        { cwd: root, encoding: "utf8" },
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

main();
