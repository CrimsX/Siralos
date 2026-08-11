/**
 * Stage 3 milestone 9 behavior fixtures: bounded, revision-aware,
 * evidence-backed impact intelligence at the final observable boundary.
 *
 * Covered behaviors (milestone effect tests): impact analysis launches no
 * process and performs no mutation/checkpoint; a leaf change in a large
 * fixture yields a bounded neighborhood (never the whole project);
 * inheritance and instancing impact stay distinct; changed autoloads
 * broaden risk conservatively without claiming verified impact on every
 * surface; stale relationships are excluded and disclosed; the reviewer
 * receives a bounded manifest (unit-level in quality-stage.test.ts).
 */
import { afterEach, describe, expect, it } from "vitest";
import { mkdir, readdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { createGodotReviewContextTool } from "@solaris/adapters";
import { createBehaviorLoopHarness, type BehaviorLoopHarness } from "./behavior-harness.js";

async function writeWorkspaceFile(
  harness: BehaviorLoopHarness,
  path: string,
  content: string,
): Promise<void> {
  const full = join(harness.workspace.root, path);
  await mkdir(dirname(full), { recursive: true });
  await writeFile(full, content, "utf8");
}

const BASE_SCENE = `[gd_scene load_steps=2 format=3 uid="uid://base001"]\n\n[ext_resource type="Script" path="res://scripts/player/player.gd" id="1_p"]\n\n[node name="Player" type="CharacterBody2D"]\nscript = ExtResource("1_p")\n`;

function unrelatedScene(name: string): string {
  return `[gd_scene format=3]\n\n[node name="${name}" type="Node2D"]\n`;
}

const PROJECT_WITH_AUTOLOAD = `[application]\nconfig/name="impact-fixture"\nrun/main_scene="res://scenes/player.tscn"\n\n[autoload]\nGameState="*res://scripts/player/game_state.gd"\n`;

describe("Milestone 9 — impact intelligence final boundary", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness?.cleanup();
  });

  it("impact analysis spawns no process and performs no workspace mutation or checkpoint", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true });
    await writeWorkspaceFile(harness, "project.godot", PROJECT_WITH_AUTOLOAD);
    await writeWorkspaceFile(harness, "scripts/player/game_state.gd", "extends Node\n");
    await writeWorkspaceFile(harness, "scenes/base_player.tscn", BASE_SCENE);
    await writeWorkspaceFile(
      harness,
      "scenes/player.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://player1"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Player" instance=ExtResource("1_base")]\n`,
    );
    // Populate the relationship index through real inspection.
    await harness.intelligence!.inspectScene({ path: "scenes/player.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/base_player.tscn" });

    const before = (await readdir(harness.workspace.root, { recursive: true })).sort();
    const beforeCheckpoints = await harness.store.list();

    const tool = createGodotReviewContextTool(harness.intelligence!);
    const result = await tool.execute(
      {
        taskId: "task-impact",
        taskContractRevision: 1,
        changedPaths: ["scripts/player/player.gd"],
      },
      {},
    );
    expect(result.status).toBe("success");

    const after = (await readdir(harness.workspace.root, { recursive: true })).sort();
    const afterCheckpoints = await harness.store.list();
    expect(after).toEqual(before);
    expect(afterCheckpoints).toEqual(beforeCheckpoints);
    // No process tool exists in the session surface at all.
    expect(harness.tools().some((toolDef) => toolDef.definition.name.startsWith("process."))).toBe(
      false,
    );
    // No approval was ever requested.
    expect(harness.approvals()).toBe(0);
  });

  it("a leaf change in a large fixture yields a bounded neighborhood, not the whole project", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true });
    await writeWorkspaceFile(harness, "project.godot", PROJECT_WITH_AUTOLOAD);
    await writeWorkspaceFile(harness, "scripts/player/game_state.gd", "extends Node\n");
    await writeWorkspaceFile(harness, "scenes/base_player.tscn", BASE_SCENE);
    await writeWorkspaceFile(
      harness,
      "scenes/player.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://player1"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Player" instance=ExtResource("1_base")]\n`,
    );
    for (let index = 0; index < 8; index += 1) {
      await writeWorkspaceFile(
        harness,
        `scenes/unrelated_${index}.tscn`,
        unrelatedScene(`U${index}`),
      );
    }
    await harness.intelligence!.inspectScene({ path: "scenes/player.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/base_player.tscn" });

    const result = await harness.intelligence!.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/player/player.gd"],
    });
    expect(result.status).toBe("ok");
    const manifest = result.manifest!;
    const allPaths = [
      ...manifest.primaryChanges.map((surface) => surface.path),
      ...manifest.relatedSurfaces.flatMap((relation) => [relation.sourcePath, relation.targetPath]),
    ];
    // The bounded neighborhood: the attaching scene and the main scene
    // reachable through it — never the unrelated scenes.
    expect(allPaths).toContain("scenes/base_player.tscn");
    for (let index = 0; index < 8; index += 1) {
      expect(allPaths).not.toContain(`scenes/unrelated_${index}.tscn`);
    }
  });

  it("keeps inheritance and instancing impact distinct", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true });
    await writeWorkspaceFile(harness, "project.godot", PROJECT_WITH_AUTOLOAD);
    await writeWorkspaceFile(harness, "scripts/player/game_state.gd", "extends Node\n");
    await writeWorkspaceFile(harness, "scenes/base_player.tscn", BASE_SCENE);
    // child inherits base; level instances base; level also instances child.
    await writeWorkspaceFile(
      harness,
      "scenes/child.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://child01"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Child" instance=ExtResource("1_base")]\n`,
    );
    await writeWorkspaceFile(
      harness,
      "scenes/level.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://level01"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Level" instance=ExtResource("1_base")]\n`,
    );
    await harness.intelligence!.inspectScene({ path: "scenes/child.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/level.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/base_player.tscn" });

    const result = await harness.intelligence!.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scenes/base_player.tscn"],
    });
    const manifest = result.manifest!;
    const kinds = manifest.relatedSurfaces.map((relation) => relation.kind);
    expect(kinds).toContain("scene_inheritance");
    expect(kinds).toContain("scene_instancing");
    const inheritanceTargets = manifest.relatedSurfaces
      .filter((relation) => relation.kind === "scene_inheritance")
      .map((relation) => relation.targetPath);
    const instancingTargets = manifest.relatedSurfaces
      .filter((relation) => relation.kind === "scene_instancing")
      .map((relation) => relation.targetPath);
    expect(inheritanceTargets).toContain("scenes/child.tscn");
    expect(instancingTargets).toContain("scenes/level.tscn");
    expect(instancingTargets).toContain("scenes/child.tscn");
  });

  it("changed autoload broadens risk conservatively, never marking every surface impacted", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true });
    await writeWorkspaceFile(harness, "project.godot", PROJECT_WITH_AUTOLOAD);
    await writeWorkspaceFile(harness, "scripts/player/game_state.gd", "extends Node\n");
    await writeWorkspaceFile(harness, "scenes/base_player.tscn", BASE_SCENE);
    await writeWorkspaceFile(
      harness,
      "scenes/player.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://player1"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Player" instance=ExtResource("1_base")]\n`,
    );
    await harness.intelligence!.inspectScene({ path: "scenes/player.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/base_player.tscn" });

    const result = await harness.intelligence!.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/player/game_state.gd"],
    });
    const manifest = result.manifest!;
    expect(manifest.primaryChanges[0]!.kind).toBe("autoload");
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.AUTOLOAD_GLOBAL"),
    ).toBe(true);
    expect(
      manifest.validation.some(
        (recommendation) => recommendation.kind === "broader_repo_validation",
      ),
    ).toBe(true);
    // The unrelated/related scenes are NOT marked verified impacted by the
    // autoload change alone.
    expect(manifest.relatedSurfaces.length).toBe(0);
    expect(manifest.completeness).toBe("partial");
  });

  it("stale relationship evidence is excluded and disclosed, never current", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true });
    await writeWorkspaceFile(harness, "project.godot", PROJECT_WITH_AUTOLOAD);
    await writeWorkspaceFile(harness, "scripts/player/game_state.gd", "extends Node\n");
    await writeWorkspaceFile(harness, "scenes/base_player.tscn", BASE_SCENE);
    await writeWorkspaceFile(
      harness,
      "scenes/player.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://player1"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Player" instance=ExtResource("1_base")]\n`,
    );
    await harness.intelligence!.inspectScene({ path: "scenes/player.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/base_player.tscn" });

    // The attaching scene changes externally to rev_B after being parsed.
    await writeWorkspaceFile(
      harness,
      "scenes/base_player.tscn",
      BASE_SCENE.replace("uid://base001", "uid://base002"),
    );
    harness.revisions.invalidatePath("scenes/base_player.tscn");

    const result = await harness.intelligence!.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/player/player.gd"],
    });
    const manifest = result.manifest!;
    // The stale scene->script edge is not presented as current impact.
    expect(
      manifest.relatedSurfaces.some(
        (relation) =>
          relation.kind === "script_attachment" &&
          relation.targetPath === "scenes/base_player.tscn",
      ),
    ).toBe(false);
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.STALE_RELATIONSHIP"),
    ).toBe(true);
    expect(manifest.completeness).toBe("partial");
  });

  it("the review tool projects a bounded manifest into provider-visible output", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true });
    await writeWorkspaceFile(harness, "project.godot", PROJECT_WITH_AUTOLOAD);
    await writeWorkspaceFile(harness, "scripts/player/game_state.gd", "extends Node\n");
    await writeWorkspaceFile(harness, "scenes/base_player.tscn", BASE_SCENE);
    await writeWorkspaceFile(
      harness,
      "scenes/player.tscn",
      `[gd_scene load_steps=2 format=3 uid="uid://player1"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n\n[node name="Player" instance=ExtResource("1_base")]\n`,
    );
    await harness.intelligence!.inspectScene({ path: "scenes/player.tscn" });
    await harness.intelligence!.inspectScene({ path: "scenes/base_player.tscn" });
    // Read the changed script so its revision is bound (realistic: changed
    // files were read during the workflow).
    await harness.workspaceRead.execute({ path: "scripts/player/player.gd", mode: "exact" }, {});

    const tool = createGodotReviewContextTool(harness.intelligence!);
    const result = await tool.execute(
      {
        taskId: "task-impact",
        taskContractRevision: 1,
        changedPaths: ["scripts/player/player.gd"],
      },
      {},
    );
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const output = result.output as Record<string, unknown>;
    expect(output.completeness).toBe("complete");
    const related = output.relatedSurfaces as Array<Record<string, unknown>>;
    expect(related.some((relation) => relation.kind === "script_attachment")).toBe(true);
    const primary = (output.primaryChanges as Array<Record<string, unknown>>)[0]!;
    expect(primary.revision).toMatch(/^rev_[0-9a-f]{32}$/);
    // No scene/resource mutation capability appears anywhere.
    const toolNames = harness.tools().map((toolDef) => toolDef.definition.name);
    expect(toolNames.some((name) => name.includes("write") && name.includes("scene"))).toBe(false);
    expect(JSON.stringify(output)).not.toContain("unrelated");
  });
});
