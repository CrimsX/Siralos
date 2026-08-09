import { afterEach, describe, expect, it } from "vitest";
import {
  DEVELOP_OFFLINE_PROFILE,
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  QUALITY_LIMITS,
  type ApprovalReviewer,
  type CheckpointStore,
  type QualityValidationExecutor,
  type SolarisApplication,
  type ValidationPlanDiscovery,
  type ValidationRunOutcome,
} from "@solaris/core";
import { readFile } from "node:fs/promises";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
  createTempWorkspace,
  writeFixtureFiles,
  type TempWorkspace,
} from "../../tools/workspace/workspace-fixtures.js";
import { createWorkspaceReadTool } from "../../tools/workspace/workspace-read-tool.js";
import { createWorkspaceApplyTextChangesetTool } from "../../tools/workspace/mutations/workspace-apply-text-changeset-tool.js";
import { createDeterministicFakeProvider } from "../../providers/deterministic-fake-provider.js";
import { createGDScriptDevelopmentService } from "../development/gdscript-development-service.js";
import {
  createFakeDiagnosticsService,
  createFakeGitInspector,
  createFakeLanguageService,
  createWorkspaceFilePrimitives,
} from "../development/gdscript-development-testing.js";
import { createFakeChangeReviewer, type FakeReviewerScenario } from "./fake-change-reviewer.js";
import { runQualityStage, type QualityStageInput } from "./quality-stage-runner.js";
import type {
  GDScriptDevelopmentStatus,
  GodotGDScriptDiagnostic,
  QualityGateResult,
} from "@solaris/core";

const FIXTURE_PATH = "scripts/player/player.gd";
const FIXTURE_CONTENT =
  "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n";

interface ScriptedValidationControl {
  packageScripts: Readonly<Record<string, string>> | null;
  /** FIFO of scripted outcomes per run; falls back to the default. */
  outcomes: ValidationRunOutcome[];
  defaultStatus: ValidationRunOutcome["status"];
  defaultExitCode: number | null;
  runs: string[];
}

function createScriptedValidation(control: ScriptedValidationControl): {
  readonly discovery: ValidationPlanDiscovery;
  readonly executor: QualityValidationExecutor;
} {
  const discovery: ValidationPlanDiscovery = {
    discover: () => Promise.resolve({ packageScripts: control.packageScripts }),
  };
  const executor: QualityValidationExecutor = {
    run: (step) => {
      control.runs.push(step.id);
      const scripted = control.outcomes.shift();
      if (scripted !== undefined) {
        return Promise.resolve(scripted);
      }
      return Promise.resolve({
        step,
        status: control.defaultStatus,
        exitCode: control.defaultExitCode,
        summary:
          control.defaultStatus === "passed"
            ? "exited with code 0"
            : control.defaultStatus === "denied"
              ? "denied"
              : control.defaultStatus === "unavailable"
                ? "runner unavailable"
                : `exit ${control.defaultExitCode ?? "unknown"}`,
      });
    },
  };
  return { discovery, executor };
}

interface LoopHarness {
  readonly workspace: TempWorkspace;
  readonly store: CheckpointStore;
  readonly application: SolarisApplication;
  readonly approvals: () => number;
  readonly status: () => GDScriptDevelopmentStatus;
  readonly startWorkflow: (request: string) => Promise<void>;
  readonly gitStatus: ReturnType<typeof createFakeGitInspector>["statusResult"];
  readonly language: import("../development/gdscript-development-testing.js").FakeLanguageControl;
}

async function createLoopHarness(options: {
  readonly scenario: FakeReviewerScenario;
  readonly validation?: ScriptedValidationControl;
  readonly request?: string;
}): Promise<LoopHarness> {
  const workspace = await createTempWorkspace();
  await writeFixtureFiles(workspace.root, {
    "project.godot": '[application]\nconfig/name="fixture"\n',
    [FIXTURE_PATH]: FIXTURE_CONTENT,
  });
  const store = await createTempCheckpointStore(workspace.root);
  const languageFake = createFakeLanguageService();
  const parserFake = createFakeDiagnosticsService();
  const validation = createScriptedValidation(
    options.validation ?? {
      packageScripts: null,
      outcomes: [],
      defaultStatus: "passed",
      defaultExitCode: 0,
      runs: [],
    },
  );
  const fakeReviewer = createFakeChangeReviewer(
    options.scenario === "repair-resolved"
      ? { scenario: options.scenario, resolveAfterRounds: 1 }
      : { scenario: options.scenario },
  );
  let approvals = 0;
  const reviewer: ApprovalReviewer = {
    review(): Promise<{ type: "approve_once" }> {
      approvals += 1;
      return Promise.resolve({ type: "approve_once" });
    },
  };
  const gitFake = createFakeGitInspector();
  const development = createGDScriptDevelopmentService({
    workspaceRoot: workspace.root,
    platform: "linux",
    store,
    lock: { acquire: () => Promise.resolve(() => undefined) },
    language: languageFake.service,
    diagnostics: parserFake.service,
    git: gitFake.git,
    canApplyIdentityBound: true,
    primitives: createWorkspaceFilePrimitives(workspace.root),
    qualityStage: {
      reviewer: fakeReviewer.reviewer,
      validation,
    },
    idFactory: () => `wf-${Math.random().toString(36).slice(2, 8)}`,
    settling: { hardTimeoutMs: 1000, pollIntervalMs: 1 },
  });
  const tools = createToolRegistry([
    createWorkspaceReadTool(workspace.root),
    createWorkspaceApplyTextChangesetTool(development),
  ]);
  const application = createSolarisApplication({
    provider: createDeterministicFakeProvider(),
    tools,
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer,
    onProviderTurnCompleted: () => {
      development.completeFromProviderTurn();
    },
  });
  return {
    workspace,
    store,
    application,
    approvals: () => approvals,
    status: () => development.status(),
    startWorkflow: async (request: string): Promise<void> => {
      const prepared = await development.prepareStart(request);
      if (prepared.status !== "ready") {
        throw new Error(prepared.message);
      }
      const started = await development.start(prepared.workflowId, {
        approvedDigest: prepared.digest,
      });
      if (started.status !== "ready") {
        throw new Error(started.message);
      }
    },
    gitStatus: gitFake.statusResult,
    language: languageFake.control,
  };
}

async function drain(harness: LoopHarness, request: string): Promise<void> {
  for await (const _event of harness.application.sendPrompt(request)) {
    // drain the bounded provider/tool loop
  }
}

describe("development completion through the quality stage", () => {
  let harness: LoopHarness;
  afterEach(async () => {
    await harness?.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  it("reports clean development as ready only after all quality gates pass", async () => {
    harness = await createLoopHarness({ scenario: "clean" });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({ kind: "terminal", status: "completed" });
    const quality = harness.status().session?.quality;
    expect(quality?.status).toBe("passed");
    const gates = quality?.report?.gates ?? [];
    const ids = gates.map((gate) => gate.id);
    for (const required of [
      "approved-change-applied",
      "checkpoint-recorded",
      "scope-verified",
      "parser",
      "lsp-errors",
      "independent-review",
    ]) {
      expect(ids).toContain(required);
      expect(gates.find((gate) => gate.id === required)?.status).toBe("passed");
    }
    // No project test runner exists in the fixture: the required-validation
    // gate is honestly not_applicable, never an infrastructure failure.
    const validationGate = gates.find((gate) => gate.id === "required-validation");
    expect(validationGate?.status).toBe("not_applicable");
    expect(validationGate?.evidence.some((entry) => entry.kind === "no-project-test-runner")).toBe(
      true,
    );
    expect(gates.find((gate) => gate.id === "diff-metrics")?.classification).toBe("informational");
    expect(gates.find((gate) => gate.id === "warnings")?.classification).toBe("soft");
  });

  it("treats a warning-only change as ready with advisories, never as a false failure", async () => {
    harness = await createLoopHarness({ scenario: "clean" });
    await harness.startWorkflow("develop fixture");
    // Seed a newly introduced warning into the fresh post-edit session
    // (the pre-edit baseline session stays empty).
    harness.language.nextSessionDiagnostics.set(FIXTURE_PATH, [
      diagnostic("warning", "unused variable"),
    ]);
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({
      kind: "terminal",
      status: "completed_with_warnings",
    });
    const warningsGate = harness
      .status()
      .session?.quality?.report?.gates.find((gate) => gate.id === "warnings");
    expect(warningsGate?.status).toBe("advisory");
    expect(warningsGate?.evidence.some((entry) => entry.kind === "warning-introduced")).toBe(true);
    expect(harness.status().session?.quality?.status).toBe("passed_with_advisories");
  });

  it("keeps a Medium review finding advisory (ready with advisories)", async () => {
    harness = await createLoopHarness({ scenario: "medium" });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({
      kind: "terminal",
      status: "completed_with_warnings",
    });
    const quality = harness.status().session?.quality;
    expect(quality?.status).toBe("passed_with_advisories");
    expect(quality?.blockingFindings).toBe(0);
    expect(quality?.advisories).toBe(1);
  });

  it("blocks clean completion on an evidence-backed High finding and repairs through separate approval", async () => {
    harness = await createLoopHarness({
      scenario: "repair-resolved",
      request: "develop fixture with review repair",
    });
    await harness.startWorkflow("develop fixture with review repair");
    await drain(harness, "develop fixture with review repair");
    const status = harness.status().session;
    expect(status?.state).toEqual({ kind: "terminal", status: "completed" });
    // The repair was a separately approved change set.
    expect(harness.approvals()).toBe(2);
    const checkpoints = await harness.store.list();
    expect(checkpoints).toHaveLength(2);
    // The final report is the fresh post-repair review: clean.
    expect(status?.quality?.status).toBe("passed");
    expect(status?.quality?.reviewRoundsUsed).toBe(2);
  });

  it("exhausts the review-repair budget and reports completed_with_blocking_findings", async () => {
    harness = await createLoopHarness({
      scenario: "new-after-repair",
      request: "develop fixture with review repair exhaust",
    });
    await harness.startWorkflow("develop fixture with review repair exhaust");
    await drain(harness, "develop fixture with review repair exhaust");
    const status = harness.status().session;
    expect(status?.state).toEqual({
      kind: "terminal",
      status: "completed_with_blocking_findings",
    });
    expect(status?.quality?.status).toBe("blocking_findings");
    expect(status?.quality?.blockingFindings).toBeGreaterThan(0);
    expect(status?.quality?.repairRoundsUsed).toBe(QUALITY_LIMITS.maxReviewRepairRounds);
    // The approved source changes remain on disk.
    const onDisk = await readFile(`${harness.workspace.root}/${FIXTURE_PATH}`, "utf8");
    expect(onDisk).toContain("move_and_slide");
  });

  it("reports validation incomplete when the required test command is denied", async () => {
    harness = await createLoopHarness({
      scenario: "clean",
      validation: {
        packageScripts: { check: "npm run lint" },
        outcomes: [],
        defaultStatus: "denied",
        defaultExitCode: null,
        runs: [],
      },
    });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    const status = harness.status().session;
    expect(status?.state).toEqual({ kind: "terminal", status: "validation_failed" });
    expect(status?.quality?.status).toBe("validation_incomplete");
    const validationGate = status?.quality?.report?.gates.find(
      (gate) => gate.id === "required-validation",
    );
    expect(validationGate?.status).toBe("not_run");
    expect(validationGate?.evidence.some((entry) => entry.kind === "validation-denied")).toBe(true);
    // The source change remains.
    const onDisk = await readFile(`${harness.workspace.root}/${FIXTURE_PATH}`, "utf8");
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
  });

  it("reports validation incomplete when the required test runner is infrastructure-unavailable", async () => {
    harness = await createLoopHarness({
      scenario: "clean",
      validation: {
        packageScripts: { test: "vitest run" },
        outcomes: [],
        defaultStatus: "unavailable",
        defaultExitCode: null,
        runs: [],
      },
    });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({
      kind: "terminal",
      status: "validation_failed",
    });
    expect(harness.status().session?.quality?.status).toBe("validation_incomplete");
  });

  it("handles a missing test command honestly as not applicable, never an infrastructure failure", async () => {
    harness = await createLoopHarness({
      scenario: "clean",
      validation: {
        packageScripts: null,
        outcomes: [],
        defaultStatus: "passed",
        defaultExitCode: 0,
        runs: [],
      },
    });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({ kind: "terminal", status: "completed" });
    const validationGate = harness
      .status()
      .session?.quality?.report?.gates.find((gate) => gate.id === "required-validation");
    expect(validationGate?.status).toBe("not_applicable");
    expect(validationGate?.evidence.some((entry) => entry.kind === "no-project-test-runner")).toBe(
      true,
    );
  });

  it("blocks readiness when a required test exits nonzero", async () => {
    harness = await createLoopHarness({
      scenario: "clean",
      validation: {
        packageScripts: { check: "npm run lint" },
        outcomes: [],
        defaultStatus: "failed",
        defaultExitCode: 1,
        runs: [],
      },
    });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({
      kind: "terminal",
      status: "quality_gate_failed",
    });
    expect(harness.status().session?.quality?.status).toBe("failed");
  });

  it("blocks readiness when Git reports a change outside the approved change set", async () => {
    harness = await createLoopHarness({ scenario: "clean" });
    await harness.startWorkflow("develop fixture");
    // A file appears in Git status that was not present at workflow start
    // and is not part of the approved change set.
    (harness.gitStatus as { untracked: readonly string[] }).untracked = ["sneaky.txt"];
    await drain(harness, "develop fixture");
    expect(harness.status().session?.state).toEqual({
      kind: "terminal",
      status: "quality_gate_failed",
    });
    const scopeGate = harness
      .status()
      .session?.quality?.report?.gates.find((gate) => gate.id === "scope-verified");
    expect(scopeGate?.status).toBe("blocked");
    expect(scopeGate?.evidence.some((entry) => entry.kind === "git-unrelated-changes")).toBe(true);
  });

  it("runs the validation plan command exactly once through the executor", async () => {
    const validation: ScriptedValidationControl = {
      packageScripts: { check: "npm run lint && npm test" },
      outcomes: [],
      defaultStatus: "passed",
      defaultExitCode: 0,
      runs: [],
    };
    harness = await createLoopHarness({ scenario: "clean", validation });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    expect(validation.runs).toEqual(["npm-check"]);
    expect(harness.status().session?.state).toEqual({ kind: "terminal", status: "completed" });
  });

  it("keeps the quality report deterministic and evidence-complete", async () => {
    harness = await createLoopHarness({ scenario: "medium" });
    await harness.startWorkflow("develop fixture");
    await drain(harness, "develop fixture");
    const report = harness.status().session?.quality?.report;
    expect(report).not.toBeNull();
    for (const gate of report?.gates ?? []) {
      expect(gate.summary.length).toBeGreaterThan(0);
    }
    expect(report?.review?.findings[0]?.evidence.length).toBeGreaterThan(0);
    expect(report?.review?.findings[0]?.id).toMatch(/^[0-9a-f]{24}$/);
  });
});

function buildStageInput(overrides: Partial<QualityStageInput> = {}): QualityStageInput {
  const baseline: QualityStageInput = {
    developmentId: "dev-1",
    request: "add a heal method",
    engineVersion: "4.7.1-stable",
    changeSetId: "cs-1",
    files: [
      {
        path: FIXTURE_PATH,
        operation: "update",
        afterContent: FIXTURE_CONTENT.replace("move_and_slide()", "move_and_slide(Vector2.UP)"),
        unifiedDiff: "@@ -3,2 +3,2 @@\n-\tmove_and_slide()\n+\tmove_and_slide(Vector2.UP)",
      },
    ],
    evidence: {
      changeSetId: "cs-1",
      files: [],
      parser: { checkedFiles: 1, validFiles: 1, diagnostics: [] },
      lsp: { started: true, diagnosticCount: 0, diagnostics: [] },
      git: { available: false, changedFiles: [] },
      workspaceIntegrity: { verified: true, unexpectedChanges: [] },
    },
    checkpointIds: ["cp-1"],
    gitBaseline: null,
    gitCurrent: null,
    warningBaseline: { available: true, diagnostics: [] },
    lspDiagnostics: [],
    reviewer: {
      review: () => Promise.resolve({ status: "completed", findings: [], message: null }),
    },
    validation: {
      discovery: { discover: () => Promise.resolve({ packageScripts: null }) },
      executor: {
        run: (step) =>
          Promise.resolve({
            step,
            status: "passed",
            exitCode: 0,
            summary: "exited with code 0",
          }),
      },
    },
    previousFindingIds: [],
    reviewRound: 1,
    repairRoundsUsed: 0,
    maxRepairRounds: QUALITY_LIMITS.maxReviewRepairRounds,
  };
  return { ...baseline, ...overrides };
}

function diagnostic(
  severity: "error" | "warning",
  message: string,
  path: string = FIXTURE_PATH,
): GodotGDScriptDiagnostic {
  return {
    source: "godot-lsp",
    severity,
    path,
    line: 4,
    column: null,
    code: null,
    message,
    rawCategory: null,
  };
}

describe("deterministic quality gates (stage runner)", () => {
  it("blocks when a changed script fails parsing", async () => {
    const output = await runQualityStage(
      buildStageInput({
        evidence: {
          changeSetId: "cs-1",
          files: [],
          parser: { checkedFiles: 1, validFiles: 0, diagnostics: [] },
          lsp: { started: true, diagnosticCount: 0, diagnostics: [] },
          git: { available: false, changedFiles: [] },
          workspaceIntegrity: { verified: true, unexpectedChanges: [] },
        },
      }),
    );
    expect(output.report.status).toBe("failed");
    expect(gateOf(output.report.gates, "parser")?.status).toBe("blocked");
  });

  it("blocks on error-severity LSP diagnostics in changed files", async () => {
    const output = await runQualityStage(
      buildStageInput({
        evidence: {
          changeSetId: "cs-1",
          files: [],
          parser: { checkedFiles: 1, validFiles: 1, diagnostics: [] },
          lsp: {
            started: true,
            diagnosticCount: 1,
            diagnostics: [diagnostic("error", "identifier not declared")],
          },
          git: { available: false, changedFiles: [] },
          workspaceIntegrity: { verified: true, unexpectedChanges: [] },
        },
      }),
    );
    expect(output.report.status).toBe("failed");
    expect(gateOf(output.report.gates, "lsp-errors")?.status).toBe("blocked");
  });

  it("does not block on unchanged or pre-existing warnings in changed files", async () => {
    const output = await runQualityStage(
      buildStageInput({
        warningBaseline: {
          available: true,
          diagnostics: [diagnostic("warning", "pre-existing warning")],
        },
        lspDiagnostics: [diagnostic("warning", "pre-existing warning")],
      }),
    );
    expect(gateOf(output.report.gates, "warnings")?.status).toBe("passed");
    expect(output.report.status).toBe("passed");
  });

  it("reports a newly introduced warning as advisory", async () => {
    const output = await runQualityStage(
      buildStageInput({
        warningBaseline: { available: true, diagnostics: [] },
        lspDiagnostics: [diagnostic("warning", "newly introduced warning")],
      }),
    );
    expect(gateOf(output.report.gates, "warnings")?.status).toBe("advisory");
    expect(output.report.status).toBe("passed_with_advisories");
  });

  it("labels warning attribution uncertain when no baseline was captured", async () => {
    const output = await runQualityStage(
      buildStageInput({
        warningBaseline: { available: false, diagnostics: [] },
        lspDiagnostics: [diagnostic("warning", "mystery warning")],
      }),
    );
    const gate = gateOf(output.report.gates, "warnings");
    expect(gate?.status).toBe("advisory");
    expect(gate?.evidence.some((entry) => entry.kind === "warning-baseline-unavailable")).toBe(
      true,
    );
  });

  it("blocks on unexpected workspace changes and .godot leakage", async () => {
    const unexpected = await runQualityStage(
      buildStageInput({
        evidence: {
          changeSetId: "cs-1",
          files: [],
          parser: { checkedFiles: 1, validFiles: 1, diagnostics: [] },
          lsp: { started: true, diagnosticCount: 0, diagnostics: [] },
          git: { available: false, changedFiles: [] },
          workspaceIntegrity: { verified: false, unexpectedChanges: ["sneaky.txt"] },
        },
      }),
    );
    expect(gateOf(unexpected.report.gates, "scope-verified")?.status).toBe("blocked");
    expect(unexpected.report.status).toBe("failed");

    const leaked = await runQualityStage(
      buildStageInput({
        evidence: {
          changeSetId: "cs-1",
          files: [],
          parser: { checkedFiles: 1, validFiles: 1, diagnostics: [] },
          lsp: { started: true, diagnosticCount: 0, diagnostics: [] },
          git: { available: false, changedFiles: [] },
          workspaceIntegrity: {
            verified: false,
            unexpectedChanges: [".godot/editor/editor_layout.cfg"],
          },
        },
      }),
    );
    const scope = gateOf(leaked.report.gates, "scope-verified");
    expect(scope?.status).toBe("blocked");
    expect(scope?.evidence.some((entry) => entry.kind === "generated-leak")).toBe(true);
  });

  it("never reports passed when the review could not run", async () => {
    const malformed = await runQualityStage(
      buildStageInput({
        reviewer: {
          review: () =>
            Promise.resolve({ status: "failed", findings: [], message: "malformed output" }),
        },
      }),
    );
    expect(malformed.report.status).toBe("validation_incomplete");
    expect(gateOf(malformed.report.gates, "independent-review")?.status).toBe("not_run");

    const tooLarge = await runQualityStage(
      buildStageInput({
        reviewer: {
          review: () =>
            Promise.resolve({ status: "too_large", findings: [], message: "too large" }),
        },
      }),
    );
    expect(tooLarge.report.status).toBe("validation_incomplete");
  });

  it("reports cancelled when the review was cancelled", async () => {
    const output = await runQualityStage(
      buildStageInput({
        reviewer: {
          review: () =>
            Promise.resolve({ status: "cancelled", findings: [], message: "cancelled" }),
        },
      }),
    );
    expect(output.report.status).toBe("cancelled");
  });

  it("reports blocking findings for evidence-backed Critical/High review findings", async () => {
    const output = await runQualityStage(
      buildStageInput({
        reviewer: {
          review: () =>
            Promise.resolve({
              status: "completed",
              findings: [
                {
                  id: "f1",
                  severity: "high",
                  category: "correctness",
                  title: "health can exceed max_health",
                  path: FIXTURE_PATH,
                  line: 4,
                  evidence: "heal() adds without clamping",
                  impact: "health exceeds the maximum",
                  recommendation: "clamp the result",
                  confidence: "high",
                },
                {
                  id: "f2",
                  severity: "medium",
                  category: "maintainability",
                  title: "helper used once",
                  path: FIXTURE_PATH,
                  line: 5,
                  evidence: "single call site",
                  impact: "minor",
                  recommendation: "inline it",
                  confidence: "high",
                },
              ],
              message: null,
            }),
        },
      }),
    );
    expect(output.report.status).toBe("blocking_findings");
    expect(output.blockingFindings).toHaveLength(1);
  });

  it("keeps a low-confidence High finding advisory rather than silently blocking", async () => {
    const output = await runQualityStage(
      buildStageInput({
        reviewer: {
          review: () =>
            Promise.resolve({
              status: "completed",
              findings: [
                {
                  id: "f1",
                  severity: "high",
                  category: "correctness",
                  title: "suspected issue",
                  path: FIXTURE_PATH,
                  line: 4,
                  evidence: "uncertain observation",
                  impact: "unclear",
                  recommendation: "verify",
                  confidence: "low",
                },
              ],
              message: null,
            }),
        },
      }),
    );
    expect(output.blockingFindings).toHaveLength(0);
    expect(output.report.status).toBe("passed_with_advisories");
  });

  it("emits the bounded quality event sequence", async () => {
    const events: string[] = [];
    await runQualityStage(
      buildStageInput({
        emit: (event) => events.push(event.type),
      }),
    );
    expect(events[0]).toBe("quality_started");
    expect(events[events.length - 1]).toBe("quality_completed");
    const reviewStarted = events.indexOf("review_started");
    const reviewCompleted = events.indexOf("review_completed");
    expect(reviewStarted).toBeGreaterThan(0);
    expect(reviewCompleted).toBeGreaterThan(reviewStarted);
    expect(events.filter((type) => type === "quality_gate_completed")).toHaveLength(10);
  });
});

function gateOf(gates: readonly QualityGateResult[], id: string): QualityGateResult | undefined {
  return gates.find((gate) => gate.id === id);
}
