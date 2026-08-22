/**
 * godot-scene-resolve oracle probe (differential harness, ADR 0033,
 * Stage 3R R8).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes godot-scene-resolve scenarios against the REAL TypeScript
 * reference implementation: the bounded Godot scene/resource parsers
 * (core godot/scene modules) with the R4 revision-handle identity
 * (null revision for direct text inputs). Thin, bounded, no engine —
 * mirrors the pattern of language-structure-oracle.mjs.
 */
import { readFileSync } from "node:fs";
import { parseGodotScene } from "../../../packages/core/src/godot/scene/scene-parser.js";
import { parseGodotResource } from "../../../packages/core/src/godot/scene/resource-parser.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();
const path = typeof input.path === "string" ? input.path : "";
const tscn = typeof input.tscn === "string" ? input.tscn : null;
const tres = typeof input.tres === "string" ? input.tres : null;

let content;
let isResource;
if (tres !== null) {
  content = tres;
  isResource = true;
} else if (tscn !== null && tscn.includes("[gd_resource")) {
  content = tscn;
  isResource = true;
} else {
  content = tscn ?? tres ?? "";
  isResource = false;
}

const doc = isResource
  ? parseGodotResource(content, path, { revision: null })
  : parseGodotScene(content, path, { revision: null });

const rawStatus = doc.status;
const status =
  rawStatus === "Complete"
    ? "complete"
    : rawStatus === "Partial"
      ? "partial"
      : rawStatus === "Invalid"
        ? "invalid"
        : rawStatus === "complete"
          ? "complete"
          : rawStatus === "partial"
            ? "partial"
            : rawStatus === "invalid"
              ? "invalid"
              : String(rawStatus).toLowerCase();

process.stdout.write(
  JSON.stringify({
    status,
    diagnostics: doc.diagnostics.length,
    truncated: doc.truncated,
  }),
);
