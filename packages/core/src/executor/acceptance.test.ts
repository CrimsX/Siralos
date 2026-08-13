import { describe, expect, it } from "vitest";
import type { AcceptanceState, EvidenceRecord } from "../tasks/task-model.js";
import { createAcceptanceEvaluator } from "./acceptance.js";
import { createMilestoneManifest } from "./milestone-manifest.js";
import { S3M8_MILESTONE_MANIFEST } from "./s3m8-manifest.js";
import { S3R2_MILESTONE_MANIFEST } from "./s3r2-manifest.js";

const TASK = {
  taskId: "task-1",
  contractRevision: 1,
  contractDigest: "a".repeat(64),
} as const;

let evidenceSequence = 0;

function evidence(
  overrides: Partial<EvidenceRecord> & { kind: EvidenceRecord["kind"] },
): EvidenceRecord {
  evidenceSequence += 1;
  return {
    id: `ev-${evidenceSequence}`,
    taskId: TASK.taskId,
    taskContractRevision: TASK.contractRevision,
    taskContractDigest: TASK.contractDigest,
    attachedAtMs: evidenceSequence,
    source: { type: "parser", checkedFiles: 1, validFiles: 1, errors: 0 },
    verification: null,
    ...overrides,
  };
}

function milestoneEvidence(
  manifest: typeof S3R2_MILESTONE_MANIFEST,
  requirementId: string,
  overrides: Partial<EvidenceRecord> = {},
): EvidenceRecord {
  const requirement = manifest.acceptance.find((entry) => entry.id === requirementId);
  if (requirement === undefined) {
    throw new Error(`Unknown test requirement: ${requirementId}`);
  }
  return evidence({
    kind: "validation_result",
    source: {
      type: "validation",
      outcome: "passed",
      workspaceIntegrityVerified: true,
      unexpectedChanges: 0,
    },
    verification: {
      checkId: requirement.checkId,
      criterionId: null,
      milestone: {
        manifestId: manifest.id,
        manifestVersion: manifest.version,
        requirementId,
      },
      outcome: "passed",
    },
    ...overrides,
  });
}

describe("acceptance evaluator", () => {
  it("cannot pass without host evidence: empty evidence leaves every requirement incomplete", () => {
    const manifest = S3M8_MILESTONE_MANIFEST;
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
      task: TASK,
      evidence: [],
      acceptance: [],
    });
    expect(report.counts.total).toBe(manifest.acceptance.length);
    expect(report.counts.incomplete).toBe(manifest.acceptance.length);
    expect(report.counts.pass).toBe(0);
    expect(report.passed).toBe(false);
  });

  it("an executor claim string can never enter the evaluator", () => {
    const manifest = S3M8_MILESTONE_MANIFEST;
    const claim = { text: "All acceptance criteria passed." };
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
      task: TASK,
      evidence: [],
      acceptance: [
        {
          criterionId: claim.text,
          description: "",
          verificationKind: "deterministic",
          status: "pending",
          verifiedBy: null,
          note: null,
        },
      ],
    });
    expect(report.counts.pass).toBe(0);
    expect(report.counts.incomplete).toBe(manifest.acceptance.length);
  });

  it("does not treat evidence kind as acceptance authority", () => {
    const manifest = S3M8_MILESTONE_MANIFEST;
    const records = [
      evidence({ kind: "parser_result", id: "ev-parse" }),
      evidence({
        kind: "validation_result",
        id: "ev-validation",
        source: {
          type: "validation",
          outcome: "passed",
          workspaceIntegrityVerified: true,
          unexpectedChanges: 0,
        },
      }),
    ];
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
      task: TASK,
      evidence: records,
      acceptance: [],
    });
    expect(report.counts.pass).toBe(0);
    expect(report.passed).toBe(false);
  });

  it("passes only evidence bound to the exact immutable milestone check", () => {
    const record = milestoneEvidence(S3R2_MILESTONE_MANIFEST, "S3R2.CORPUS.SCHEMA");
    const report = createAcceptanceEvaluator().evaluate({
      manifest: S3R2_MILESTONE_MANIFEST,
      task: TASK,
      evidence: [record],
      acceptance: [],
    });
    expect(report.requirements.find((entry) => entry.id === "S3R2.CORPUS.SCHEMA")).toMatchObject({
      status: "pass",
      satisfiedBy: [record.id],
    });
    expect(report.counts.pass).toBe(1);
    expect(report.counts.incomplete).toBe(S3R2_MILESTONE_MANIFEST.acceptance.length - 1);
  });

  it("passes via an exactly bound host-verified criterion and fails mismatched evidence", () => {
    const linked = createMilestoneManifest({
      id: "S3M9",
      title: "t",
      goal: "g",
      acceptance: [{ id: "S3M9.X", description: "x", criterionId: "crit-x" }],
    });
    const evaluator = createAcceptanceEvaluator();
    const satisfied: AcceptanceState[] = [
      {
        criterionId: "crit-x",
        description: "x",
        verificationKind: "deterministic",
        status: "satisfied",
        verifiedBy: "ev-criterion",
        note: null,
      },
    ];
    const bound = evidence({
      kind: "parser_result",
      id: "ev-criterion",
      verification: {
        checkId: "crit-x",
        criterionId: "crit-x",
        milestone: null,
        outcome: "passed",
      },
    });
    expect(
      evaluator.evaluate({ manifest: linked, task: TASK, evidence: [bound], acceptance: satisfied })
        .passed,
    ).toBe(true);

    const mismatched = {
      ...bound,
      verification: { ...bound.verification!, criterionId: "other" },
    };
    const failed = evaluator.evaluate({
      manifest: linked,
      task: TASK,
      evidence: [mismatched],
      acceptance: satisfied,
    });
    expect(failed.requirements[0]?.status).toBe("fail");
    expect(failed.passed).toBe(false);
  });

  it("requires review and user criteria to use their explicit evidence paths", () => {
    const linked = createMilestoneManifest({
      id: "S3M9",
      title: "t",
      goal: "g",
      acceptance: [
        { id: "S3M9.REVIEW", description: "review", criterionId: "reviewed" },
        { id: "S3M9.USER", description: "user", criterionId: "approved" },
      ],
    });
    const acceptance: AcceptanceState[] = [
      {
        criterionId: "reviewed",
        description: "review",
        verificationKind: "review",
        status: "satisfied",
        verifiedBy: "ev-review-wrong-kind",
        note: null,
      },
      {
        criterionId: "approved",
        description: "user",
        verificationKind: "user",
        status: "satisfied",
        verifiedBy: null,
        note: null,
      },
    ];
    const wrongKind = evidence({
      kind: "parser_result",
      id: "ev-review-wrong-kind",
      verification: {
        checkId: "reviewed",
        criterionId: "reviewed",
        milestone: null,
        outcome: "passed",
      },
    });
    const report = createAcceptanceEvaluator().evaluate({
      manifest: linked,
      task: TASK,
      evidence: [wrongKind],
      acceptance,
    });
    expect(report.requirements.map((entry) => entry.status)).toEqual(["fail", "incomplete"]);
  });

  it("optional requirements become not_applicable only when their target is absent", () => {
    const optional = createMilestoneManifest({
      id: "S3M9",
      title: "t",
      goal: "g",
      acceptance: [{ id: "S3M9.X", description: "x", criterionId: "crit-x", optional: true }],
    });
    const report = createAcceptanceEvaluator().evaluate({
      manifest: optional,
      task: TASK,
      evidence: [],
      acceptance: [],
    });
    expect(report.requirements[0]?.status).toBe("not_applicable");
    expect(report.passed).toBe(true);
  });

  it("a failing or unrelated validation record cannot pass S3R2", () => {
    const failed = milestoneEvidence(S3R2_MILESTONE_MANIFEST, "S3R2.CORPUS.SCHEMA", {
      source: {
        type: "validation",
        outcome: "failed",
        workspaceIntegrityVerified: false,
        unexpectedChanges: 1,
      },
      verification: {
        checkId: "S3R2.CORPUS.SCHEMA",
        criterionId: null,
        milestone: {
          manifestId: "S3R2",
          manifestVersion: 1,
          requirementId: "S3R2.CORPUS.SCHEMA",
        },
        outcome: "failed",
      },
    });
    const unrelated = milestoneEvidence(S3R2_MILESTONE_MANIFEST, "S3R2.ADR.REGISTERED");
    const report = createAcceptanceEvaluator().evaluate({
      manifest: S3R2_MILESTONE_MANIFEST,
      task: TASK,
      evidence: [failed, unrelated],
      acceptance: [],
    });
    expect(report.requirements.find((entry) => entry.id === "S3R2.CORPUS.SCHEMA")?.status).toBe(
      "fail",
    );
    expect(report.counts.pass).toBe(1);
    expect(report.passed).toBe(false);
  });

  it("rejects stale task revisions, digests, and check identities", () => {
    const requirementId = "S3R2.CORPUS.SCHEMA";
    const stale = milestoneEvidence(S3R2_MILESTONE_MANIFEST, requirementId, {
      taskContractRevision: 0,
    });
    const wrongDigest = milestoneEvidence(S3R2_MILESTONE_MANIFEST, requirementId, {
      taskContractDigest: "b".repeat(64),
    });
    const wrongCheck = milestoneEvidence(S3R2_MILESTONE_MANIFEST, requirementId, {
      verification: {
        checkId: "OTHER.CHECK",
        criterionId: null,
        milestone: { manifestId: "S3R2", manifestVersion: 1, requirementId },
        outcome: "passed",
      },
    });
    const report = createAcceptanceEvaluator().evaluate({
      manifest: S3R2_MILESTONE_MANIFEST,
      task: TASK,
      evidence: [stale, wrongDigest, wrongCheck],
      acceptance: [],
    });
    expect(report.requirements.find((entry) => entry.id === requirementId)?.status).toBe(
      "incomplete",
    );
    expect(report.passed).toBe(false);
  });

  it("reports deterministic counts and overall state", () => {
    const manifest = S3M8_MILESTONE_MANIFEST;
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
      task: TASK,
      evidence: [],
      acceptance: [],
    });
    expect(report.manifestId).toBe("S3M8");
    expect(
      report.counts.pass +
        report.counts.fail +
        report.counts.incomplete +
        report.counts.not_applicable,
    ).toBe(report.counts.total);
  });
});
