import { describe, expect, it } from "vitest";
import type { CheckpointStore, GDScriptDevelopmentService } from "@siralos/core";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
  createTempWorkspace,
  writeFixtureFiles,
  type TempWorkspace,
} from "../../tools/workspace/workspace-fixtures.js";
import { createWorkspaceApplyTextChangesetTool } from "../../tools/workspace/mutations/workspace-apply-text-changeset-tool.js";
import { createGodotDevelopmentStatusTool } from "../tools/godot-development-status-tool.js";
import { createGDScriptDevelopmentService } from "./gdscript-development-service.js";
import {
  createFakeDiagnosticsService,
  createFakeLanguageService,
  createWorkspaceFilePrimitives,
  sha256Of,
} from "./gdscript-development-testing.js";

const PLAYER = "extends CharacterBody2D\n\nfunc _physics_process(delta):\n\tmove_and_slide()\n";

interface Fixture {
  readonly workspace: TempWorkspace;
  readonly store: CheckpointStore;
  readonly service: GDScriptDevelopmentService;
}

async function withFixture(
  canApplyIdentityBound: boolean,
  run: (fixture: Fixture) => Promise<void>,
): Promise<void> {
  const workspace = await createTempWorkspace();
  let store: CheckpointStore;
  try {
    await writeFixtureFiles(workspace.root, {
      "src/player/player.gd": PLAYER,
    });
    store = await createTempCheckpointStore(workspace.root);
    const languageFake = createFakeLanguageService();
    const parserFake = createFakeDiagnosticsService();
    const service = createGDScriptDevelopmentService({
      workspaceRoot: workspace.root,
      platform: "linux",
      store,
      lock: { acquire: () => Promise.resolve(() => undefined) },
      language: languageFake.service,
      diagnostics: parserFake.service,
      git: null,
      canApplyIdentityBound,
      primitives: createWorkspaceFilePrimitives(workspace.root),
      idFactory: () => `id-${Math.random().toString(36).slice(2, 8)}`,
      settling: { hardTimeoutMs: 1000, pollIntervalMs: 1 },
    });
    await run({ workspace, store, service });
  } finally {
    await workspace.cleanup();
    await cleanupTempCheckpointDirs();
  }
}

async function startWorkflow(service: GDScriptDevelopmentService): Promise<void> {
  const prepared = await service.prepareStart("request");
  if (prepared.status !== "ready") {
    throw new Error(prepared.message);
  }
  const started = await service.start(prepared.workflowId, {
    approvedDigest: prepared.digest,
  });
  if (started.status !== "ready") {
    throw new Error(started.message);
  }
}

const changeSetInput = {
  changes: [
    {
      operation: "edit" as const,
      path: "src/player/player.gd",
      expectedSha256: sha256Of(PLAYER),
      replacements: [{ oldText: "move_and_slide()", newText: "move_and_slide(Vector2.UP)" }],
    },
  ],
};

describe("workspace.apply_text_changeset tool", () => {
  it("is a reviewable prepared mutation tool with the workspace.write capability", async () => {
    await withFixture(true, async ({ service }) => {
      await startWorkflow(service);
      const tool = createWorkspaceApplyTextChangesetTool(service);
      expect(tool.kind).toBe("prepared_mutation");
      expect(tool.capability).toBe("workspace.write");
      expect(tool.definition.name).toBe("workspace.apply_text_changeset");
    });
  });

  it("prepares a ready change set with a complete preview and digest inside a workflow", async () => {
    await withFixture(true, async ({ service }) => {
      await startWorkflow(service);
      const tool = createWorkspaceApplyTextChangesetTool(service);
      const prepared = await tool.prepare(changeSetInput, {});
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      expect(prepared.preview.files).toHaveLength(1);
      expect(prepared.preview.truncated).toBe(false);
      expect(prepared.digest).toMatch(/^[0-9a-f]{64}$/);
      const applied = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      expect(applied.status).toBe("success");
      if (applied.status === "success") {
        const output = applied.output as Record<string, unknown>;
        expect((output["changedFiles"] as readonly unknown[]).length).toBe(1);
        expect(output["checkpointIds"]).toBeDefined();
      }
    });
  });

  it("fails closed outside a workflow before any approval", async () => {
    await withFixture(true, async ({ service }) => {
      const tool = createWorkspaceApplyTextChangesetTool(service);
      const prepared = await tool.prepare(
        { changes: [{ operation: "create", path: "x.gd", content: "extends Node\n" }] },
        {},
      );
      expect(prepared.status).toBe("failed");
      if (prepared.status === "failed") {
        expect(prepared.message).toContain("development workflow");
      }
    });
  });

  it("refuses preparation when the applier is unavailable (no approval requested)", async () => {
    await withFixture(false, async ({ service }) => {
      const tool = createWorkspaceApplyTextChangesetTool(service);
      const prepared = await tool.prepare(
        { changes: [{ operation: "create", path: "x.gd", content: "extends Node\n" }] },
        {},
      );
      expect(prepared.status).toBe("failed");
    });
  });

  it("does not allow reusing a consumed prepared handle", async () => {
    await withFixture(true, async ({ service }) => {
      await startWorkflow(service);
      const tool = createWorkspaceApplyTextChangesetTool(service);
      const prepared = await tool.prepare(changeSetInput, {});
      expect(prepared.status).toBe("ready");
      if (prepared.status !== "ready") {
        return;
      }
      await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      const second = await tool.apply(prepared.mutation, { approvedDigest: prepared.digest });
      expect(second.status).toBe("failed");
    });
  });
});

describe("godot.development_status tool", () => {
  it("is a read-only tool exposing bounded status", async () => {
    await withFixture(true, async ({ service }) => {
      const tool = createGodotDevelopmentStatusTool(service);
      expect(tool.definition.name).toBe("godot.development_status");
      const idle = await tool.execute({}, {});
      expect(idle.status).toBe("success");
      if (idle.status === "success") {
        expect((idle.output as Record<string, unknown>)["session"]).toBeNull();
      }
      await startWorkflow(service);
      const during = await tool.execute({}, {});
      if (during.status === "success") {
        const session = (during.output as Record<string, unknown>)["session"] as Record<
          string,
          unknown
        >;
        expect(session["state"]).toEqual({ kind: "active", phase: "investigating" });
      }
    });
  });

  it("rejects non-empty input", async () => {
    await withFixture(true, async ({ service }) => {
      const tool = createGodotDevelopmentStatusTool(service);
      const result = await tool.execute({ anything: true }, {});
      expect(result.status).toBe("invalid_input");
    });
  });
});
