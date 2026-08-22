/**
 * godot-mutation-prepare oracle probe (differential harness, ADR 0033,
 * Stage 3R R9).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes mutation-prepare scenarios against the REAL TypeScript
 * reference contracts (packages/core/src/godot/scene-mutation):
 * validateMutationOperations, expectedSemanticEffect, and the prepared
 * artifact fingerprint. Thin, bounded, no engine, no writes; mirrors
 * crates/siralos-cli/src/harness.rs::godot_mutation_prepare_record.
 */
import { readFileSync } from "node:fs";
import {
  expectedSemanticEffect,
  validateMutationOperations,
} from "../../../packages/core/src/godot/scene-mutation/operations.js";
import { createPreparedGodotMutation } from "../../../packages/core/src/godot/scene-mutation/prepared.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();

let validated;
try {
  validated = validateMutationOperations(input.operations ?? []);
} catch (error) {
  process.stdout.write(JSON.stringify({ ok: false, error: error.message }));
  process.exit(0);
}

try {
  const preview = {
    structuralSummary: input.previewSummary ?? "",
    diff: input.previewDiff ?? "",
  };
  const created = createPreparedGodotMutation({
    targetPath: input.targetPath ?? "",
    sourceRevision: input.sourceRevision ?? "",
    sourceSha256: input.sourceSha256 ?? "",
    kind: input.kind === "resource" ? "resource" : "scene",
    operations: validated,
    expectedSemanticEffect: expectedSemanticEffect(validated),
    preview,
    serializedAfter: input.serializedAfter ?? "",
    addedLines: input.addedLines ?? 0,
    removedLines: input.removedLines ?? 0,
  });

  process.stdout.write(
    JSON.stringify({
      ok: true,
      fingerprint: created.fingerprint,
      operations: [...created.operations],
      expectedSemanticEffect: [...created.expectedSemanticEffect],
      structuralSummary: created.preview.structuralSummary,
      diff: created.preview.diff,
    }),
  );
} catch (error) {
  process.stdout.write(JSON.stringify({ ok: false, error: error.message }));
}
