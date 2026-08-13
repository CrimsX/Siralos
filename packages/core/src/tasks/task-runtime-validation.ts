import { deepFreeze } from "../domain/deep-freeze.js";
import { evidenceSourceSupportsSuccessfulOutcome } from "./task-evidence-outcome.js";
import type {
  EvidenceKind,
  EvidenceSource,
  EvidenceVerification,
  FindingRef,
  TaskStepSpec,
} from "./task-model.js";

export const MAX_EVIDENCE_SOURCE_BYTES = 4096;
export const MAX_TASK_EVIDENCE_RECORDS = 256;
export const MAX_TASK_STEPS = 128;
export const MAX_TASK_FINDINGS = 128;
export const MAX_TASK_STEP_DESCRIPTION_BYTES = 4096;
export const MAX_TASK_FINDING_FIELD_BYTES = 4096;
export const MAX_TASK_EVIDENCE_ID_BYTES = 256;

const textEncoder = new TextEncoder();
const TASK_STEP_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,63}$/;
const TASK_STEP_KINDS = new Set(["research", "implementation", "review"]);
const FINDING_SEVERITIES = new Set(["critical", "high", "medium", "low"]);
const EVIDENCE_SOURCE_TYPES_BY_KIND: Readonly<
  Record<EvidenceKind, readonly EvidenceSource["type"][]>
> = {
  workspace_read: ["workspace_read"],
  api_lookup: ["api_lookup"],
  lsp_query: ["lsp_query"],
  change_preview: ["change_preview"],
  mutation_receipt: ["mutation"],
  checkpoint: ["checkpoint"],
  parser_result: ["parser"],
  lsp_result: ["lsp"],
  // Stage 3 milestone 11: validation evidence also carries native
  // verification, cross-surface consistency, and impact sources.
  validation_result: ["validation", "native_verification", "consistency", "impact"],
  review_result: ["review"],
  user_approval: ["user_approval"],
  reference_read: ["reference_read"],
  reference_search: ["reference_search"],
  research: ["research"],
};
const EVIDENCE_KINDS = new Set<EvidenceKind>(
  Object.keys(EVIDENCE_SOURCE_TYPES_BY_KIND) as EvidenceKind[],
);
const EVIDENCE_BINDING_ID_PATTERN = /^[A-Za-z][A-Za-z0-9._-]{0,127}$/;
const VERIFICATION_OUTCOMES = new Set(["passed", "failed", "incomplete"]);

export function prepareTaskStepSpecs(
  steps: readonly TaskStepSpec[] | undefined,
): ReadonlyMap<string, TaskStepSpec> {
  if ((steps ?? []).length > MAX_TASK_STEPS) {
    throw new Error(`A task accepts at most ${MAX_TASK_STEPS} steps.`);
  }
  const specs = new Map<string, TaskStepSpec>();
  for (const spec of steps ?? []) {
    if (!TASK_STEP_ID_PATTERN.test(spec.id)) {
      throw new Error(`Invalid task step id: ${spec.id}`);
    }
    if (specs.has(spec.id)) {
      throw new Error(`Duplicate task step id: ${spec.id}`);
    }
    const description = spec.description.trim();
    if (description.length === 0) {
      throw new Error(`Task step ${spec.id} requires a non-empty description.`);
    }
    if (textEncoder.encode(description).length > MAX_TASK_STEP_DESCRIPTION_BYTES) {
      throw new Error(
        `Task step ${spec.id} description exceeds ${MAX_TASK_STEP_DESCRIPTION_BYTES} UTF-8 bytes.`,
      );
    }
    if (!TASK_STEP_KINDS.has(spec.kind)) {
      throw new Error(`Task step ${spec.id} has invalid kind ${String(spec.kind)}.`);
    }
    if (spec.accepts.length === 0) {
      throw new Error(`Task step ${spec.id} accepts no evidence kinds.`);
    }
    if (spec.accepts.some((kind) => !EVIDENCE_KINDS.has(kind))) {
      throw new Error(`Task step ${spec.id} contains an invalid evidence kind.`);
    }
    if (new Set(spec.accepts).size !== spec.accepts.length) {
      throw new Error(`Task step ${spec.id} contains duplicate evidence kinds.`);
    }
    specs.set(spec.id, deepFreeze({ ...spec, description, accepts: [...spec.accepts] }));
  }
  return specs;
}

export function normalizeTaskIteration(iteration: number | undefined): number {
  return iteration === undefined || !Number.isFinite(iteration)
    ? 0
    : Math.max(0, Math.floor(iteration));
}

export function validateAndCloneTaskFindings(findings: readonly FindingRef[]): FindingRef[] {
  if (findings.length > MAX_TASK_FINDINGS) {
    throw new Error(`A task accepts at most ${MAX_TASK_FINDINGS} current findings.`);
  }
  const ids = new Set<string>();
  for (const finding of findings) {
    if (finding.findingId.trim().length === 0 || finding.source.trim().length === 0) {
      throw new Error("Task findings require non-empty ids and sources.");
    }
    if (
      textEncoder.encode(finding.findingId).length > MAX_TASK_FINDING_FIELD_BYTES ||
      textEncoder.encode(finding.source).length > MAX_TASK_FINDING_FIELD_BYTES
    ) {
      throw new Error(
        `Task finding fields cannot exceed ${MAX_TASK_FINDING_FIELD_BYTES} UTF-8 bytes.`,
      );
    }
    if (ids.has(finding.findingId)) {
      throw new Error(`Duplicate task finding id: ${finding.findingId}`);
    }
    ids.add(finding.findingId);
    if (!FINDING_SEVERITIES.has(finding.severity)) {
      throw new Error(`Invalid task finding severity: ${String(finding.severity)}`);
    }
  }
  return findings.map((finding) => ({ ...finding }));
}

export type EvidencePayloadValidation =
  | {
      readonly ok: true;
      readonly source: EvidenceSource;
      readonly verification: EvidenceVerification | null;
    }
  | { readonly ok: false; readonly reason: string };

export function validateEvidencePayload(input: {
  readonly id: string;
  readonly kind: EvidenceKind;
  readonly source: EvidenceSource;
  readonly verification?: EvidenceVerification;
}): EvidencePayloadValidation {
  if (input.id.trim().length === 0) {
    return { ok: false, reason: "Evidence requires a non-empty id." };
  }
  if (textEncoder.encode(input.id).length > MAX_TASK_EVIDENCE_ID_BYTES) {
    return {
      ok: false,
      reason: `Evidence id exceeds the ${MAX_TASK_EVIDENCE_ID_BYTES}-byte bound.`,
    };
  }
  if (!EVIDENCE_KINDS.has(input.kind)) {
    return { ok: false, reason: `Unknown evidence kind: ${String(input.kind)}` };
  }
  if (
    input.source.type !== undefined &&
    !EVIDENCE_SOURCE_TYPES_BY_KIND[input.kind].includes(input.source.type)
  ) {
    return {
      ok: false,
      reason: `Evidence kind ${input.kind} requires source type ${EVIDENCE_SOURCE_TYPES_BY_KIND[
        input.kind
      ].join(" or ")}, not ${input.source.type}.`,
    };
  }
  const verification = validateEvidenceVerification(input.verification);
  if (!verification.ok) {
    return verification;
  }
  try {
    const serialized = JSON.stringify(input.source);
    const source = structuredClone(input.source);
    if (textEncoder.encode(serialized).length > MAX_EVIDENCE_SOURCE_BYTES) {
      return {
        ok: false,
        reason: `Evidence source exceeds the ${MAX_EVIDENCE_SOURCE_BYTES}-byte bound; attach a reference, not raw output.`,
      };
    }
    if (
      verification.verification?.outcome === "passed" &&
      !evidenceSourceSupportsSuccessfulOutcome(input.kind, source)
    ) {
      return {
        ok: false,
        reason: "Passed verification evidence must contain a successful source outcome.",
      };
    }
    return { ok: true, source, verification: verification.verification };
  } catch {
    return { ok: false, reason: "Evidence source must be finite JSON-serializable data." };
  }
}

function validateEvidenceVerification(
  input: EvidenceVerification | undefined,
):
  | { readonly ok: true; readonly verification: EvidenceVerification | null }
  | { readonly ok: false; readonly reason: string } {
  if (input === undefined) {
    return { ok: true, verification: null };
  }
  if (!EVIDENCE_BINDING_ID_PATTERN.test(input.checkId)) {
    return { ok: false, reason: `Invalid evidence verification check id: ${input.checkId}` };
  }
  if (!VERIFICATION_OUTCOMES.has(input.outcome)) {
    return {
      ok: false,
      reason: `Invalid evidence verification outcome: ${String(input.outcome)}`,
    };
  }
  if (input.criterionId !== null && !EVIDENCE_BINDING_ID_PATTERN.test(input.criterionId)) {
    return {
      ok: false,
      reason: `Invalid evidence verification criterion id: ${input.criterionId}`,
    };
  }
  if (input.milestone !== null) {
    if (
      !EVIDENCE_BINDING_ID_PATTERN.test(input.milestone.manifestId) ||
      !EVIDENCE_BINDING_ID_PATTERN.test(input.milestone.requirementId) ||
      !Number.isSafeInteger(input.milestone.manifestVersion) ||
      input.milestone.manifestVersion < 1
    ) {
      return { ok: false, reason: "Invalid milestone evidence target." };
    }
  }
  if (input.criterionId === null && input.milestone === null) {
    return {
      ok: false,
      reason: "Verification evidence must bind a task criterion or milestone requirement.",
    };
  }
  return { ok: true, verification: structuredClone(input) };
}
