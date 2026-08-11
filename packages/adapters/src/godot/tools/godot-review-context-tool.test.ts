import { describe, expect, it } from "vitest";
import {
  validateReviewContextManifest,
  type GodotSceneIntelligence,
  type ReviewContextManifest,
} from "@solaris/core";
import { createGodotReviewContextTool } from "./godot-review-context-tool.js";

const REV = "rev_".padEnd(36, "a");

function manifest(): ReviewContextManifest {
  return validateReviewContextManifest({
    taskId: "task-impact",
    taskContractRevision: 1,
    primaryChanges: [
      {
        path: "scripts/player.gd",
        kind: "script",
        revision: REV,
        confidence: "verified",
        evidence: "impact:changed-surface",
      },
    ],
    relatedSurfaces: [
      {
        kind: "script_attachment",
        sourcePath: "scripts/player.gd",
        targetPath: "scenes/player.tscn",
        sourceRevision: REV,
        targetRevision: REV,
        confidence: "verified",
        evidence: "index:scene_uses_script",
      },
    ],
    regressionAreas: [
      {
        id: "REGRESSION.SCRIPT_BEHAVIOR",
        title: "Scene script behavior",
        reason: "1 related surface via script_attachment.",
        surfaces: ["scenes/player.tscn"],
      },
    ],
    validation: [
      {
        kind: "gdscript_check_only",
        priority: "required_now",
        rationale: "Script changed.",
        surfaces: ["scripts/player.gd"],
      },
    ],
    evidence: ["impact:changed-surface"],
    completeness: "complete",
    diagnostics: [],
  });
}

function fakeIntelligence(result: {
  status: "ok" | "failed";
  message?: string | null;
  manifest?: ReviewContextManifest | null;
}): GodotSceneIntelligence {
  return {
    inspectScene: () => Promise.reject(new Error("unused")),
    inspectResource: () => Promise.reject(new Error("unused")),
    dependencies: () => Promise.reject(new Error("unused")),
    projectRelationships: () => Promise.reject(new Error("unused")),
    reviewContext: () =>
      Promise.resolve({
        status: result.status,
        message: result.message ?? null,
        manifest: result.manifest ?? null,
      }),
    support: () => ({ state: "ready" }),
  };
}

describe("createGodotReviewContextTool", () => {
  it("projects a bounded structured manifest with completeness disclosure", async () => {
    const tool = createGodotReviewContextTool(
      fakeIntelligence({ status: "ok", manifest: manifest() }),
    );
    const result = await tool.execute(
      { taskId: "task-impact", taskContractRevision: 1, changedPaths: ["scripts/player.gd"] },
      {},
    );
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect(result.summary).toContain("completeness complete");
    const output = result.output as Record<string, unknown>;
    expect(output.taskId).toBe("task-impact");
    expect(output.completeness).toBe("complete");
    const related = output.relatedSurfaces as Array<Record<string, unknown>>;
    expect(related[0]!.kind).toBe("script_attachment");
    expect(related[0]!.confidence).toBe("verified");
    expect((output.validation as Array<Record<string, unknown>>)[0]!.priority).toBe("required_now");
  });

  it("rejects invalid input without touching the intelligence", async () => {
    let called = false;
    const tool = createGodotReviewContextTool({
      ...fakeIntelligence({ status: "ok", manifest: null }),
      reviewContext: async () => {
        called = true;
        return Promise.resolve({ status: "ok", message: null, manifest: null });
      },
    });
    const result = await tool.execute({ changedPaths: [] }, {});
    expect(result.status).toBe("invalid_input");
    expect(called).toBe(false);
    const missingPaths = await tool.execute({ taskId: "t", taskContractRevision: 1 }, {});
    expect(missingPaths.status).toBe("invalid_input");
  });

  it("reports analysis failures truthfully", async () => {
    const tool = createGodotReviewContextTool(
      fakeIntelligence({ status: "failed", message: "impact analysis failed" }),
    );
    const result = await tool.execute(
      { taskId: "t", taskContractRevision: 1, changedPaths: ["scripts/player.gd"] },
      {},
    );
    if (result.status === "success") {
      throw new Error("expected a failure");
    }
    expect(result.message).toContain("impact analysis failed");
  });

  it("declares the read-only godot.inspect capability", () => {
    const tool = createGodotReviewContextTool(fakeIntelligence({ status: "ok", manifest: null }));
    expect(tool.capability).toBe("godot.inspect");
    expect(tool.definition.inputSchema.required).toEqual([
      "taskId",
      "taskContractRevision",
      "changedPaths",
    ]);
  });
});
