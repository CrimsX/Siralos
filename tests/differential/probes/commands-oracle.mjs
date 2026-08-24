import { readFileSync } from "node:fs";
import {
  COMMAND_CATALOG,
  COMMAND_CATALOG_IDS,
  COMMAND_CATALOG_REVISION,
  catalogEntry,
} from "../../../packages/core/src/commands/command-catalog.ts";
import { canonicalizeJson, sha256Hex } from "../../../packages/core/src/godot/digest.js";
import { createNodeScriptRunner } from "../../../packages/adapters/src/process/runners/node-script-runner.ts";
import { createNpmScriptRunner } from "../../../packages/adapters/src/process/runners/npm-script-runner.ts";

function runCase(inputCase) {
  switch (inputCase.name) {
    case "catalog-snapshot":
      return {
        entries: COMMAND_CATALOG.map((entry) => ({
          id: entry.id,
          description: entry.description,
          group: entry.group,
        })),
        revision: COMMAND_CATALOG_REVISION,
      };
    case "unknown-command-refusal": {
      const entry = catalogEntry("definitely-not-a-command");
      return entry === undefined ? { found: false } : { found: true, id: entry.id };
    }
    case "known-entry-lookup": {
      const entry = catalogEntry(inputCase.probe);
      return entry === undefined ? { found: false } : { found: true, entry };
    }
    case "revision-recomputation": {
      const recomputed = sha256Hex(
        canonicalizeJson(
          COMMAND_CATALOG.map((entry) => ({ id: entry.id, description: entry.description })),
        ),
      );
      return { stable: recomputed === COMMAND_CATALOG_REVISION, ids: [...COMMAND_CATALOG_IDS] };
    }
    case "runner-availability": {
      return (async () => {
        const nodeScript = createNodeScriptRunner({});
        const npmScript = createNpmScriptRunner({});
        return {
          nodeScript: {
            definitionId: nodeScript.definition.id,
            available: await nodeScript.isAvailable(),
          },
          npmScript: {
            definitionId: npmScript.definition.id,
            available: await npmScript.isAvailable(),
          },
        };
      })();
    }
    default:
      throw new Error(`unknown command-catalog fixture case ${inputCase.name}`);
  }
}

const input = JSON.parse(readFileSync(0, "utf8"));
const results = [];
for (const item of input.cases) {
  results.push(await runCase(item));
}
process.stdout.write(JSON.stringify({ cases: results }));
