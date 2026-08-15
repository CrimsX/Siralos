/**
 * language-structure oracle probe (differential harness, ADR 0033,
 * Stage 3R R5).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes language-structure scenarios against the REAL TypeScript
 * reference implementation: the generic structural-document
 * normalization and the deterministic advisory summary builder (core
 * language module), with the R4 revision-handle identity. The scenario
 * input IS the generic structural observation (language-neutral
 * declarations); the GDScript scanner that produces such observations
 * remains the later Godot milestone's oracle. This is a thin scenario
 * adapter: it passes the declared structure to the production
 * normalization and formatter and maps the result to the canonical
 * record vocabulary.
 */
import { readFileSync } from "node:fs";
import { computeWorkspaceRevisionHandle } from "../../../packages/core/src/workspace/workspace-revision.js";
import {
  DEFAULT_STRUCTURE_LIMITS,
  buildStructuralSummary,
  normalizeStructuralDocument,
  parseStructuralKind,
} from "../../../packages/core/src/language/structure.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

function readLine(value) {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 1 ? value : null;
}

function readString(value) {
  return typeof value === "string" ? value : null;
}

function readAttributes(value) {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.filter((entry) => typeof entry === "string");
}

function parseDeclaration(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return {
      kind: "other",
      name: null,
      detail: null,
      line: null,
      attributes: [],
      children: [],
    };
  }
  return {
    kind: parseStructuralKind(value.kind) ?? "other",
    name: readString(value.name),
    detail: readString(value.detail),
    line: readLine(value.line),
    attributes: readAttributes(value.attributes),
    children: Array.isArray(value.children) ? value.children.map(parseDeclaration) : [],
  };
}

function parseStructure(value) {
  const path = readString(value.path) ?? "";
  const declarations = Array.isArray(value.declarations)
    ? value.declarations.map(parseDeclaration)
    : [];
  const dependencies = readAttributes(value.dependencies);
  const issues = Array.isArray(value.issues)
    ? value.issues.map((entry) => ({
        line: readLine(entry?.line),
        message: typeof entry?.message === "string" ? entry.message : "",
      }))
    : [];
  return { path, declarations, dependencies, issues };
}

const input = readStdinBounded();
const fingerprint = input.fingerprint;
const summaries = [];
for (const document of input.documents ?? []) {
  const structure = document.structure;
  if (structure === null || typeof structure !== "object" || Array.isArray(structure)) {
    throw new Error("language-structure document requires a structure object");
  }
  const parsed = parseStructure(structure);
  const revision =
    typeof document.sha256 === "string" && parsed.path.length > 0
      ? computeWorkspaceRevisionHandle(fingerprint, parsed.path, document.sha256)
      : null;
  const normalized = normalizeStructuralDocument(
    parsed.path,
    parsed.declarations,
    parsed.dependencies,
    parsed.issues,
    DEFAULT_STRUCTURE_LIMITS,
  );
  const summary = buildStructuralSummary(
    { ...normalized, revision },
    {
      maxBytes: document.maxBytes,
      notableDeclarations: document.notableDeclarations,
    },
  );
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
