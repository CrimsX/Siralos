import type { TaskContract } from "../tasks/task-contract.js";
import {
  PLANNING_LIMITS,
  PLAN_CONSTRAINT_ID_PATTERN,
  PLAN_REVISION_HANDLE_PATTERN,
  PLAN_RISK_ID_PATTERN,
  PLAN_STEP_ID_PATTERN,
  PLAN_TOUCHPOINT_ID_PATTERN,
  type PlanConstraint,
  type PlanRisk,
  type PlanStep,
  type PlanTouchpoint,
  type TaskPlan,
  type TaskPlanContent,
} from "./planning-model.js";
import type { PlanningDepth } from "./planning-model.js";

/**
 * Host-owned plan candidate validation boundary (Stage 3 milestone 7,
 * Part H). Planner output is untrusted data: it becomes a `TaskPlan` only
 * after this deterministic validation passes. Malformed output is REJECTED
 * (planning failure / bounded retry) — never silently treated as plan
 * prose.
 *
 * Validation covers structure, bounds, path containment, evidence/
 * revision identity of verified touchpoints, acceptance-criteria linkage,
 * secret content, and policy-shaped capability claims. The plan model has
 * no capability surface at all; requirement-shaped text that tries to
 * claim policy authority is additionally rejected so a plan can never
 * even LOOK like it grants capability.
 */

export interface PlanCandidateContext {
  /** The exact TaskContract the plan will bind to. */
  readonly contract: TaskContract;
  /** The host-routed planning depth; the candidate must match it. */
  readonly depth: PlanningDepth;
}

export type PlanCandidateResult =
  | { readonly ok: true; readonly content: TaskPlanContent }
  | { readonly ok: false; readonly reasons: readonly string[] };

const textEncoder = new TextEncoder();

function utf8Bytes(text: string): number {
  return textEncoder.encode(text).length;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string");
}

function nonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

/** Conservative deterministic check for secret-shaped content. */
const SECRET_PATTERNS: readonly RegExp[] = [
  /\b(api[_-]?key|secret|password|passwd|token|bearer|private[_-]?key|credential|access[_-]?key)\b\s*[:=]\s*[A-Za-z0-9_\-./+]{8,}/i,
  /\b(sk|pk|ghp|gho|ghu|AKIA)[A-Za-z0-9]{16,}\b/,
];

/** Conservative deterministic policy-claim patterns (mirrors knowledge's
 * `rejectPolicyShapedContent` posture; plans can never claim authority). */
const POLICY_CLAIM_PATTERNS: readonly RegExp[] = [
  /\b(enable|allow|grant|permit|approve)\b[^.\n]{0,60}\b(unrestricted|full)?\s*(network|internet|shell|write|execution|commands?|sandbox|approval|mutation)\b/,
  /\b(disable|bypass|turn\s*off|turn\s*down|ignore|override)\b[^.\n]{0,60}\b(sandbox|approval|checkpoint|security|policy|restriction|limit|permission)\b/,
  /\bno\s+(approval|permission|checkpoint|review|sandbox)\b[^.\n]{0,40}\b(needed|required|necessary)\b/,
  /\b(commands?|scripts?|shell|execution|mutations?|writes?|edits?)\b[^.\n]{0,40}\b(without|no)\s+(approval|permission|checkpoint|review)\b/,
  /^[^.\n]{0,40}\b(unrestricted|full)\s+(network|shell|write|access)\b/,
];

export function rejectPlanPolicyClaims(text: string): string | null {
  const normalized = text.toLowerCase().replace(/\s+/g, " ").trim();
  for (const pattern of POLICY_CLAIM_PATTERNS) {
    if (pattern.test(normalized)) {
      return "The plan contains a policy-shaped capability claim; plans are descriptive and can never grant or disable capability, sandbox, approval, or execution policy.";
    }
  }
  return null;
}

function rejectSecretContent(text: string): string | null {
  for (const pattern of SECRET_PATTERNS) {
    if (pattern.test(text)) {
      return "The plan contains secret-shaped content and was rejected.";
    }
  }
  return null;
}

function bounded(value: string, maxBytes: number): boolean {
  return utf8Bytes(value) <= maxBytes;
}

/**
 * Workspace-relative path containment. Paths must be relative, use forward
 * slashes, contain no null bytes, never escape the workspace, never be a
 * reference/research namespace path, and never an absolute path.
 */
export function isSafePlanPath(path: string, options: { readonly allowGlob: boolean }): boolean {
  if (path.length === 0 || path.length > PLANNING_LIMITS.maxPathBytes) {
    return false;
  }
  if (path.includes("\0") || path.includes("\\") || path.startsWith("/")) {
    return false;
  }
  if (/^[A-Za-z]:/.test(path)) {
    return false;
  }
  if (path.startsWith("@reference/") || path.startsWith("@research/")) {
    return false;
  }
  const segments = path.split("/");
  if (segments.includes("..") || segments.includes(".") || segments.includes("")) {
    return false;
  }
  if (!options.allowGlob && (path.includes("*") || path.includes("?"))) {
    return false;
  }
  if (options.allowGlob && /[*?]/.test(path.replace(/\*\*/g, "").replace(/\*/g, ""))) {
    return false;
  }
  return true;
}

const EVIDENCE_KINDS = new Set([
  "read",
  "api",
  "reference",
  "research",
  "knowledge",
  "instruction",
]);

function isEvidenceReference(value: string): boolean {
  const separator = value.indexOf(":");
  if (separator <= 0) {
    return false;
  }
  const kind = value.slice(0, separator);
  const ref = value.slice(separator + 1);
  if (!EVIDENCE_KINDS.has(kind)) {
    return false;
  }
  return ref.length > 0 && ref.length <= 256 && !ref.includes("\0");
}

function collectReasons(target: string[], reason: string): void {
  target.push(reason);
}

/**
 * Validate untrusted planner output. `depth` must equal the host decision,
 * the contract revision is bound by `createTaskPlan` callers, verified
 * touchpoints must carry exact revision handles, references must resolve
 * against the contract's acceptance criteria, and all fields must fit the
 * deterministic bounds.
 */
export function validatePlanCandidate(
  raw: unknown,
  context: PlanCandidateContext,
): PlanCandidateResult {
  const reasons: string[] = [];
  if (!isPlainObject(raw)) {
    return { ok: false, reasons: ["The plan candidate must be a JSON object."] };
  }
  if (context.depth !== "light" && context.depth !== "full") {
    return { ok: false, reasons: ["A plan cannot be created at depth none."] };
  }
  const declaredDepth = raw["depth"];
  if (declaredDepth !== undefined && declaredDepth !== context.depth) {
    collectReasons(
      reasons,
      `The plan depth ${JSON.stringify(declaredDepth)} does not match the host-routed depth ${context.depth}.`,
    );
  }
  const objective = raw["objective"];
  if (!nonEmptyString(objective)) {
    collectReasons(reasons, "A plan requires a non-empty objective.");
  } else if (!bounded(objective, PLANNING_LIMITS.maxObjectiveBytes)) {
    collectReasons(
      reasons,
      `The objective exceeds the ${PLANNING_LIMITS.maxObjectiveBytes}-byte bound.`,
    );
  } else {
    const claim = rejectPlanPolicyClaims(objective);
    if (claim !== null) {
      collectReasons(reasons, claim);
    }
    const secret = rejectSecretContent(objective);
    if (secret !== null) {
      collectReasons(reasons, secret);
    }
  }

  const maxSteps =
    context.depth === "light" ? PLANNING_LIMITS.maxStepsLight : PLANNING_LIMITS.maxSteps;
  const scopeRaw = raw["scope"];
  if (scopeRaw === undefined && context.depth === "light") {
    // Light plans are compact: scope is not forced onto them (ADR 0020
    // §7: no fake filler for fields irrelevant to light plans).
  } else if (!isPlainObject(scopeRaw)) {
    collectReasons(reasons, "A plan requires a scope object.");
  } else {
    const inScope = scopeRaw["inScope"];
    const outOfScope = scopeRaw["outOfScope"];
    if (!isStringArray(inScope) || inScope.length > PLANNING_LIMITS.maxScopeEntries) {
      collectReasons(reasons, "A plan requires an inScope string array within bounds.");
    } else {
      for (const entry of inScope) {
        if (!bounded(entry, PLANNING_LIMITS.maxStatementBytes)) {
          collectReasons(reasons, "A scope entry exceeds the statement byte bound.");
          break;
        }
      }
    }
    if (!isStringArray(outOfScope) || outOfScope.length > PLANNING_LIMITS.maxScopeEntries) {
      collectReasons(reasons, "A plan requires an outOfScope string array within bounds.");
    } else {
      for (const entry of outOfScope) {
        if (!bounded(entry, PLANNING_LIMITS.maxStatementBytes)) {
          collectReasons(reasons, "A scope entry exceeds the statement byte bound.");
          break;
        }
      }
    }
  }

  const nonGoals = raw["nonGoals"];
  if (nonGoals === undefined && context.depth === "light") {
    // Compact light plans may omit non-goals.
  } else if (!isStringArray(nonGoals) || nonGoals.length > PLANNING_LIMITS.maxNonGoals) {
    collectReasons(reasons, "nonGoals must be a bounded string array.");
  } else {
    for (const entry of nonGoals) {
      if (!bounded(entry, PLANNING_LIMITS.maxStatementBytes)) {
        collectReasons(reasons, "A non-goal exceeds the statement byte bound.");
        break;
      }
    }
  }

  const touchpointIds = new Set<string>();
  const touchpointsRaw = raw["touchpoints"];
  if (!Array.isArray(touchpointsRaw) || touchpointsRaw.length > PLANNING_LIMITS.maxTouchpoints) {
    collectReasons(reasons, "touchpoints must be a bounded array.");
  } else {
    for (const entry of touchpointsRaw) {
      if (!isPlainObject(entry)) {
        collectReasons(reasons, "Each touchpoint must be an object.");
        continue;
      }
      const id = entry["id"];
      const path = entry["path"];
      const confidence = entry["confidence"];
      if (!nonEmptyString(id) || !PLAN_TOUCHPOINT_ID_PATTERN.test(id)) {
        collectReasons(reasons, "Each touchpoint requires a valid id.");
        continue;
      }
      if (touchpointIds.has(id)) {
        collectReasons(reasons, `Duplicate touchpoint id: ${id}`);
        continue;
      }
      touchpointIds.add(id);
      if (!nonEmptyString(path) || !bounded(path, PLANNING_LIMITS.maxPathBytes)) {
        collectReasons(reasons, `Touchpoint ${id} requires a bounded path.`);
        continue;
      }
      if (confidence !== "verified" && confidence !== "candidate") {
        collectReasons(reasons, `Touchpoint ${id} requires confidence verified or candidate.`);
        continue;
      }
      const allowGlob = confidence === "candidate";
      if (!isSafePlanPath(path, { allowGlob })) {
        collectReasons(
          reasons,
          `Touchpoint ${id} path ${JSON.stringify(path)} is not a safe workspace-relative path.`,
        );
      }
      if (confidence === "verified") {
        const revision = entry["revision"];
        if (!nonEmptyString(revision) || !PLAN_REVISION_HANDLE_PATTERN.test(revision)) {
          collectReasons(
            reasons,
            `Verified touchpoint ${id} requires the exact inspected workspace revision handle (rev_ + 32 hex).`,
          );
        } else if (!bounded(revision, PLANNING_LIMITS.maxRevisionBytes)) {
          collectReasons(reasons, `Touchpoint ${id} revision exceeds the byte bound.`);
        }
      } else {
        const revision = entry["revision"];
        if (revision !== undefined && !nonEmptyString(revision)) {
          collectReasons(reasons, `Touchpoint ${id} revision must be a string when present.`);
        }
      }
      const evidence = entry["evidence"];
      if (evidence !== undefined && (!nonEmptyString(evidence) || !isEvidenceReference(evidence))) {
        collectReasons(
          reasons,
          `Touchpoint ${id} evidence must be a bounded kind:ref reference (read|api|reference|research|knowledge|instruction).`,
        );
      }
      const note = entry["note"];
      if (
        note !== undefined &&
        (!nonEmptyString(note) || !bounded(note, PLANNING_LIMITS.maxNoteBytes))
      ) {
        collectReasons(reasons, `Touchpoint ${id} note exceeds the byte bound.`);
      }
    }
  }

  const constraintIds = new Set<string>();
  const constraintsRaw = raw["constraints"];
  if (constraintsRaw === undefined) {
    // Constraints are optional for both depths (compact plans).
  } else if (
    !Array.isArray(constraintsRaw) ||
    constraintsRaw.length > PLANNING_LIMITS.maxConstraints
  ) {
    collectReasons(reasons, "constraints must be a bounded array.");
  } else {
    for (const entry of constraintsRaw) {
      if (!isPlainObject(entry)) {
        collectReasons(reasons, "Each constraint must be an object.");
        continue;
      }
      const id = entry["id"];
      const description = entry["description"];
      if (!nonEmptyString(id) || !PLAN_CONSTRAINT_ID_PATTERN.test(id)) {
        collectReasons(reasons, "Each constraint requires a valid id.");
        continue;
      }
      if (constraintIds.has(id)) {
        collectReasons(reasons, `Duplicate constraint id: ${id}`);
        continue;
      }
      constraintIds.add(id);
      if (
        !nonEmptyString(description) ||
        !bounded(description, PLANNING_LIMITS.maxStatementBytes)
      ) {
        collectReasons(reasons, `Constraint ${id} requires a bounded description.`);
      } else {
        const claim = rejectPlanPolicyClaims(description);
        if (claim !== null) {
          collectReasons(reasons, claim);
        }
      }
    }
  }

  const riskIds = new Set<string>();
  const risksRaw = raw["risks"];
  if (risksRaw === undefined) {
    // Risks are optional for both depths (compact plans).
  } else if (!Array.isArray(risksRaw) || risksRaw.length > PLANNING_LIMITS.maxRisks) {
    collectReasons(reasons, "risks must be a bounded array.");
  } else {
    for (const entry of risksRaw) {
      if (!isPlainObject(entry)) {
        collectReasons(reasons, "Each risk must be an object.");
        continue;
      }
      const id = entry["id"];
      const severity = entry["severity"];
      const description = entry["description"];
      if (!nonEmptyString(id) || !PLAN_RISK_ID_PATTERN.test(id)) {
        collectReasons(reasons, "Each risk requires a valid id.");
        continue;
      }
      if (riskIds.has(id)) {
        collectReasons(reasons, `Duplicate risk id: ${id}`);
        continue;
      }
      riskIds.add(id);
      if (severity !== "low" && severity !== "medium" && severity !== "high") {
        collectReasons(reasons, `Risk ${id} requires severity low, medium, or high.`);
      }
      if (
        !nonEmptyString(description) ||
        !bounded(description, PLANNING_LIMITS.maxStatementBytes)
      ) {
        collectReasons(reasons, `Risk ${id} requires a bounded description.`);
      }
    }
  }

  const stepIds = new Set<string>();
  const stepsRaw = raw["steps"];
  if (!Array.isArray(stepsRaw) || stepsRaw.length === 0 || stepsRaw.length > maxSteps) {
    collectReasons(
      reasons,
      `steps must be a non-empty bounded array (at most ${maxSteps} for ${context.depth} plans).`,
    );
  } else {
    for (const entry of stepsRaw) {
      if (!isPlainObject(entry)) {
        collectReasons(reasons, "Each step must be an object.");
        continue;
      }
      const id = entry["id"];
      const title = entry["title"];
      if (!nonEmptyString(id) || !PLAN_STEP_ID_PATTERN.test(id)) {
        collectReasons(reasons, "Each step requires a valid id.");
        continue;
      }
      if (stepIds.has(id)) {
        collectReasons(reasons, `Duplicate step id: ${id}`);
        continue;
      }
      stepIds.add(id);
      if (!nonEmptyString(title) || !bounded(title, PLANNING_LIMITS.maxStepTitleBytes)) {
        collectReasons(reasons, `Step ${id} requires a bounded title.`);
      } else {
        const claim = rejectPlanPolicyClaims(title);
        if (claim !== null) {
          collectReasons(reasons, claim);
        }
      }
      const description = entry["description"];
      if (
        description !== undefined &&
        (!nonEmptyString(description) ||
          !bounded(description, PLANNING_LIMITS.maxStepDescriptionBytes))
      ) {
        collectReasons(reasons, `Step ${id} description exceeds the byte bound.`);
      }
      const expectedTouchpoints = entry["expectedTouchpoints"];
      if (
        !Array.isArray(expectedTouchpoints) ||
        expectedTouchpoints.length > PLANNING_LIMITS.maxExpectedTouchpointsPerStep ||
        expectedTouchpoints.some((ref) => typeof ref !== "string")
      ) {
        collectReasons(reasons, `Step ${id} expectedTouchpoints must be a bounded id array.`);
      } else {
        for (const ref of expectedTouchpoints as string[]) {
          if (!touchpointIds.has(ref)) {
            collectReasons(
              reasons,
              `Step ${id} references unknown touchpoint ${JSON.stringify(ref)}.`,
            );
          }
        }
      }
      const verification = entry["verification"];
      if (verification !== undefined) {
        if (
          !Array.isArray(verification) ||
          verification.length > PLANNING_LIMITS.maxVerificationRefsPerStep ||
          verification.some((ref) => typeof ref !== "string")
        ) {
          collectReasons(reasons, `Step ${id} verification must be a bounded criterion-id array.`);
        } else {
          for (const ref of verification) {
            if (!context.contract.acceptanceCriteria.some((criterion) => criterion.id === ref)) {
              collectReasons(
                reasons,
                `Step ${id} references unknown acceptance criterion ${JSON.stringify(ref)}.`,
              );
            }
          }
        }
      }
    }
  }

  const validationRaw = raw["validation"];
  if (!isPlainObject(validationRaw)) {
    collectReasons(reasons, "A plan requires a validation object.");
  } else {
    const checks = validationRaw["checks"];
    if (
      !isStringArray(checks) ||
      checks.length === 0 ||
      checks.length > PLANNING_LIMITS.maxValidationChecks
    ) {
      collectReasons(reasons, "validation.checks must be a non-empty bounded string array.");
    } else {
      for (const check of checks) {
        if (!bounded(check, PLANNING_LIMITS.maxStatementBytes)) {
          collectReasons(reasons, "A validation check exceeds the statement byte bound.");
          break;
        }
      }
    }
    const requirements = validationRaw["requirements"];
    if (requirements !== undefined) {
      if (
        !isStringArray(requirements) ||
        requirements.length > PLANNING_LIMITS.maxValidationRequirements
      ) {
        collectReasons(reasons, "validation.requirements must be a bounded string array.");
      } else {
        for (const requirement of requirements) {
          if (!bounded(requirement, PLANNING_LIMITS.maxStatementBytes)) {
            collectReasons(reasons, "A validation requirement exceeds the statement byte bound.");
            break;
          }
          const claim = rejectPlanPolicyClaims(requirement);
          if (claim !== null) {
            collectReasons(reasons, claim);
          }
        }
      }
    }
  }

  const rollback = raw["rollback"];
  const rollbackObject = isPlainObject(rollback) ? rollback : null;
  if (rollback !== undefined && rollbackObject === null) {
    collectReasons(reasons, "rollback requires an object.");
  } else if (
    rollbackObject !== null &&
    (!nonEmptyString(rollbackObject["description"]) ||
      !bounded(rollbackObject["description"], PLANNING_LIMITS.maxRollbackBytes))
  ) {
    collectReasons(reasons, "rollback requires a bounded description.");
  }

  const rationale = raw["rationale"];
  if (
    rationale !== undefined &&
    (!nonEmptyString(rationale) || !bounded(rationale, PLANNING_LIMITS.maxRationaleBytes))
  ) {
    collectReasons(reasons, "rationale must be a bounded string.");
  }

  // Aggregate content byte bound: an enormous plan is rejected, never
  // injected into context.
  if (reasons.length === 0) {
    const serialized = textEncoder.encode(
      JSON.stringify({
        objective,
        scope: scopeRaw,
        nonGoals,
        touchpoints: touchpointsRaw,
        constraints: constraintsRaw,
        risks: risksRaw,
        steps: stepsRaw,
        validation: validationRaw,
        rollback,
        rationale,
      }),
    ).length;
    if (serialized > PLANNING_LIMITS.maxPlanContentBytes) {
      collectReasons(
        reasons,
        `The plan exceeds the ${PLANNING_LIMITS.maxPlanContentBytes}-byte content bound.`,
      );
    }
  }

  if (reasons.length > 0) {
    return { ok: false, reasons: reasons.slice(0, 8) };
  }

  // Construct a fresh exact-shape value. Validation deliberately ignores
  // unknown keys, but those keys must never cross the boundary into the
  // immutable plan (where they could carry unreviewed data or defeat the
  // documented schema even while all known fields are valid).
  const cleanTouchpoints = (touchpointsRaw as Record<string, unknown>[]).map((entry) => ({
    id: entry["id"] as string,
    path: entry["path"] as string,
    confidence: entry["confidence"] as PlanTouchpoint["confidence"],
    ...(entry["revision"] === undefined ? {} : { revision: entry["revision"] as string }),
    ...(entry["evidence"] === undefined ? {} : { evidence: entry["evidence"] as string }),
    ...(entry["note"] === undefined ? {} : { note: entry["note"] as string }),
  }));
  const cleanConstraints: PlanConstraint[] = (
    constraintsRaw === undefined ? [] : (constraintsRaw as Record<string, unknown>[])
  ).map((entry) => ({
    id: entry["id"] as string,
    description: entry["description"] as string,
  }));
  const cleanRisks: PlanRisk[] = (
    risksRaw === undefined ? [] : (risksRaw as Record<string, unknown>[])
  ).map((entry) => ({
    id: entry["id"] as string,
    severity: entry["severity"] as PlanRisk["severity"],
    description: entry["description"] as string,
  }));
  const cleanSteps: PlanStep[] = (stepsRaw as Record<string, unknown>[]).map((entry) => ({
    id: entry["id"] as string,
    title: entry["title"] as string,
    ...(entry["description"] === undefined ? {} : { description: entry["description"] as string }),
    expectedTouchpoints: [...(entry["expectedTouchpoints"] as string[])],
    ...(entry["verification"] === undefined
      ? {}
      : { verification: [...(entry["verification"] as string[])] }),
  }));
  const content: TaskPlanContent = {
    objective: objective as string,
    scope:
      scopeRaw === undefined
        ? { inScope: [], outOfScope: [] }
        : {
            inScope: [...((scopeRaw as Record<string, unknown>)["inScope"] as string[])],
            outOfScope: [...((scopeRaw as Record<string, unknown>)["outOfScope"] as string[])],
          },
    nonGoals: nonGoals === undefined ? [] : [...(nonGoals as string[])],
    touchpoints: cleanTouchpoints,
    constraints: cleanConstraints,
    risks: cleanRisks,
    steps: cleanSteps,
    validation: {
      checks: [...((validationRaw as Record<string, unknown>)["checks"] as string[])],
      ...(((validationRaw as Record<string, unknown>)["requirements"] as string[] | undefined) ===
      undefined
        ? {}
        : {
            requirements: [
              ...((validationRaw as Record<string, unknown>)["requirements"] as string[]),
            ],
          }),
    },
    ...(rollbackObject === null
      ? {}
      : { rollback: { description: rollbackObject["description"] as string } }),
    ...(rationale === undefined ? {} : { rationale: rationale as string }),
  };
  return { ok: true, content };
}

/** Extract a JSON object from planner text (tolerant of one code fence). */
export function extractPlanCandidateJson(text: string): unknown {
  const trimmed = text.trim();
  const fenced = /^```(?:json)?\s*([\s\S]*?)\s*```$/.exec(trimmed);
  const candidate = fenced === null ? trimmed : (fenced[1] ?? trimmed);
  try {
    const parsed: unknown = JSON.parse(candidate);
    return isPlainObject(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * Surface staleness of a plan's verified touchpoints against the current
 * workspace revision state. A verified touchpoint is stale when the path's
 * CURRENT revision no longer equals the recorded inspected revision. This
 * never invalidates the whole plan automatically; it surfaces staleness so
 * the workflow can revalidate before mutation (Part M §38).
 */
export function planTouchpointStaleness(
  plan: TaskPlan,
  currentRevision: (path: string) => string | null,
): readonly string[] {
  const stale: string[] = [];
  for (const touchpoint of plan.touchpoints) {
    if (touchpoint.confidence !== "verified" || touchpoint.revision === undefined) {
      continue;
    }
    const current = currentRevision(touchpoint.path);
    if (current !== touchpoint.revision) {
      stale.push(touchpoint.path);
    }
  }
  return stale;
}
