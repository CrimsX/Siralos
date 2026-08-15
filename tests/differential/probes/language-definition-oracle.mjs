/**
 * language-definition oracle probe (differential harness, ADR 0033,
 * Stage 3R R5).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes language-definition scenarios against the REAL TypeScript
 * reference implementation: the LSP definition normalization (adapter
 * normalizers module, which consumes the generic core definition
 * module) with the real mirror URI mapping. This is a thin scenario
 * adapter: it builds the normalization context from declared inputs and
 * maps results to the canonical record vocabulary; it does not
 * reimplement definition behavior.
 */
import { readFileSync } from "node:fs";
import { normalizeDefinition } from "../../../packages/adapters/src/godot/lsp/normalizers.js";
import { GODOT_LIMITS } from "../../../packages/core/src/godot/limits.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();
const root = input.root;
const queries = [];
for (const query of input.queries ?? []) {
  const result = normalizeDefinition(
    query.uri,
    query.locations,
    {
      mirrorRootPath: root,
      path: query.path,
    },
    {
      maxLocations: query.max ?? GODOT_LIMITS.lspMaxDefinitionLocations,
    },
  );
  queries.push({
    uri: query.uri,
    path: result.path,
    locations: result.locations.map((location) => ({
      path: location.path,
      range: {
        start: { line: location.range.start.line, column: location.range.start.column },
        end: { line: location.range.end.line, column: location.range.end.column },
      },
      external: location.external,
    })),
    truncated: result.truncated,
  });
}
process.stdout.write(JSON.stringify({ queries }));
