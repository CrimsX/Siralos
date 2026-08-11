/**
 * Stage 3 milestone 10 behavior fixtures: approved scene/resource
 * mutation at the final observable boundary.
 *
 * Covered behaviors (milestone effect tests): stale source rejects before
 * apply with rev_B preserved and the old approval never reused; approval
 * binds the exact prepared mutation; checkpoints are created before the
 * mutation; success is based on reparsed semantic state, not the write
 * alone; a deterministic verification failure is reported failed, never
 * success; the provider-visible schema has prepare-only tools and no raw
 * native write bypass; mutation context stays bounded.
 */
import { afterEach, describe, expect, it } from "vitest";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { createGodotPrepareSceneChangeTool } from "@solaris/adapters";
import { createBehaviorLoopHarness, type BehaviorLoopHarness } from "./behavior-harness.js";

const PLAYER_SCENE = `[gd_scene load_steps=3 format=3 uid="uid://player1"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_p"]

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_p")

[node name="Sprite" type="Sprite2D" parent="Player"]
visible = true

[node name="HUD" type="CanvasLayer" parent="."]
`;

function unrelatedScene(name: string): string {
  return `[gd_scene format=3]\n\n[node name="${name}" type="Node2D"]\n`;
}

async function writeWorkspaceFile(
  harness: BehaviorLoopHarness,
  path: string,
  content: string,
): Promise<void> {
  const full = join(harness.workspace.root, path);
  await mkdir(dirname(full), { recursive: true });
  await writeFile(full, content, "utf8");
}

describe("Milestone 10 — approved scene/resource mutation final boundary", () => {
  let harness: BehaviorLoopHarness;
  afterEach(async () => {
    await harness?.cleanup();
  });

  it("stale source rejects before apply, preserving rev_B and never reusing the old approval", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true, mutation: true });
    await writeWorkspaceFile(harness, "scenes/player.tscn", PLAYER_SCENE);
    const service = harness.mutation!;

    const preparedResult = await service.prepareSceneChange({
      path: "scenes/player.tscn",
      operations: [
        {
          op: "set_property",
          nodePath: "Player/Sprite",
          property: "visible",
          value: { kind: "boolean", value: false },
        },
      ],
    });
    expect(preparedResult.status).toBe("ready");
    if (preparedResult.status !== "ready") {
      return;
    }
    const prepared = preparedResult.prepared;
    // External modification to rev_B.
    await writeWorkspaceFile(
      harness,
      "scenes/player.tscn",
      PLAYER_SCENE.replace("visible = true", "visible = true\n# external rev_B"),
    );
    harness.revisions.invalidatePath("scenes/player.tscn");
    const applied = await service.applyPrepared({
      prepared,
      approvedDigest: prepared.fingerprint,
    });
    expect(applied.status).toBe("conflict");
    const onDisk = await readFile(join(harness.workspace.root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("# external rev_B");
    expect(onDisk).toContain("visible = true");
    expect(await harness.store.list()).toHaveLength(0);
    // No approval was ever requested for the mutation.
    expect(harness.approvals()).toBe(0);
  });

  it("approval binds the exact prepared mutation: a mismatched digest is rejected", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true, mutation: true });
    await writeWorkspaceFile(harness, "scenes/player.tscn", PLAYER_SCENE);
    const service = harness.mutation!;
    const preparedResult = await service.prepareSceneChange({
      path: "scenes/player.tscn",
      operations: [
        {
          op: "set_property",
          nodePath: "Player/Sprite",
          property: "visible",
          value: { kind: "boolean", value: false },
        },
      ],
    });
    expect(preparedResult.status).toBe("ready");
    if (preparedResult.status !== "ready") {
      return;
    }
    const applied = await service.applyPrepared({
      prepared: preparedResult.prepared,
      approvedDigest: "f".repeat(64),
    });
    expect(applied.status).toBe("conflict");
    const onDisk = await readFile(join(harness.workspace.root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("visible = true");
  });

  it("applies a valid mutation with checkpoint-before-mutation and semantic verification", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true, mutation: true });
    await writeWorkspaceFile(harness, "scenes/player.tscn", PLAYER_SCENE);
    const service = harness.mutation!;
    const preparedResult = await service.prepareSceneChange({
      path: "scenes/player.tscn",
      operations: [
        {
          op: "set_property",
          nodePath: "Player/Sprite",
          property: "visible",
          value: { kind: "boolean", value: false },
        },
      ],
    });
    expect(preparedResult.status).toBe("ready");
    if (preparedResult.status !== "ready") {
      return;
    }
    const applied = await service.applyPrepared({
      prepared: preparedResult.prepared,
      approvedDigest: preparedResult.prepared.fingerprint,
    });
    // Success is semantic: the reparsed revision matches the intent.
    expect(applied.status).toBe("applied");
    if (applied.status !== "applied") {
      return;
    }
    expect(applied.verification.status).toBe("verified");
    expect(applied.revision).toMatch(/^rev_[0-9a-f]{32}$/);
    // A checkpoint exists for recovery, created with the mutation.
    expect((await harness.store.list()).length).toBeGreaterThan(0);
    const onDisk = await readFile(join(harness.workspace.root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("visible = false");
    // Unrelated identities survive.
    expect(onDisk).toContain('uid="uid://player1"');
    expect(onDisk).toContain('id="1_p"');
  });

  it("reports a deterministic semantic verification failure as failed, never success", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true, mutation: true });
    await writeWorkspaceFile(harness, "scenes/player.tscn", PLAYER_SCENE);
    const service = harness.mutation!;
    // A float serializes as "3", which the parser re-reads as an integer:
    // the expectation (float) cannot hold after reparse.
    const preparedResult = await service.prepareSceneChange({
      path: "scenes/player.tscn",
      operations: [
        {
          op: "set_property",
          nodePath: "Player/Sprite",
          property: "offset",
          value: { kind: "float", value: 3 },
        },
      ],
    });
    expect(preparedResult.status).toBe("ready");
    if (preparedResult.status !== "ready") {
      return;
    }
    const applied = await service.applyPrepared({
      prepared: preparedResult.prepared,
      approvedDigest: preparedResult.prepared.fingerprint,
    });
    expect(applied.status).toBe("verification_failed");
    expect(applied.verification?.status).toBe("failed");
    // Recovery evidence exists (checkpoint), and the failure is explicit.
    expect((await harness.store.list()).length).toBeGreaterThan(0);
  });

  it("exposes prepare-only tools and no raw native write bypass", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true, mutation: true });
    await writeWorkspaceFile(harness, "scenes/player.tscn", PLAYER_SCENE);
    const names = harness.tools().map((toolDef) => toolDef.definition.name);
    expect(names).toContain("godot.prepare_scene_change");
    expect(names).toContain("godot.prepare_resource_change");
    // No scene/resource write or rewrite tool exists anywhere.
    expect(
      names.some(
        (name) =>
          (name.includes("scene") || name.includes("resource")) &&
          (name.includes("write") || name.includes("rewrite") || name.includes("apply")),
      ),
    ).toBe(false);
    // The raw change-set boundary still refuses scene/resource paths (the
    // S3.8 no-backdoor guarantee), and the prepare tool itself only
    // prepares: nothing is applied without the approval-bound apply.
    const tool = createGodotPrepareSceneChangeTool(harness.mutation!);
    const result = await tool.execute(
      {
        path: "scenes/player.tscn",
        operations: [
          {
            op: "set_property",
            nodePath: "Player/Sprite",
            property: "visible",
            value: { kind: "boolean", value: false },
          },
        ],
      },
      {},
    );
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const output = result.output as Record<string, unknown>;
    expect(output.fingerprint).toMatch(/^[0-9a-f]{64}$/);
    // The prepare tool never touched the workspace.
    const onDisk = await readFile(join(harness.workspace.root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("visible = true");
    expect(await harness.store.list()).toHaveLength(0);
  });

  it("keeps mutation context bounded to the target in a project with many unrelated assets", async () => {
    harness = await createBehaviorLoopHarness({ intelligence: true, mutation: true });
    await writeWorkspaceFile(harness, "scenes/player.tscn", PLAYER_SCENE);
    for (let index = 0; index < 6; index += 1) {
      await writeWorkspaceFile(
        harness,
        `scenes/unrelated_${index}.tscn`,
        unrelatedScene(`U${index}`),
      );
      await writeWorkspaceFile(
        harness,
        `resources/asset_${index}.tres`,
        `[gd_resource type="Resource" load_steps=1 format=3]\n\n[resource]\n`,
      );
    }
    const service = harness.mutation!;
    const preparedResult = await service.prepareSceneChange({
      path: "scenes/player.tscn",
      operations: [
        {
          op: "set_property",
          nodePath: "Player/Sprite",
          property: "visible",
          value: { kind: "boolean", value: false },
        },
      ],
    });
    expect(preparedResult.status).toBe("ready");
    if (preparedResult.status !== "ready") {
      return;
    }
    // The preview and prepared artifact reference only the target:
    // unrelated scenes/resources never enter the serialized output or diff.
    const serialized = preparedResult.prepared.serializedAfter;
    expect(serialized).not.toContain("unrelated_");
    expect(serialized).not.toContain("asset_");
    expect(preparedResult.prepared.preview.diff).not.toContain("unrelated_");
    expect(preparedResult.prepared.preview.diff).not.toContain("asset_");
  });
});
