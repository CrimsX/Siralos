/**
 * godot-develop-plan oracle probe (differential harness, ADR 0033,
 * Stage 3R R9).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Executes develop-plan scenarios against the REAL TypeScript reference
 * deterministic core (packages/core/src/godot/development): surface
 * routing from host-observed evidence, cross-target dependency edges,
 * and the topological apply order with its rationale. Thin, bounded, no
 * engine, no writes; mirrors crates/siralos-cli/src/harness.rs::
 * godot_develop_plan_record.
 */
import { readFileSync } from "node:fs";
import { classifyDevelopmentSurface } from "../../../packages/core/src/godot/development/development-surface.js";
import {
  deriveUnifiedApplyOrder,
  deriveUnifiedOrderEdges,
} from "../../../packages/core/src/godot/development/unified-order.js";

const MAX_INPUT_BYTES = 64 * 1024;

function readStdinBounded() {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
}

const input = readStdinBounded();

const decision = classifyDevelopmentSurface({
  request: input.request ?? "",
  touchpoints: (input.touchpoints ?? []).map((touchpoint) => ({
    path: touchpoint.path,
    status: touchpoint.status === "verified" ? "verified" : "candidate",
  })),
  ...(input.projectSurfaces === undefined || input.projectSurfaces === null
    ? {}
    : {
        projectSurfaces: {
          hasScenes: input.projectSurfaces.hasScenes === true,
          hasResources: input.projectSurfaces.hasResources === true,
          hasScripts: input.projectSurfaces.hasScripts === true,
        },
      }),
});

const targets = (input.targets ?? []).map((target) => ({
  targetId: target.targetId,
  path: target.path,
  references: [...(target.references ?? [])],
}));
const { edges, unresolvedReferences } = deriveUnifiedOrderEdges(targets);

let applyOrder;
let applyOrderError;
try {
  applyOrder = deriveUnifiedApplyOrder(targets, edges);
} catch (error) {
  applyOrderError = error.message;
}

process.stdout.write(
  JSON.stringify({
    surface: {
      kind: decision.kind,
      rationale: decision.rationale,
      evidence: [...decision.evidence],
    },
    edges: edges.map((edge) => ({ before: edge.before, after: edge.after })),
    unresolvedReferences: unresolvedReferences.map((reference) => ({
      targetId: reference.targetId,
      path: reference.path,
    })),
    ...(applyOrderError === undefined
      ? {
          applyOrder: {
            order: [...applyOrder.order],
            rationale: applyOrder.rationale,
          },
        }
      : { applyOrderError }),
  }),
);
