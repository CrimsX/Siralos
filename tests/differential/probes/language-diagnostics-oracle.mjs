/**
 * language-diagnostics oracle probe (differential harness, ADR 0033,
 * Stage 3R R5).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes language-diagnostics scenarios against the REAL TypeScript
 * reference implementation: the generic payload normalization (core
 * language module), the real URI mapping (adapter file-uri module), the
 * deterministic set aggregation (core gdscript module), and the R4
 * revision-handle identity. This is a thin scenario adapter: it wires
 * production functions exactly like the Godot publish-diagnostics
 * wrapper and maps results to the canonical record vocabulary; it does
 * not reimplement language behavior.
 *
 * Deterministic: revision handles come from declared inputs; no ambient
 * clock, randomness, or environment access enters records.
 */
import { readFileSync } from "node:fs";
import { mirrorUriToWorkspaceRelative } from "../../../packages/adapters/src/godot/lsp/file-uri.js";
import { normalizeDiagnosticPayload } from "../../../packages/core/src/language/diagnostic.js";
import { aggregateGDScriptDiagnostics } from "../../../packages/core/src/godot/gdscript.js";
import { computeWorkspaceRevisionHandle } from "../../../packages/core/src/workspace/workspace-revision.js";
import { GODOT_LIMITS } from "../../../packages/core/src/godot/limits.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function diagnosticEntry(diagnostic) {
  return {
    source: diagnostic.source,
    severity: diagnostic.severity,
    path: diagnostic.path,
    line: diagnostic.line,
    column: diagnostic.column,
    code: diagnostic.code,
    message: diagnostic.message,
    rawCategory: diagnostic.rawCategory,
  };
}

const input = readStdinBounded();
const fingerprint = input.fingerprint;
const root = input.root;
const source = input.source;
const documents = [];
const all = [];
for (const document of input.documents ?? []) {
  const uri = document.uri;
  const path = mirrorUriToWorkspaceRelative(uri, root);
  if (path === null || !Array.isArray(document.diagnostics)) {
    documents.push({ uri, status: "rejected" });
    continue;
  }
  const payload = normalizeDiagnosticPayload(document.diagnostics, source, path, root, {
    maxDiagnostics: document.max ?? GODOT_LIMITS.lspMaxDiagnosticsPerDocument,
    maxMessageBytes: GODOT_LIMITS.maxDiagnosticMessageBytes,
  });
  if (payload === null) {
    documents.push({ uri, status: "rejected" });
    continue;
  }
  const revision =
    typeof document.sha256 === "string"
      ? computeWorkspaceRevisionHandle(fingerprint, payload.path, document.sha256)
      : null;
  documents.push({
    uri,
    status: "normalized",
    path: payload.path,
    revision,
    diagnostics: payload.diagnostics.map(diagnosticEntry),
    truncated: payload.truncated,
  });
  all.push(...payload.diagnostics);
}
const aggregate = aggregateGDScriptDiagnostics(
  all,
  input.runMax ?? GODOT_LIMITS.maxDiagnosticsPerRun,
);
const output = {
  documents,
  aggregate: {
    diagnostics: aggregate.diagnostics.map(diagnosticEntry),
    truncated: aggregate.truncated,
  },
};
process.stdout.write(JSON.stringify(output));
