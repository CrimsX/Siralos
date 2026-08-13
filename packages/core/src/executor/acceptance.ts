import type { EvidenceRecord, AcceptanceState } from "../tasks/task-model.js";
import { evidenceSourceSupportsSuccessfulOutcome } from "../tasks/task-evidence-outcome.js";
import type { MilestoneManifest, AcceptanceRequirement } from "./milestone-manifest.js";
import { resolveAcceptanceEvidenceKinds } from "./standard-acceptance.js";

/**
 * Host-owned milestone acceptance evaluation (executor briefing
 * foundation).
 *
 * A small, deterministic evaluator that maps milestone acceptance
 * requirements to HOST-OBSERVED evidence: the task's attached
 * EvidenceRecords and the host-verified AcceptanceStates of its
 * TaskContract. There is no path for an executor claim to enter: the
 * evaluator accepts only structured evidence already attached by host
 * flows, so "all tests pass" asserted in prose can never satisfy a
 * requirement. This is deliberately not a generic rules engine — it
 * supports exactly the four requirement states below.
 */

export type AcceptanceRequirementStatus = "pass" | "fail" | "incomplete" | "not_applicable";

export interface MilestoneRequirementResult {
  readonly id: string;
  readonly status: AcceptanceRequirementStatus;
  /** Evidence ids / criterion ids that satisfied the requirement, when any. */
  readonly satisfiedBy: readonly string[];
  readonly note: string | null;
}

export interface MilestoneAcceptanceCounts {
  readonly pass: number;
  readonly fail: number;
  readonly incomplete: number;
  readonly not_applicable: number;
  readonly total: number;
}

export interface MilestoneAcceptanceReport {
  readonly manifestId: string;
  readonly manifestVersion: number;
  readonly requirements: readonly MilestoneRequirementResult[];
  readonly counts: MilestoneAcceptanceCounts;
  /**
   * True only when every non-optional requirement passes (or is
   * not_applicable). A single incomplete requirement keeps the milestone
   * incomplete even if an executor claims otherwise.
   */
  readonly passed: boolean;
}

export interface AcceptanceEvaluationInput {
  readonly manifest: MilestoneManifest;
  /** Exact authoritative task contract identity being evaluated. */
  readonly task: {
    readonly taskId: string;
    readonly contractRevision: number;
    readonly contractDigest: string | null;
  };
  /** Host-attached evidence records of the task (never executor claims). */
  readonly evidence: readonly EvidenceRecord[];
  /** Host-verified acceptance states of the task's contract. */
  readonly acceptance: readonly AcceptanceState[];
}

function isCurrentTaskEvidence(
  record: EvidenceRecord,
  task: AcceptanceEvaluationInput["task"],
): boolean {
  return (
    task.contractDigest !== null &&
    record.taskId === task.taskId &&
    record.taskContractRevision === task.contractRevision &&
    record.taskContractDigest === task.contractDigest
  );
}

function kindCanVerifyCriterion(record: EvidenceRecord, criterion: AcceptanceState): boolean {
  if (criterion.verificationKind === "user") {
    return record.kind === "user_approval";
  }
  if (criterion.verificationKind === "review") {
    return record.kind === "review_result";
  }
  return record.kind !== "review_result" && record.kind !== "user_approval";
}

function validSuccessfulRecord(record: EvidenceRecord): boolean {
  return (
    record.verification?.outcome === "passed" &&
    evidenceSourceSupportsSuccessfulOutcome(record.kind, record.source)
  );
}

function evaluateRequirement(
  manifest: MilestoneManifest,
  requirement: AcceptanceRequirement,
  task: AcceptanceEvaluationInput["task"],
  evidence: readonly EvidenceRecord[],
  acceptance: readonly AcceptanceState[],
): MilestoneRequirementResult {
  const evidenceKinds = resolveAcceptanceEvidenceKinds(requirement);

  // A linked TaskContract criterion is the strongest signal: it can only
  // be satisfied through host verification (verifiedBy evidence id).
  if (requirement.criterionId !== undefined) {
    const criterion = acceptance.find((entry) => entry.criterionId === requirement.criterionId);
    if (criterion === undefined) {
      return requirement.optional === true
        ? {
            id: requirement.id,
            status: "not_applicable",
            satisfiedBy: [],
            note: "No linked task criterion exists for this task.",
          }
        : {
            id: requirement.id,
            status: "incomplete",
            satisfiedBy: [],
            note: "No linked task criterion exists for this task.",
          };
    }
    if (criterion.status === "satisfied" && criterion.verifiedBy !== null) {
      const records = evidence.filter((entry) => entry.id === criterion.verifiedBy);
      const record = records.length === 1 ? records[0] : undefined;
      if (
        record === undefined ||
        !isCurrentTaskEvidence(record, task) ||
        record.verification?.criterionId !== requirement.criterionId ||
        record.verification.checkId !== requirement.checkId ||
        !kindCanVerifyCriterion(record, criterion) ||
        (evidenceKinds.length > 0 && !evidenceKinds.includes(record.kind)) ||
        !validSuccessfulRecord(record)
      ) {
        return {
          id: requirement.id,
          status: "fail",
          satisfiedBy: [],
          note: `Linked criterion ${requirement.criterionId} has invalid or stale verification evidence.`,
        };
      }
      return {
        id: requirement.id,
        status: "pass",
        satisfiedBy: [record.id],
        note: null,
      };
    }
    if (criterion.status === "failed") {
      return {
        id: requirement.id,
        status: "fail",
        satisfiedBy: [],
        note: `Linked criterion ${requirement.criterionId} failed.`,
      };
    }
    return {
      id: requirement.id,
      status: "incomplete",
      satisfiedBy: [],
      note: `Linked criterion ${requirement.criterionId} is not host-verified.`,
    };
  }

  // A kind is only a whitelist. Direct milestone evidence must additionally
  // target this immutable manifest requirement/check and current task
  // contract, and its structured source must independently show success.
  const targeted = evidence.filter(
    (record) =>
      isCurrentTaskEvidence(record, task) &&
      record.verification?.milestone?.manifestId === manifest.id &&
      record.verification.milestone.manifestVersion === manifest.version &&
      record.verification.milestone.requirementId === requirement.id,
  );
  const passing = targeted.filter(
    (record) =>
      record.verification?.checkId === requirement.checkId &&
      evidenceKinds.includes(record.kind) &&
      validSuccessfulRecord(record),
  );
  if (passing.length > 0) {
    return {
      id: requirement.id,
      status: "pass",
      satisfiedBy: passing.map((record) => record.id).sort(),
      note: null,
    };
  }
  if (targeted.length > 0) {
    return {
      id: requirement.id,
      status: targeted.some((record) => record.verification?.outcome === "failed")
        ? "fail"
        : "incomplete",
      satisfiedBy: [],
      note: "Targeted host evidence did not contain a matching successful check outcome.",
    };
  }
  if (requirement.optional === true) {
    return {
      id: requirement.id,
      status: "not_applicable",
      satisfiedBy: [],
      note: "No matching host evidence; optional requirement not applicable.",
    };
  }
  return {
    id: requirement.id,
    status: "incomplete",
    satisfiedBy: [],
    note: "No matching host-attached evidence.",
  };
}

export interface AcceptanceEvaluator {
  evaluate(input: AcceptanceEvaluationInput): MilestoneAcceptanceReport;
}

export function createAcceptanceEvaluator(): AcceptanceEvaluator {
  return {
    evaluate(input: AcceptanceEvaluationInput): MilestoneAcceptanceReport {
      const requirements = input.manifest.acceptance.map((requirement) =>
        evaluateRequirement(
          input.manifest,
          requirement,
          input.task,
          input.evidence,
          input.acceptance,
        ),
      );
      const counts: MilestoneAcceptanceCounts = {
        pass: 0,
        fail: 0,
        incomplete: 0,
        not_applicable: 0,
        total: requirements.length,
      };
      const mutable: { pass: number; fail: number; incomplete: number; not_applicable: number } =
        counts;
      for (const result of requirements) {
        mutable[result.status] += 1;
      }
      const passed = counts.fail === 0 && counts.incomplete === 0 && counts.total > 0;
      return {
        manifestId: input.manifest.id,
        manifestVersion: input.manifest.version,
        requirements,
        counts,
        passed,
      };
    },
  };
}
