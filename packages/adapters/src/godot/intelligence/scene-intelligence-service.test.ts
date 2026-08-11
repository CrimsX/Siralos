import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  canonicalizeJson,
  createWorkspaceRevisionRegistry,
  sha256Hex,
  type GodotSceneEvidenceView,
} from "@solaris/core";
import { createGodotSceneIntelligence } from "./scene-intelligence-service.js";

const tempDirectories: string[] = [];

async function withWorkspace(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-scene-intel-"));
  tempDirectories.push(directory);
  return directory;
}

async function writeFiles(root: string, files: Record<string, string>): Promise<void> {
  for (const [path, content] of Object.entries(files)) {
    const full = join(root, path);
    await mkdir(join(full, ".."), { recursive: true });
    await writeFile(full, content);
  }
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
});

const FIXTURE_PROJECT = {
  "project.godot": `[application]\nconfig/name="Fixture"\nrun/main_scene="res://scenes/player.tscn"\n\n[autoload]\nGameState="*res://scripts/game_state.gd"\nStats="res://scripts/stats.gd"\n\n[input]\nmove_left={\n"deadzone": 0.2,\n"events": [Object(InputEventKey, "resource_local_to_scene": false, "keycode": 65)]\n}\n`,
  "scenes/base_player.tscn": `[gd_scene load_steps=2 format=3 uid="uid://base001"]\n\n[ext_resource type="Script" path="res://scripts/player.gd" id="1_p"]\n\n[node name="Player" type="CharacterBody2D"]\nscript = ExtResource("1_p")\n`,
  "scenes/weapon.tscn": `[gd_scene format=3]\n\n[node name="Weapon" type="Node2D"]\n`,
  "scenes/player.tscn": `[gd_scene load_steps=4 format=3 uid="uid://player1"]\n\n[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base_player.tscn" id="1_base"]\n[ext_resource type="PackedScene" path="res://scenes/weapon.tscn" id="2_wep"]\n\n[node name="Player" instance=ExtResource("1_base")]\n\n[node name="Weapon" parent="." instance=ExtResource("2_wep")]\n\n[node name="UI" type="CanvasLayer" parent="."]\n\n[connection signal="died" from="Player" to="UI" method="on_player_died"]\n`,
  "resources/player_stats.tres": `[gd_resource type="Resource" load_steps=2 format=3 uid="uid://stats01"]\n\n[ext_resource type="Script" path="res://scripts/player_stats.gd" id="1_s"]\n\n[resource]\nscript = ExtResource("1_s")\nmax_hp = 100\n`,
  "scripts/player.gd": "extends CharacterBody2D\nsignal died\n",
  "scripts/player_stats.gd": "extends Resource\n",
  "scripts/game_state.gd": "extends Node\n",
  "scripts/stats.gd": "extends Node\n",
};

describe("createGodotSceneIntelligence", () => {
  it("inspects a scene with revision identity, inheritance, instances, and scripts", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const observations: GodotSceneEvidenceView[] = [];
    const intelligence = createGodotSceneIntelligence({
      workspaceRoot: root,
      revisions,
      onInspection: (view) => observations.push(view),
    });

    const result = await intelligence.inspectScene({ path: "scenes/player.tscn" });
    expect(result.status).toBe("ok");
    expect(result.revision).toMatch(/^rev_[0-9a-f]{32}$/);
    expect(result.document?.status).toBe("complete");
    const model = result.document!.document!;
    expect(model.uid).toBe("uid://player1");
    expect(model.baseScene?.resolvedPath).toBe("scenes/base_player.tscn");
    expect(model.nodes[1]!.instance?.resolvedPath).toBe("scenes/weapon.tscn");
    expect(model.connections).toHaveLength(1);
    expect(result.tree?.root?.node.name).toBe("Player");
    expect(observations).toHaveLength(1);
    expect(observations[0]!.path).toBe("scenes/player.tscn");
    expect(observations[0]!.revision).toBe(result.revision);
  });

  it("inspects a .tres with type, uid, script, and properties", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const result = await intelligence.inspectResource({ path: "resources/player_stats.tres" });
    expect(result.status).toBe("ok");
    expect(result.document?.status).toBe("complete");
    const model = result.document!.document!;
    expect(model.type).toBe("Resource");
    expect(model.uid).toBe("uid://stats01");
    expect(model.script?.resolvedPath).toBe("scripts/player_stats.gd");
    expect(model.properties.find((property) => property.name === "max_hp")?.value).toEqual({
      kind: "integer",
      value: 100,
    });
  });

  it("returns bounded dependencies with cycle detection", async () => {
    const root = await withWorkspace();
    await writeFiles(root, {
      ...FIXTURE_PROJECT,
      "scenes/loop_a.tscn": `[gd_scene load_steps=2 format=3]\n[ext_resource type="PackedScene" path="res://scenes/loop_b.tscn" id="1_b"]\n[node name="A" instance=ExtResource("1_b")]\n`,
      "scenes/loop_b.tscn": `[gd_scene load_steps=2 format=3]\n[ext_resource type="PackedScene" path="res://scenes/loop_a.tscn" id="1_a"]\n[node name="B" instance=ExtResource("1_a")]\n`,
    });
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });

    const result = await intelligence.dependencies({ path: "scenes/player.tscn" });
    expect(result.status).toBe("ok");
    const kinds = result.edges.map((edge) => edge.kind);
    expect(kinds).toContain("scene_inherits");
    expect(kinds).toContain("scene_instances");
    expect(kinds).toContain("scene_uses_script");
    // Bounded traversal reached the base/weapon scenes.
    expect(result.filesVisited).toBeGreaterThanOrEqual(3);
    expect(result.truncatedDepth).toBe(false);
    expect(result.cycleDetected).toBe(false);

    const cycle = await intelligence.dependencies({ path: "scenes/loop_a.tscn" });
    expect(cycle.cycleDetected).toBe(true);
    expect(cycle.cyclePath).toBeDefined();
    expect(cycle.cyclePath![0]).toBe("scenes/loop_a.tscn");
  });

  it("enforces the dependency file-count bound", async () => {
    const root = await withWorkspace();
    const files: Record<string, string> = { ...FIXTURE_PROJECT };
    for (let index = 0; index < 70; index += 1) {
      files[`scenes/chain_${index}.tscn`] =
        `[gd_scene load_steps=2 format=3]\n[ext_resource type="PackedScene" path="res://scenes/chain_${index + 1}.tscn" id="1_c"]\n[node name="C${index}" instance=ExtResource("1_c")]\n`;
    }
    files["scenes/chain_70.tscn"] = `[gd_scene format=3]\n[node name="End" type="Node2D"]\n`;
    files["scenes/chain_entry.tscn"] =
      `[gd_scene load_steps=2 format=3]\n[ext_resource type="PackedScene" path="res://scenes/chain_0.tscn" id="1_c"]\n[node name="Entry" instance=ExtResource("1_c")]\n`;
    await writeFiles(root, files);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const result = await intelligence.dependencies({ path: "scenes/chain_entry.tscn" });
    expect(result.status).toBe("ok");
    expect(result.filesVisited).toBeLessThanOrEqual(64);
    // Either bound may trigger first for a 71-deep chain: the depth bound
    // stops at maxDependencyDepth, the file bound at maxDependencyFiles.
    expect(result.truncatedFiles || result.truncatedDepth).toBe(true);
  });

  it("marks referrers stale after the source file changes", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const first = await intelligence.inspectScene({ path: "scenes/player.tscn" });
    const firstRevision = first.revision!;
    // External modification to rev_B.
    await writeFile(
      join(root, "scenes/player.tscn"),
      FIXTURE_PROJECT["scenes/player.tscn"].replace("uid://player1", "uid://player2"),
      "utf8",
    );
    revisions.invalidatePath("scenes/player.tscn");
    const second = await intelligence.inspectScene({ path: "scenes/player.tscn" });
    expect(second.revision).not.toBe(firstRevision);
    // The stale derivation is replaced on reparse: the index holds only the
    // rev_B entries, and the A-derived relationship is no longer current.
    const deps = await intelligence.dependencies({ path: "scenes/player.tscn" });
    expect(deps.revision).toBe(second.revision);
    expect(deps.referrers.every((referrer) => !referrer.stale)).toBe(true);
  });

  it("resolves project relationships: main scene, autoloads, input actions", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const result = await intelligence.projectRelationships();
    expect(result.status).toBe("ok");
    expect(result.mainScene?.path).toBe("scenes/player.tscn");
    expect(result.mainScene?.exists).toBe(true);
    expect(result.mainScene?.revision).toMatch(/^rev_/);
    expect(result.autoloads).toHaveLength(2);
    const gameState = result.autoloads.find((autoload) => autoload.name === "GameState")!;
    expect(gameState.path).toBe("scripts/game_state.gd");
    expect(gameState.targetKind).toBe("script");
    expect(gameState.enabled).toBe(true);
    const moveLeft = result.inputActions.find((action) => action.name === "move_left")!;
    expect(moveLeft).toBeDefined();
    expect(moveLeft.deadzone).toBe(0.2);
    expect(moveLeft.eventCount).toBe(1);
    expect(moveLeft.eventTypes).toEqual(["InputEventKey"]);
  });

  it("reports no_project when project.godot is absent", async () => {
    const root = await withWorkspace();
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const result = await intelligence.projectRelationships();
    expect(result.status).toBe("no_project");
    expect(result.mainScene).toBeNull();
  });

  it("denies paths outside the workspace and unsupported kinds", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const outside = await intelligence.inspectScene({ path: "../evil.tscn" });
    expect(outside.status).toBe("denied");
    const wrongKind = await intelligence.inspectScene({ path: "scripts/player.gd" });
    expect(wrongKind.status).toBe("unsupported");
    const wrongResource = await intelligence.inspectResource({ path: "scenes/player.tscn" });
    expect(wrongResource.status).toBe("unsupported");
  });

  it("reports missing files honestly", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const result = await intelligence.inspectScene({ path: "scenes/missing.tscn" });
    expect(result.status).toBe("not_found");
    expect(result.document).toBeNull();
  });
});
