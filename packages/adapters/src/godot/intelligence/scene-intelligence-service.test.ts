import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  canonicalizeJson,
  createWorkspaceRevisionRegistry,
  sha256Hex,
  type GodotSceneEvidenceView,
} from "@siralos/core";
import { createGodotSceneIntelligence } from "./scene-intelligence-service.js";

const tempDirectories: string[] = [];

async function withWorkspace(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "siralos-scene-intel-"));
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

  it("surfaces a root read failure in the dependency query instead of an empty ok", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    const missing = await intelligence.dependencies({ path: "scenes/does_not_exist.tscn" });
    expect(missing.status).toBe("not_found");
    expect(missing.edges).toHaveLength(0);
    const denied = await intelligence.dependencies({ path: "../outside.tscn" });
    expect(denied.status).toBe("denied");
  });
});

describe("createGodotSceneIntelligence — review context (Stage 3 milestone 9)", () => {
  async function makeIntelligence(root: string) {
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const intelligence = createGodotSceneIntelligence({ workspaceRoot: root, revisions });
    // The relationship index is fed by inspection (S3.8): parse the
    // fixture scenes/resources before deriving impact.
    await intelligence.inspectScene({ path: "scenes/player.tscn" });
    await intelligence.inspectScene({ path: "scenes/base_player.tscn" });
    await intelligence.inspectScene({ path: "scenes/weapon.tscn" });
    await intelligence.inspectResource({ path: "resources/player_stats.tres" });
    return { revisions, intelligence };
  }

  function readScriptRevision(
    revisions: ReturnType<typeof createWorkspaceRevisionRegistry>,
    path: string,
    content: string,
  ): void {
    revisions.issue(path, sha256Hex(content));
  }

  it("derives a bounded manifest for a changed script: attachments, signals, candidate tests", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { ...FIXTURE_PROJECT, "tests/player_test.gd": "extends SceneTree\n" });
    const { revisions, intelligence } = await makeIntelligence(root);
    readScriptRevision(revisions, "scripts/player.gd", FIXTURE_PROJECT["scripts/player.gd"]);
    const result = await intelligence.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/player.gd"],
    });
    expect(result.status).toBe("ok");
    const manifest = result.manifest!;
    expect(manifest.primaryChanges).toHaveLength(1);
    expect(manifest.primaryChanges[0]!.path).toBe("scripts/player.gd");
    expect(manifest.primaryChanges[0]!.revision).toMatch(/^rev_/);
    // Script attachment: base_player.tscn directly attaches the changed
    // script; player.tscn reaches it through inheritance at second order.
    expect(
      manifest.relatedSurfaces.some(
        (relation) =>
          relation.kind === "script_attachment" &&
          relation.targetPath === "scenes/base_player.tscn",
      ),
    ).toBe(true);
    expect(
      manifest.relatedSurfaces.some(
        (relation) =>
          relation.kind === "scene_inheritance" && relation.targetPath === "scenes/player.tscn",
      ),
    ).toBe(true);
    // Serialized signal connection is surfaced honestly (scene-local).
    const signals = manifest.relatedSurfaces.filter(
      (relation) => relation.kind === "signal_connection",
    );
    expect(signals.length).toBeGreaterThan(0);
    expect(signals[0]!.note).toContain("died");
    // Candidate test surface by convention, never verified.
    const tests = manifest.relatedSurfaces.filter((relation) => relation.kind === "test_covers");
    expect(tests.length).toBe(1);
    expect(tests[0]!.confidence).toBe("candidate");
    expect(tests[0]!.targetPath).toBe("tests/player_test.gd");
    // Regression areas and validation reflect the observed impact.
    expect(manifest.regressionAreas.some((area) => area.id === "REGRESSION.SCRIPT_BEHAVIOR")).toBe(
      true,
    );
    expect(
      manifest.validation.some(
        (recommendation) =>
          recommendation.kind === "gdscript_check_only" &&
          recommendation.priority === "required_now",
      ),
    ).toBe(true);
    expect(
      manifest.validation.some((recommendation) => recommendation.kind === "runtime_validation"),
    ).toBe(true);
  });

  it("keeps inheritance impact distinct from instancing impact", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const { intelligence } = await makeIntelligence(root);
    const result = await intelligence.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scenes/base_player.tscn"],
    });
    const manifest = result.manifest!;
    const kinds = manifest.relatedSurfaces.map((relation) => relation.kind);
    expect(kinds).toContain("scene_inheritance");
    expect(kinds).toContain("scene_instancing");
    expect(
      manifest.relatedSurfaces.find((relation) => relation.kind === "scene_inheritance")
        ?.targetPath,
    ).toBe("scenes/player.tscn");
    // The base scene is instanced by player.tscn (direct) and weapon.tscn
    // is reached through it at second order — inheritance and instancing
    // stay distinct kinds with distinct neighborhoods.
    const instancingTargets = manifest.relatedSurfaces
      .filter((relation) => relation.kind === "scene_instancing")
      .map((relation) => relation.targetPath);
    expect(instancingTargets).toContain("scenes/player.tscn");
    expect(instancingTargets).toContain("scenes/weapon.tscn");
    expect(
      manifest.regressionAreas.some((area) => area.id === "REGRESSION.SCENE_INHERITANCE"),
    ).toBe(true);
    expect(
      manifest.regressionAreas.some((area) => area.id === "REGRESSION.SCENE_INSTANTIATION"),
    ).toBe(true);
  });

  it("surfaces autoload reach conservatively for a changed autoload script", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const { intelligence } = await makeIntelligence(root);
    const result = await intelligence.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/game_state.gd"],
    });
    const manifest = result.manifest!;
    expect(manifest.primaryChanges[0]!.kind).toBe("autoload");
    expect(manifest.primaryChanges[0]!.note).toContain("GameState");
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.AUTOLOAD_GLOBAL"),
    ).toBe(true);
    expect(
      manifest.validation.some(
        (recommendation) => recommendation.kind === "broader_repo_validation",
      ),
    ).toBe(true);
    // Global reach is a risk signal, never verified impact on every scene.
    expect(
      manifest.relatedSurfaces.some((relation) => relation.targetPath === "scenes/weapon.tscn"),
    ).toBe(false);
    expect(manifest.completeness).toBe("partial");
  });

  it("never presents stale relationships as current impact", async () => {
    const root = await withWorkspace();
    await writeFiles(root, FIXTURE_PROJECT);
    const { revisions, intelligence } = await makeIntelligence(root);
    // The scene that attached the script changes to rev_B after parsing.
    await writeFile(
      join(root, "scenes/player.tscn"),
      FIXTURE_PROJECT["scenes/player.tscn"].replace("uid://player1", "uid://player2"),
      "utf8",
    );
    revisions.invalidatePath("scenes/player.tscn");
    const result = await intelligence.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/player.gd"],
    });
    const manifest = result.manifest!;
    // The stale scene->script edge is excluded from current impact and
    // surfaced as a diagnostic.
    expect(
      manifest.relatedSurfaces.some(
        (relation) =>
          relation.kind === "script_attachment" && relation.targetPath === "scenes/player.tscn",
      ),
    ).toBe(false);
    expect(
      manifest.diagnostics.some((diagnostic) => diagnostic.code === "IMPACT.STALE_RELATIONSHIP"),
    ).toBe(true);
    expect(manifest.completeness).toBe("partial");
  });

  it("keeps unrelated project surfaces out of the impact neighborhood", async () => {
    const root = await withWorkspace();
    await writeFiles(root, {
      ...FIXTURE_PROJECT,
      "scenes/unrelated_a.tscn": `[gd_scene format=3]\n\n[node name="A" type="Node2D"]\n`,
      "scenes/unrelated_b.tscn": `[gd_scene format=3]\n\n[node name="B" type="Node2D"]\n`,
    });
    const { intelligence } = await makeIntelligence(root);
    const result = await intelligence.reviewContext({
      taskId: "task-impact",
      taskContractRevision: 1,
      changedPaths: ["scripts/player.gd"],
    });
    const manifest = result.manifest!;
    const allPaths = [
      ...manifest.primaryChanges.map((surface) => surface.path),
      ...manifest.relatedSurfaces.flatMap((relation) => [relation.sourcePath, relation.targetPath]),
    ];
    expect(allPaths).toContain("scenes/player.tscn");
    expect(allPaths).not.toContain("scenes/unrelated_a.tscn");
    expect(allPaths).not.toContain("scenes/unrelated_b.tscn");
  });
});
