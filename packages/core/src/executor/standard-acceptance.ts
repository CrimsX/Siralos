import { deepFreeze } from "../domain/deep-freeze.js";
import type { EvidenceKind } from "../tasks/task-model.js";

/**
 * Standard acceptance library (executor briefing foundation).
 *
 * Several milestones repeatedly require the same observable property
 * (no workspace mutation, no process execution, no network, no secret
 * output, no tool leakage, full validation). A milestone manifest
 * references these definitions by stable id instead of restating them.
 * This is a small fixed vocabulary — not an arbitrary policy language —
 * and definitions only ever describe host-observed evidence kinds; they
 * grant nothing and cannot satisfy acceptance by themselves.
 */

export type StandardAcceptanceId =
  | "STANDARD.NO_WORKSPACE_MUTATION"
  | "STANDARD.NO_PROCESS_EXECUTION"
  | "STANDARD.NO_NETWORK"
  | "STANDARD.NO_SECRET_OUTPUT"
  | "STANDARD.NO_TOOL_LEAKAGE"
  | "STANDARD.FULL_VALIDATION";

export interface StandardAcceptanceDefinition {
  readonly id: StandardAcceptanceId;
  readonly description: string;
  /** Evidence kinds whose host-attached records count toward this property. */
  readonly evidenceKinds: readonly EvidenceKind[];
}

export const STANDARD_ACCEPTANCE_DEFINITIONS: Readonly<
  Record<StandardAcceptanceId, StandardAcceptanceDefinition>
> = deepFreeze({
  "STANDARD.NO_WORKSPACE_MUTATION": {
    id: "STANDARD.NO_WORKSPACE_MUTATION",
    description: "No workspace mutation: no create/edit/delete/undo of workspace files.",
    evidenceKinds: ["validation_result", "review_result"],
  },
  "STANDARD.NO_PROCESS_EXECUTION": {
    id: "STANDARD.NO_PROCESS_EXECUTION",
    description: "No process execution or engine launch for the inspected surface.",
    evidenceKinds: ["validation_result", "review_result"],
  },
  "STANDARD.NO_NETWORK": {
    id: "STANDARD.NO_NETWORK",
    description: "No network access during the task.",
    evidenceKinds: ["validation_result", "review_result"],
  },
  "STANDARD.NO_SECRET_OUTPUT": {
    id: "STANDARD.NO_SECRET_OUTPUT",
    description: "No secrets or absolute host paths appear in provider-visible output.",
    evidenceKinds: ["review_result", "validation_result"],
  },
  "STANDARD.NO_TOOL_LEAKAGE": {
    id: "STANDARD.NO_TOOL_LEAKAGE",
    description: "No native mutation tool surface leaks into provider-visible tools.",
    evidenceKinds: ["review_result", "validation_result"],
  },
  "STANDARD.FULL_VALIDATION": {
    id: "STANDARD.FULL_VALIDATION",
    description: "The standard repository validation profile ran and passed.",
    evidenceKinds: ["validation_result"],
  },
});

export const STANDARD_ACCEPTANCE_IDS: readonly StandardAcceptanceId[] = Object.keys(
  STANDARD_ACCEPTANCE_DEFINITIONS,
) as readonly StandardAcceptanceId[];

/** Deterministic resolved evidence kinds for one requirement (manifest + standards). */
export function resolveAcceptanceEvidenceKinds(input: {
  readonly evidenceKinds?: readonly EvidenceKind[];
  readonly standardIds?: readonly StandardAcceptanceId[];
}): readonly EvidenceKind[] {
  const kinds: EvidenceKind[] = [...(input.evidenceKinds ?? [])];
  for (const standardId of input.standardIds ?? []) {
    const definition = STANDARD_ACCEPTANCE_DEFINITIONS[standardId];
    if (definition !== undefined) {
      for (const kind of definition.evidenceKinds) {
        if (!kinds.includes(kind)) {
          kinds.push(kind);
        }
      }
    }
  }
  return kinds;
}
