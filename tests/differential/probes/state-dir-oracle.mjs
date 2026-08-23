/**
 * State-dir oracle probe (differential harness, ADR 0033).
 *
 * Spawned by the oracle runner with a scrubbed environment containing
 * exactly the scenario's fixtures. Prints the TypeScript reference's
 * state directory (`os.homedir()` + `.siralos`, mirroring the CLI's
 * runs-root construction) or the literal marker `ERR` when no home
 * directory can be resolved. Never writes a trailing newline; the
 * runner hashes the exact bytes.
 */
import { homedir } from "node:os";
import { join } from "node:path";

let home;
try {
  home = homedir();
} catch {
  home = null;
}
if (home === null || home === "") {
  process.stdout.write("ERR");
  process.exit(0);
}
process.stdout.write(join(home, ".siralos"));
