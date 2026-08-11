import { describe, expect, it } from "vitest";
import { createAcceptanceEvaluator } from "./acceptance.js";
import { createMilestoneManifest } from "./milestone-manifest.js";
import { S3M8_MILESTONE_MANIFEST } from "./s3m8-manifest.js";
import type { AcceptanceState, EvidenceRecord } from "../tasks/task-model.js";

const manifest = S3M8_MILESTONE_MANIFEST;

function evidence(
  overrides: Partial<EvidenceRecord> & { kind: EvidenceRecord["kind"] },
): EvidenceRecord {
  return {
    id: `ev-${Math.random().toString(36).slice(2)}`,
    taskId: "task-1",
    attachedAtMs: 1,
    source: { type: "parser", checkedFiles: 1, validFiles: 1, errors: 0 },
    ...overrides,
  };
}

describe("acceptance evaluator", () => {
  it("cannot pass without host evidence: empty evidence leaves every requirement incomplete", () => {
    const report = createAcceptanceEvaluator().evaluate({ manifest, evidence: [], acceptance: [] });
    expect(report.counts.total).toBe(manifest.acceptance.length);
    expect(report.counts.incomplete).toBe(manifest.acceptance.length);
    expect(report.counts.pass).toBe(0);
    expect(report.passed).toBe(false);
  });

  it("an executor claim string can never enter the evaluator", () => {
    // The evaluator's input surface accepts only EvidenceRecord[] and
    // AcceptanceState[]; a prose claim is structurally unrepresentable.
    const claim = { text: "All acceptance criteria passed." };
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
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

  it("passes requirements whose evidence kinds have host-attached records", () => {
    const records = [
      evidence({ kind: "parser_result", id: "ev-parse" }),
      evidence({ kind: "workspace_read", id: "ev-read" }),
    ];
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
      evidence: records,
      acceptance: [],
    });
    const parse = report.requirements.find((requirement) => requirement.id === "S3M8.PARSE.TSCN");
    expect(parse?.status).toBe("pass");
    expect(parse?.satisfiedBy).toContain("ev-parse");
    // Security negatives still cannot pass on parser evidence alone.
    const noProcess = report.requirements.find(
      (requirement) => requirement.id === "S3M8.SECURITY.NO_PROCESS",
    );
    expect(noProcess?.status).toBe("incomplete");
  });

  it("passes standard-library requirements when their resolved evidence kinds match", () => {
    const records = [
      evidence({ kind: "review_result", id: "ev-review" }),
      evidence({ kind: "validation_result", id: "ev-validation" }),
    ];
    const report = createAcceptanceEvaluator().evaluate({
      manifest,
      evidence: records,
      acceptance: [],
    });
    const noProcess = report.requirements.find(
      (requirement) => requirement.id === "S3M8.SECURITY.NO_PROCESS",
    );
    // STANDARD.NO_PROCESS_EXECUTION resolves to validation_result/review_result/workspace_read.
    expect(noProcess?.status).toBe("pass");
    expect(noProcess?.satisfiedBy).toContain("ev-review");
  });

  it("passes via a host-verified linked criterion, and fails on a failed criterion", () => {
    const linked = createMilestoneManifest({
      id: "S3M9",
      title: "t",
      goal: "g",
      acceptance: [
        {
          id: "S3M9.X",
          description: "x",
          criterionId: "crit-x",
        },
      ],
    });
    const evaluator = createAcceptanceEvaluator();
    const satisfied: AcceptanceState[] = [
      {
        criterionId: "crit-x",
        description: "x",
        verificationKind: "deterministic",
        status: "satisfied",
        verifiedBy: "ev-1",
        note: null,
      },
    ];
    const pass = evaluator.evaluate({ manifest: linked, evidence: [], acceptance: satisfied });
    expect(pass.requirements[0]?.status).toBe("pass");
    expect(pass.passed).toBe(true);

    const failed: AcceptanceState[] = [
      {
        criterionId: "crit-x",
        description: "x",
        verificationKind: "deterministic",
        status: "failed",
        verifiedBy: null,
        note: "n",
      },
    ];
    const fail = evaluator.evaluate({ manifest: linked, evidence: [], acceptance: failed });
    expect(fail.requirements[0]?.status).toBe("fail");
    expect(fail.passed).toBe(false);
  });

  it("optional requirements become not_applicable when their linked criterion is absent", () => {
    const optional = createMilestoneManifest({
      id: "S3M9",
      title: "t",
      goal: "g",
      acceptance: [
        { id: "S3M9.X", description: "x", criterionId: "crit-x", optional: true },
        { id: "S3M9.Y", description: "y", evidenceKinds: ["workspace_read"] },
      ],
    });
    const report = createAcceptanceEvaluator().evaluate({
      manifest: optional,
      evidence: [evidence({ kind: "workspace_read", id: "ev-read" })],
      acceptance: [],
    });
    expect(report.requirements.find((requirement) => requirement.id === "S3M9.X")?.status).toBe(
      "not_applicable",
    );
    expect(report.requirements.find((requirement) => requirement.id === "S3M9.Y")?.status).toBe(
      "pass",
    );
    expect(report.passed).toBe(true);
  });

  it("reports deterministic counts and overall state", () => {
    const report = createAcceptanceEvaluator().evaluate({ manifest, evidence: [], acceptance: [] });
    expect(report.manifestId).toBe("S3M8");
    expect(report.counts.total).toBe(report.requirements.length);
    expect(
      report.counts.pass +
        report.counts.fail +
        report.counts.incomplete +
        report.counts.not_applicable,
    ).toBe(report.counts.total);
  });
});
