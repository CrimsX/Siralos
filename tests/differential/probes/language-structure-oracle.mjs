/**
 * language-structure oracle probe (differential harness, ADR 0033,
 * Stage 3R R5).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes language-structure scenarios against the REAL TypeScript
 * reference implementation: the deterministic advisory structural
 * summary builder (core workspace module) over scenario-declared
 * structural facts, with the R4 revision-handle identity. The scenario
 * input IS the structural document (generic declarations); the GDScript
 * scanner that produces such documents remains the later Godot
 * milestone's oracle. This is a thin scenario adapter: it passes the
 * declared structure to the production formatter and maps the result to
 * the canonical record vocabulary.
 */
import { readFileSync } from "node:fs";
import { buildWorkspaceSummary } from "../../../packages/core/src/workspace/workspace-summary.js";
import { computeWorkspaceRevisionHandle } from "../../../packages/core/src/workspace/workspace-revision.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();
const fingerprint = input.fingerprint;
const summaries = [];
for (const document of input.documents ?? []) {
  const structure = document.structure;
  if (structure === null || typeof structure !== "object" || Array.isArray(structure)) {
    throw new Error("language-structure document requires a structure object");
  }
  const revision =
    typeof document.sha256 === "string" && typeof structure.path === "string"
      ? computeWorkspaceRevisionHandle(fingerprint, structure.path, document.sha256)
      : null;
  const summary = buildWorkspaceSummary(structure, revision, {
    maxBytes: document.maxBytes,
    notableMethods: document.notableMethods,
  });
  summaries.push({
    path: summary.path,
    revision: summary.revision,
    mode: "summary",
    advisory: true,
    truncated: summary.truncated,
    bytes: summary.bytes,
    text: summary.text,
  });
}
process.stdout.write(JSON.stringify({ summaries }));
