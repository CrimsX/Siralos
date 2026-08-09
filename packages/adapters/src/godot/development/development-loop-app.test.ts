import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  DEVELOP_OFFLINE_PROFILE,
  createDefaultPolicy,
  createSolarisApplication,
  createToolRegistry,
  type ApprovalReviewer,
  type CheckpointStore,
  type SolarisApplication,
} from "@solaris/core";
import { createHash } from "node:crypto";
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
import { createGodotDevelopmentStatusTool } from "../tools/godot-development-status-tool.js";
import { createDeterministicFakeProvider } from "../../providers/deterministic-fake-provider.js";
import { createGDScriptDevelopmentService } from "./gdscript-development-service.js";
import {
  createFakeDiagnosticsService,
  createFakeGitInspector,
  createFakeLanguageService,
  createWorkspaceFilePrimitives,
  type FakeLanguageControl,
  type FakeParserControl,
} from "./gdscript-development-testing.js";

const FIXTURE_PATH = "scripts/player/player.gd";
const FIXTURE_CONTENT =
  "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n";

function sha256Of(text: string): string {
  return createHash("sha256").update(text).digest("hex");
}

interface LoopHarness {
  readonly workspace: TempWorkspace;
  readonly store: CheckpointStore;
  readonly application: SolarisApplication;
  readonly language: FakeLanguageControl;
  readonly parser: FakeParserControl;
  readonly approvals: () => number;
  readonly events: readonly string[];
  readonly status: () => import("@solaris/core").GDScriptDevelopmentStatus;
  readonly startWorkflow: (request: string) => Promise<void>;
}

async function createLoopHarness(options: { readonly repair?: boolean } = {}): Promise<LoopHarness> {
  const workspace = await createTempWorkspace();
  await writeFixtureFiles(workspace.root, {
    "project.godot": '[application]\nconfig/name="fixture"\n',
    [FIXTURE_PATH]: FIXTURE_CONTENT,
  });
  const store = await createTempCheckpointStore(workspace.root);
  const languageFake = createFakeLanguageService();
  const parserFake = createFakeDiagnosticsService();
  if (options.repair === true) {
    parserFake.control.queuedResults.push({
      path: FIXTURE_PATH,
      valid: false,
      diagnostics: [
        {
          source: "godot-check-only",
          severity: "error",
          path: FIXTURE_PATH,
          line: 4,
          column: 3,
          code: null,
          message: "Parse error: Unexpected token )",
          rawCategory: "SCRIPT ERROR",
        },
      ],
    });
  }
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
    idFactory: () => `wf-${Math.random().toString(36).slice(2, 8)}`,
    settling: { hardTimeoutMs: 1000, pollIntervalMs: 1 },
  });
  const tools = createToolRegistry([
    createWorkspaceReadTool(workspace.root),
    createWorkspaceApplyTextChangesetTool(development),
    createGodotDevelopmentStatusTool(development),
  ]);
  const events: string[] = [];
  const application = createSolarisApplication({
    provider: createDeterministicFakeProvider(),
    tools,
    policy: createDefaultPolicy("develop-offline"),
    profile: DEVELOP_OFFLINE_PROFILE,
    reviewer,
    onProviderTurnCompleted: () => {
      development.completeFromProviderTurn();
      events.push("provider-turn-completed");
    },
  });
  return {
    workspace,
    store,
    application,
    language: languageFake.control,
    parser: parserFake.control,
    approvals: () => approvals,
    events,
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
  };
}

describe("complete GDScript development loop through the application", () => {
  let harness: LoopHarness;
  beforeEach(async () => {
    harness = await createLoopHarness();
  });
  afterEach(async () => {
    await harness.workspace.cleanup();
    await cleanupTempCheckpointDirs();
  });

  it("runs investigate -> propose -> approve -> apply -> validate -> complete", async () => {
    await harness.startWorkflow("develop fixture");
    for await (const _event of harness.application.sendPrompt("develop fixture")) {
      // drain the bounded provider/tool loop
    }
    // The provider finished with a final turn; the workflow completed.
    expect(harness.events).toContain("provider-turn-completed");
    expect(harness.status().session?.state).toEqual({
      kind: "terminal",
      status: "completed",
    });
    // The fixture now contains exactly the approved change.
    const onDisk = await readFile(`${harness.workspace.root}/${FIXTURE_PATH}`, "utf8");
    expect(onDisk).toContain("move_and_slide(Vector2.UP)");
    // Exactly one user approval was requested (the change set approval).
    expect(harness.approvals()).toBe(1);
    // The language session was suspended and recreated fresh.
    expect(harness.language.startCount).toBeGreaterThanOrEqual(2);
    // A checkpoint was recorded for the edited file.
    const checkpoints = await harness.store.list();
    expect(checkpoints).toHaveLength(1);
    expect(checkpoints[0]?.operation).toBe("update");
    expect(checkpoints[0]?.after.sha256).toBe(sha256Of(onDisk));
    // The final result is bounded and truthful.
    const result = harness.status();
    expect(result.session?.validation).toBe("clean");
    expect(result.session?.appliedChangeSets).toBe(1);
  });

  it("runs the repair loop: broken edit -> parser error -> approved repair -> clean", async () => {
    const harness = await createLoopHarness({ repair: true });
    try {
      await harness.startWorkflow("develop fixture with repair");
      for await (const _event of harness.application.sendPrompt("develop fixture with repair")) {
        // drain
      }
      const onDisk = await readFile(`${harness.workspace.root}/${FIXTURE_PATH}`, "utf8");
      expect(onDisk).toContain("move_and_slide(Vector2.UP)");
      expect(onDisk).not.toContain("move_and_slide())");
      // Two change sets were approved (initial + repair).
      expect(harness.approvals()).toBe(2);
      // The repair consumed the queued parser outcome; the second
      // validation ran clean.
      expect(harness.parser.queuedResults).toHaveLength(0);
      const checkpoints = await harness.store.list();
      expect(checkpoints).toHaveLength(2);
      expect(harness.events).toContain("provider-turn-completed");
      expect(harness.status().session?.state).toEqual({
        kind: "terminal",
        status: "completed",
      });
    } finally {
      await harness.workspace.cleanup();
      await cleanupTempCheckpointDirs();
    }
  });
});
