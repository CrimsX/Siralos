/**
 * godot-review-context oracle probe (differential harness, ADR 0033,
 * Stage 3R R9).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes review-context scenarios against the REAL TypeScript
 * reference impact analyzer (packages/core/src/godot/impact): the
 * declared edges/signals/autoloads/candidate-tests/revisions build an
 * in-memory relationship source, and analyzeImpact derives the bounded
 * manifest. Thin, bounded, no engine; mirrors the canonical record
 * vocabulary of crates/siralos-cli/src/harness.rs::
 * godot_review_context_record.
 */
import { readFileSync } from "node:fs";
import { analyzeImpact } from "../../../packages/core/src/godot/impact/impact-analyzer.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();

/** In-memory relationship source over the scenario declaration. */
function createSource(declaration) {
  const edges = declaration.edges ?? [];
  const signalsByPath = new Map();
  for (const connection of declaration.signalConnections ?? []) {
    const list = signalsByPath.get(connection.path) ?? [];
    list.push({
      signal: connection.signal,
      sourceNode: connection.sourceNode,
      targetNode: connection.targetNode,
      targetMethod: connection.targetMethod,
    });
    signalsByPath.set(connection.path, list);
  }
  const autoloads = new Map((declaration.autoloads ?? []).map((entry) => [entry.path, entry.name]));
  const testsByPath = new Map();
  for (const entry of declaration.candidateTests ?? []) {
    testsByPath.set(entry.path, entry.tests ?? []);
  }
  const revisions = new Map();
  for (const entry of declaration.revisions ?? []) {
    revisions.set(entry.path, entry.revision ?? null);
  }
  return {
    outgoing(path) {
      return edges.filter((edge) => edge.fromPath === path);
    },
    incoming(path) {
      return edges.filter((edge) => edge.toPath === path);
    },
    async signalConnections(path) {
      return signalsByPath.get(path) ?? [];
    },
    autoloadName(path) {
      return autoloads.get(path) ?? null;
    },
    mainScene() {
      return declaration.mainScene ?? null;
    },
    currentRevision(path) {
      return revisions.has(path) ? revisions.get(path) : null;
    },
    async candidateTests(path) {
      return testsByPath.get(path) ?? [];
    },
  };
}

const manifest = await analyzeImpact({
  taskId: "differential-task",
  taskContractRevision: input.taskContractRevision,
  changedPaths: input.changedPaths ?? [],
  source: createSource(input),
});

process.stdout.write(
  JSON.stringify({
    taskId: manifest.taskId,
    taskContractRevision: manifest.taskContractRevision,
    primaryChanges: manifest.primaryChanges.map((surface) => ({
      path: surface.path,
      kind: surface.kind,
      revision: surface.revision ?? null,
      confidence: surface.confidence,
      evidence: surface.evidence,
      ...(surface.note === undefined || surface.note === null ? {} : { note: surface.note }),
    })),
    relatedSurfaces: manifest.relatedSurfaces.map((relation) => ({
      kind: relation.kind,
      sourcePath: relation.sourcePath,
      targetPath: relation.targetPath,
      sourceRevision: relation.sourceRevision ?? null,
      targetRevision: relation.targetRevision ?? null,
      confidence: relation.confidence,
      evidence: relation.evidence,
      ...(relation.note === undefined || relation.note === null ? {} : { note: relation.note }),
    })),
    regressionAreas: manifest.regressionAreas.map((area) => ({
      id: area.id,
      title: area.title,
      reason: area.reason,
      surfaces: [...area.surfaces],
    })),
    validation: manifest.validation.map((recommendation) => ({
      kind: recommendation.kind,
      priority: recommendation.priority,
      rationale: recommendation.rationale,
      surfaces: [...recommendation.surfaces],
    })),
    evidence: [...manifest.evidence],
    completeness: manifest.completeness,
    diagnostics: manifest.diagnostics.map((diagnostic) => ({
      code: diagnostic.code,
      message: diagnostic.message,
    })),
  }),
);
