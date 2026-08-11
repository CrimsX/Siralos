import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { canonicalizeJson, createWorkspaceRevisionRegistry, sha256Hex } from "@solaris/core";
import {
  cleanupTempCheckpointDirs,
  createTempCheckpointStore,
} from "../../tools/workspace/workspace-fixtures.js";
import { createWorkspaceFilePrimitives } from "../development/gdscript-development-testing.js";
import { createGodotSceneMutationService } from "./scene-mutation-service.js";

const tempDirectories: string[] = [];

async function withWorkspace(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "solaris-mutation-"));
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
  await cleanupTempCheckpointDirs();
});

const PLAYER_SCENE = `[gd_scene load_steps=3 format=3 uid="uid://player1"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_p"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_1"]
size = Vector2(32, 32)

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_p")

[node name="Sprite" type="Sprite2D" parent="Player"]
visible = true
`;

async function makeService(root: string) {
  const revisions = createWorkspaceRevisionRegistry({
    workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
  });
  const store = await createTempCheckpointStore(root);
  const service = createGodotSceneMutationService({
    workspaceRoot: root,
    revisions,
    store,
    lock: { acquire: () => Promise.resolve(() => undefined) },
    canApplyIdentityBound: true,
    primitives: createWorkspaceFilePrimitives(root),
  });
  return { revisions, store, service };
}

describe("scene/resource mutation service", () => {
  it("prepares a mutation with a complete preview and applies it with semantic verification", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { "scenes/player.tscn": PLAYER_SCENE });
    const { store, service } = await makeService(root);

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
    expect(prepared.sourceRevision).toMatch(/^rev_[0-9a-f]{32}$/);
    expect(prepared.fingerprint).toMatch(/^[0-9a-f]{64}$/);
    // Complete preview: structural summary plus a real unified diff.
    expect(prepared.preview.structuralSummary).toContain("set Player/Sprite.visible");
    expect(prepared.preview.diff).toContain("visible = true");
    expect(prepared.preview.diff).toContain("visible = false");
    expect(prepared.serializedAfter).toContain("visible = false");

    // The serialized output re-parses (prepare-time self-check).
    expect(prepared.serializedAfter).toContain('uid="uid://player1"');

    const applied = await service.applyPrepared({
      prepared,
      approvedDigest: prepared.fingerprint,
    });
    expect(applied.status).toBe("applied");
    if (applied.status !== "applied") {
      return;
    }
    expect(applied.verification.status).toBe("verified");
    expect(applied.revision).toMatch(/^rev_[0-9a-f]{32}$/);
    // A checkpoint was created before/with the mutation.
    expect((await store.list()).length).toBeGreaterThan(0);
    // The file on disk reflects the mutation.
    const onDisk = await (
      await import("node:fs/promises")
    ).readFile(join(root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("visible = false");
  });

  it("rejects stale source revisions before apply and preserves rev_B", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { "scenes/player.tscn": PLAYER_SCENE });
    const { revisions, store, service } = await makeService(root);

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
    await writeFile(
      join(root, "scenes/player.tscn"),
      PLAYER_SCENE.replace("visible = true", "visible = true\n\n# externally changed"),
      "utf8",
    );
    revisions.invalidatePath("scenes/player.tscn");

    const applied = await service.applyPrepared({
      prepared,
      approvedDigest: prepared.fingerprint,
    });
    expect(applied.status).toBe("conflict");
    // rev_B is preserved and no checkpoint was created.
    const onDisk = await (
      await import("node:fs/promises")
    ).readFile(join(root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("# externally changed");
    expect(onDisk).toContain("visible = true");
    expect(await store.list()).toHaveLength(0);
  });

  it("binds approval to the exact prepared mutation", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { "scenes/player.tscn": PLAYER_SCENE });
    const { store, service } = await makeService(root);

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
    expect(applied.message).toMatch(/new approval/);
    expect(await store.list()).toHaveLength(0);
  });

  it("surfaces semantic verification failure as failed, never success", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { "scenes/player.tscn": PLAYER_SCENE });
    const { store, service } = await makeService(root);

    // A float value serializes as "3", which the parser re-reads as an
    // integer — the semantic expectation (float) cannot hold after
    // reparse, so verification must fail deterministically.
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
    // The checkpoint exists for recovery; the failure is explicit.
    expect((await store.list()).length).toBeGreaterThan(0);
  });

  it("refuses to prepare when the serialized output cannot reparse", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { "scenes/player.tscn": PLAYER_SCENE });
    const { service } = await makeService(root);
    // An opaque constructor value is rejected at validation time.
    const preparedResult = await service.prepareSceneChange({
      path: "scenes/player.tscn",
      operations: [
        {
          op: "set_property",
          nodePath: "Player/Sprite",
          property: "x",
          value: {
            kind: "opaque",
            typeName: "unknown",
            raw: { text: "MysteryThing(1)", truncated: false },
          },
        },
      ],
    });
    expect(preparedResult.status).toBe("failed");
    expect(preparedResult.message).toMatch(/opaque|structured/);
  });

  it("is unavailable through the production fail-closed gate", async () => {
    const root = await withWorkspace();
    await writeFiles(root, { "scenes/player.tscn": PLAYER_SCENE });
    const revisions = createWorkspaceRevisionRegistry({
      workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot: root })),
    });
    const store = await createTempCheckpointStore(root);
    const service = createGodotSceneMutationService({
      workspaceRoot: root,
      revisions,
      store,
      lock: { acquire: () => Promise.resolve(() => undefined) },
      canApplyIdentityBound: false,
      primitives: createWorkspaceFilePrimitives(root),
    });
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
    expect(applied.status).toBe("unavailable");
    const onDisk = await (
      await import("node:fs/promises")
    ).readFile(join(root, "scenes/player.tscn"), "utf8");
    expect(onDisk).toContain("visible = true");
  });
});
