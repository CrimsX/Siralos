import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { canonicalizeJson, createWorkspaceRevisionRegistry, sha256Hex } from "@solaris/core";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
} from "../../tools/workspace/workspace-fixtures.js";
import { createGodotSceneMutationService } from "../scene-mutation/scene-mutation-service.js";
import {
  createFakeDiagnosticsService,
  createFakeLanguageService,
  createWorkspaceFilePrimitives,
} from "./gdscript-development-testing.js";
import { createUnifiedDevelopmentService } from "./unified-development-service.js";

const PLAYER_SCENE = `[gd_scene load_steps=3 format=3 uid="uid://player1"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_p"]

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_p")

[node name="Sprite" type="Sprite2D" parent="Player"]
visible = true
`;

const PLAYER_SCRIPT = `extends CharacterBody2D

var speed: float = 100.0

func _physics_process(_delta: float) -> void:
    pass
`;

const tempDirectories: string[] = [];

async function withWorkspace(): Promise<{ root: string; cleanup(): Promise<void> }> {
  const root = await mkdtemp(join(tmpdir(), "solaris-unified-"));
  tempDirectories.push(root);
  await writeFile(join(root, "project.godot"), '[application]\nconfig/name="unified"\n', "utf8");
  return {
    root,
    cleanup: async () => {
      await rm(root, { recursive: true, force: true });
    },
  };
}

async function writeFiles(root: string, files: Record<string, string>): Promise<void> {
  for (const [path, content] of Object.entries(files)) {
    const full = join(root, path);
    await mkdir(join(full, ".."), { recursive: true });
    await writeFile(full, content);
  }
}

function sha256OfText(text: string): string {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

async function makeService(root: string) {
  const revisions = createWorkspaceRevisionRegistry({
    workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
  });
  const store = await createTempCheckpointStore(root);
  const language = createFakeLanguageService();
  const parser = createFakeDiagnosticsService();
  const mutation = createGodotSceneMutationService({
    workspaceRoot: root,
    revisions,
    store,
    lock: { acquire: () => Promise.resolve(() => undefined) },
    canApplyIdentityBound: true,
    primitives: createWorkspaceFilePrimitives(root),
  });
  const impactCalls: {
    taskId: string;
    primaryChanges: readonly { readonly path: string; readonly operation: string }[];
  }[] = [];
  const service = createUnifiedDevelopmentService({
    workspaceRoot: root,
    store,
    lock: { acquire: () => Promise.resolve(() => undefined) },
    revisions,
    canApplyIdentityBound: true,
    primitives: createWorkspaceFilePrimitives(root),
    native: mutation,
    diagnostics: parser.service,
    language: language.service,
    impact: (input) => {
      impactCalls.push(input);
      return Promise.resolve(null);
    },
  });
  return { revisions, store, service, language, parser, impactCalls };
}

afterEach(async () => {
  for (const directory of tempDirectories.splice(0)) {
    await rm(directory, { recursive: true, force: true });
  }
  await cleanupTempCheckpointDirs();
});

describe("unified development service", () => {
  it("prepares a mixed script/native change set with a combined digest", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const { service } = await makeService(root);

    const result = await service.prepareUnified({
      request: "Add a sprint property and configure the scene value",
      targets: [
        {
          kind: "text",
          changes: {
            changes: [
              {
                operation: "edit",
                path: "scripts/player.gd",
                expectedSha256: sha256OfText(PLAYER_SCRIPT),
                replacements: [{ oldText: "100.0", newText: "140.0" }],
              },
            ],
          },
        },
        {
          kind: "scene",
          path: "scenes/player.tscn",
          operations: [
            {
              op: "set_property",
              nodePath: "Player",
              property: "speed",
              value: { kind: "float", value: 140.5 },
            },
          ],
        },
      ],
    });
    expect(
      result.status,
      result.status !== "ready" ? `prepare failed: ${result.message}` : "",
    ).toBe("ready");
    if (result.status !== "ready") {
      return;
    }
    expect(result.changeSet.targets).toHaveLength(2);
    expect(result.changeSet.surface).toBe("mixed");
    expect(result.changeSet.combinedDigest).toMatch(/^[0-9a-f]{64}$/);
    // Derived apply order is deterministic path order (scenes before scripts).
    expect(result.preview.files).toHaveLength(2);
    expect(result.preview.files[0]?.path).toBe("scenes/player.tscn");
    expect(result.preview.files[1]?.path).toBe("scripts/player.gd");
  });

  it("rejects an approval that does not match the prepared change set", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const { service } = await makeService(root);
    const prepared = await service.prepareUnified({
      request: "mixed change",
      targets: [
        {
          kind: "scene",
          path: "scenes/player.tscn",
          operations: [
            {
              op: "set_property",
              nodePath: "Player",
              property: "speed",
              value: { kind: "float", value: 140.5 },
            },
          ],
        },
      ],
    });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const applied = await service.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: "f".repeat(64),
    });
    expect(applied.status).toBe("conflict");
    const after = await readFile(join(root, "scenes/player.tscn"), "utf8");
    expect(after).toBe(PLAYER_SCENE);
  });

  it("applies a mixed change set end to end with per-surface verification", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const { store, service, impactCalls } = await makeService(root);

    const prepared = await service.prepareUnified({
      request: "Add a sprint property and configure the scene value",
      targets: [
        {
          kind: "text",
          changes: {
            changes: [
              {
                operation: "edit",
                path: "scripts/player.gd",
                expectedSha256: sha256OfText(PLAYER_SCRIPT),
                replacements: [{ oldText: "100.0", newText: "140.0" }],
              },
            ],
          },
        },
        {
          kind: "scene",
          path: "scenes/player.tscn",
          operations: [
            {
              op: "set_property",
              nodePath: "Player",
              property: "speed",
              value: { kind: "float", value: 140.5 },
            },
          ],
        },
      ],
    });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const applied = await service.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(
      applied.status,
      applied.status !== "applied"
        ? `apply failed: ${applied.message} native=${
            "nativeVerification" in applied ? JSON.stringify(applied.nativeVerification) : "n/a"
          }`
        : "",
    ).toBe("applied");
    if (applied.status !== "applied") {
      return;
    }
    // GDScript target applied exactly.
    const script = await readFile(join(root, "scripts/player.gd"), "utf8");
    expect(script).toContain("140.0");
    expect(script).not.toContain("100.0");
    // Native target applied and semantically verified.
    expect(applied.nativeVerification).toEqual([
      { path: "scenes/player.tscn", status: "verified", detail: null },
    ]);
    // One checkpoint batch covered every affected file.
    expect(applied.checkpointIds.length).toBeGreaterThanOrEqual(2);
    expect(await store.list()).toHaveLength(applied.checkpointIds.length);
    // Text gates produced evidence.
    expect(applied.parser).toEqual({ checkedFiles: 1, validFiles: 1 });
    expect(applied.lsp).toEqual({ errors: 0, warnings: 0 });
    // Consistency and impact.
    expect(applied.consistency.consistent).toBe(true);
    expect(impactCalls).toHaveLength(1);
    expect(impactCalls[0]?.primaryChanges.map((change) => change.path)).toEqual([
      "scenes/player.tscn",
      "scripts/player.gd",
    ]);
  });

  it("never mutates any target under a stale prepared change set", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const { service } = await makeService(root);

    const prepared = await service.prepareUnified({
      request: "mixed change",
      targets: [
        {
          kind: "text",
          changes: {
            changes: [
              {
                operation: "edit",
                path: "scripts/player.gd",
                expectedSha256: sha256OfText(PLAYER_SCRIPT),
                replacements: [{ oldText: "100.0", newText: "140.0" }],
              },
            ],
          },
        },
        {
          kind: "scene",
          path: "scenes/player.tscn",
          operations: [
            {
              op: "set_property",
              nodePath: "Player",
              property: "speed",
              value: { kind: "float", value: 140.5 },
            },
          ],
        },
      ],
    });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    // External change to one target before apply (rev_B).
    await writeFile(
      join(root, "scenes/player.tscn"),
      PLAYER_SCENE.replace("visible = true", "visible = false"),
    );
    const applied = await service.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(applied.status).toBe("conflict");
    // No target was mutated under the stale approval.
    const script = await readFile(join(root, "scripts/player.gd"), "utf8");
    expect(script).toBe(PLAYER_SCRIPT);
    const scene = await readFile(join(root, "scenes/player.tscn"), "utf8");
    expect(scene).toContain("visible = false");
    expect(scene).not.toContain("speed =");
  });

  it("fails closed as unavailable when the identity-bound gate is off", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const store = await createTempCheckpointStore(root);
    const mutation = createGodotSceneMutationService({
      workspaceRoot: root,
      revisions,
      store,
      lock: { acquire: () => Promise.resolve(() => undefined) },
      canApplyIdentityBound: false,
      primitives: createWorkspaceFilePrimitives(root),
    });
    const service = createUnifiedDevelopmentService({
      workspaceRoot: root,
      store,
      lock: { acquire: () => Promise.resolve(() => undefined) },
      revisions,
      canApplyIdentityBound: false,
      primitives: createWorkspaceFilePrimitives(root),
      native: mutation,
    });
    expect(service.support().state).toBe("unavailable");
    const prepared = await service.prepareUnified({
      request: "mixed change",
      targets: [
        {
          kind: "scene",
          path: "scenes/player.tscn",
          operations: [
            {
              op: "set_property",
              nodePath: "Player",
              property: "speed",
              value: { kind: "float", value: 140.5 },
            },
          ],
        },
      ],
    });
    expect(prepared.status).toBe("unavailable");
  });

  it("reports validation_failed when the GDScript gate reports errors", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const { service, parser } = await makeService(root);
    parser.control.resultsByPath.set("scripts/player.gd", {
      valid: false,
      diagnostics: [
        {
          source: "godot-check-only",
          severity: "error",
          path: "scripts/player.gd",
          line: 1,
          column: 1,
          code: "PARSE_ERROR",
          message: "syntax error",
          rawCategory: "error",
        },
      ],
    });
    const prepared = await service.prepareUnified({
      request: "mixed change",
      targets: [
        {
          kind: "text",
          changes: {
            changes: [
              {
                operation: "edit",
                path: "scripts/player.gd",
                expectedSha256: sha256OfText(PLAYER_SCRIPT),
                replacements: [{ oldText: "100.0", newText: "140.0" }],
              },
            ],
          },
        },
      ],
    });
    expect(prepared.status).toBe("ready");
    if (prepared.status !== "ready") {
      return;
    }
    const applied = await service.applyUnified({
      changeSetId: prepared.changeSet.id,
      approvedDigest: prepared.changeSet.combinedDigest,
    });
    expect(applied.status).toBe("validation_failed");
  });

  it("refuses scene/resource paths at the raw text boundary inside a unified set", async () => {
    const { root } = await withWorkspace();
    await writeFiles(root, {
      "scripts/player.gd": PLAYER_SCRIPT,
      "scenes/player.tscn": PLAYER_SCENE,
    });
    const { service } = await makeService(root);
    const prepared = await service.prepareUnified({
      request: "raw scene edit",
      targets: [
        {
          kind: "text",
          changes: {
            changes: [
              {
                operation: "edit",
                path: "scenes/player.tscn",
                expectedSha256: sha256OfText(PLAYER_SCENE),
                replacements: [{ oldText: "visible = true", newText: "visible = false" }],
              },
            ],
          },
        },
      ],
    });
    expect(prepared.status).toBe("invalid_input");
  });
});
