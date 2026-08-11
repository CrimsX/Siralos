import { describe, expect, it } from "vitest";
import { parseGodotScene } from "../scene/scene-parser.js";
import { parseGodotResource } from "../scene/resource-parser.js";
import { expectedSemanticEffect, validateMutationOperations } from "./operations.js";
import { applySceneOperations, applyResourceOperations } from "./model-apply.js";
import { serializeScene, serializeResource, serializeVariantValue } from "./serializer.js";
import { verifySceneSemanticEffect, verifyResourceSemanticEffect } from "./verify.js";
import { createPreparedGodotMutation, computeMutationFingerprint } from "./prepared.js";

const REV = "rev_".padEnd(36, "a");

const PLAYER_SCENE = `[gd_scene load_steps=3 format=3 uid="uid://player1"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_p"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_1"]
size = Vector2(32, 32)

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_p")

[node name="Sprite" type="Sprite2D" parent="Player"]
visible = true

[node name="HUD" type="CanvasLayer" parent="."]

[connection signal="died" from="Player" to="HUD" method="on_player_died"]
`;

function parseScene(text: string) {
  const parsed = parseGodotScene(text, "scenes/player.tscn", { revision: REV });
  if (parsed.document === null) {
    throw new Error(
      `fixture failed to parse: ${parsed.diagnostics.map((d) => d.message).join("; ")}`,
    );
  }
  return parsed.document;
}

const STATS_RESOURCE = `[gd_resource type="Resource" load_steps=2 format=3 uid="uid://stats01"]

[ext_resource type="Script" path="res://scripts/player_stats.gd" id="1_s"]

[sub_resource type="Gradient" id="Gradient_1"]
offsets = PackedFloat32Array(0, 1)

[resource]
script = ExtResource("1_s")
max_hp = 100
`;

function parseResource(text: string) {
  const parsed = parseGodotResource(text, "resources/player_stats.tres", { revision: REV });
  if (parsed.document === null) {
    throw new Error(
      `fixture failed to parse: ${parsed.diagnostics.map((d) => d.message).join("; ")}`,
    );
  }
  return parsed.document;
}

describe("scene mutation — apply, serialize, reparse, verify", () => {
  it("sets a node property and verifies the reparsed semantics", () => {
    const model = parseScene(PLAYER_SCENE);
    const operations = validateMutationOperations([
      {
        op: "set_property",
        nodePath: "Player/Sprite",
        property: "visible",
        value: { kind: "boolean", value: false },
      },
    ]);
    const after = applySceneOperations(model, operations);
    const text = serializeScene(after);
    const reparsed = parseScene(text);
    const verification = verifySceneSemanticEffect(reparsed, expectedSemanticEffect(operations));
    expect(verification.status).toBe("verified");
    // The changed property is false on the reparsed model.
    const sprite = reparsed.nodes.find((node) => node.name === "Sprite")!;
    expect(sprite.properties.find((property) => property.name === "visible")?.value).toEqual({
      kind: "boolean",
      value: false,
    });
    // Unrelated identities are preserved: ext id, uid, node names.
    expect(reparsed.uid).toBe("uid://player1");
    expect(reparsed.externalResources[0]!.id).toBe("1_p");
    expect(reparsed.nodes.map((node) => node.name)).toEqual(["Player", "Sprite", "HUD"]);
    expect(reparsed.connections).toHaveLength(1);
  });

  it("removes a property and reports the verification failure when the effect is absent", () => {
    const model = parseScene(PLAYER_SCENE);
    const operations = validateMutationOperations([
      { op: "remove_property", nodePath: "Player/Sprite", property: "visible" },
    ]);
    const after = applySceneOperations(model, operations);
    const verification = verifySceneSemanticEffect(
      parseScene(serializeScene(after)),
      expectedSemanticEffect(operations),
    );
    expect(verification.status).toBe("verified");
    // A mismatched expectation (property should still be absent but we
    // check presence) surfaces as failed — never success.
    const wrong = verifySceneSemanticEffect(parseScene(PLAYER_SCENE), [
      {
        kind: "property_absent",
        nodePath: "Player/Sprite",
        property: "visible",
      },
    ]);
    expect(wrong.status).toBe("failed");
  });

  it("adds and removes nodes, cleaning descendants and dangling connections", () => {
    const model = parseScene(PLAYER_SCENE);
    const add = validateMutationOperations([
      { op: "add_node", name: "Camera", type: "Camera2D", parentPath: "." },
    ]);
    const added = applySceneOperations(model, add);
    const withCamera = parseScene(serializeScene(added));
    expect(withCamera.nodes.some((node) => node.name === "Camera")).toBe(true);
    const remove = validateMutationOperations([{ op: "remove_node", nodePath: "Player" }]);
    const removed = applySceneOperations(withCamera, remove);
    const after = parseScene(serializeScene(removed));
    // Player and its child Sprite are removed; the sibling HUD and the
    // added Camera survive.
    expect(after.nodes.map((node) => node.name)).toEqual(["HUD", "Camera"]);
    // The connection from the removed Player node is dropped too.
    expect(after.connections).toHaveLength(0);
  });

  it("changes the script attachment and signal connections", () => {
    const model = parseScene(PLAYER_SCENE);
    const operations = validateMutationOperations([
      { op: "set_script_attachment", nodePath: "Player", extResourceId: null },
      {
        op: "add_signal_connection",
        signal: "ready",
        from: "Player",
        to: "HUD",
        method: "on_ready",
      },
    ]);
    const after = parseScene(serializeScene(applySceneOperations(model, operations)));
    const verification = verifySceneSemanticEffect(after, expectedSemanticEffect(operations));
    expect(verification.status).toBe("verified");
    expect(after.nodes.find((node) => node.name === "Player")!.script).toBeUndefined();
    expect(
      after.connections.some(
        (connection) => connection.signal === "ready" && connection.method === "on_ready",
      ),
    ).toBe(true);
    // The original connection survives.
    expect(after.connections.some((connection) => connection.signal === "died")).toBe(true);
  });

  it("serialization is deterministic and preserves untouched raw formatting", () => {
    const model = parseScene(PLAYER_SCENE);
    const first = serializeScene(model);
    const second = serializeScene(model);
    expect(first).toBe(second);
    // Unchanged property raw text survives verbatim (no churn).
    expect(first).toContain("visible = true");
    expect(first).toContain("size = Vector2(32, 32)");
    expect(first).toContain('uid="uid://player1"');
  });

  it("removing a node never prunes same-named nodes elsewhere in the tree", () => {
    const scene = `[gd_scene load_steps=2 format=3 uid="uid://player1"]\n\n[node name="Root" type="Node2D"]\n\n[node name="Camera" type="Camera2D" parent="Root"]\n\n[node name="Camera" type="Camera2D" parent="."]\n`;
    const model = parseScene(scene);
    const operations = validateMutationOperations([{ op: "remove_node", nodePath: "Root/Camera" }]);
    const after = parseScene(serializeScene(applySceneOperations(model, operations)));
    // Only the descendant under Root is removed; the sibling Camera survives.
    expect(after.nodes.filter((node) => node.name === "Camera")).toHaveLength(1);
    const survivor = after.nodes.find((node) => node.name === "Camera")!;
    expect(survivor.parentPath).toBe(".");
  });

  it("rejects operations that contradict the parsed document", () => {
    const model = parseScene(PLAYER_SCENE);
    expect(() =>
      applySceneOperations(model, [
        {
          op: "set_property",
          nodePath: "Missing/Node",
          property: "x",
          value: { kind: "integer", value: 1 },
        },
      ]),
    ).toThrow(/node not found/);
    expect(() =>
      applySceneOperations(model, [
        { op: "set_script_attachment", nodePath: "Player", extResourceId: "missing_id" },
      ]),
    ).toThrow(/does not exist/);
    expect(() => validateMutationOperations([])).toThrow(/at least one/);
  });
});

describe("resource mutation — apply, serialize, reparse, verify", () => {
  it("sets properties and mutates subresources with verification", () => {
    const model = parseResource(STATS_RESOURCE);
    const operations = validateMutationOperations([
      { op: "set_property", property: "max_hp", value: { kind: "integer", value: 150 } },
      {
        op: "update_subresource",
        id: "Gradient_1",
        properties: [{ name: "width", value: { kind: "float", value: 2.5 } }],
      },
    ]);
    const after = parseResource(serializeResource(applyResourceOperations(model, operations)));
    const verification = verifyResourceSemanticEffect(after, expectedSemanticEffect(operations));
    expect(verification.status).toBe("verified");
    expect(after.properties.find((property) => property.name === "max_hp")?.value).toEqual({
      kind: "integer",
      value: 150,
    });
    expect(
      after.subResources[0]!.properties.find((property) => property.name === "width")?.value,
    ).toEqual({
      kind: "float",
      value: 2.5,
    });
    // The untouched gradient offset raw text is preserved.
    expect(serializeResource(after)).toContain("offsets = PackedFloat32Array(0, 1)");
    expect(after.uid).toBe("uid://stats01");
  });

  it("rejects scene-only operations on resource documents", () => {
    const model = parseResource(STATS_RESOURCE);
    expect(() =>
      applyResourceOperations(model, [{ op: "add_node", name: "X", type: "Node" }]),
    ).toThrow(/not valid on a resource document/);
  });
});

describe("prepared godot mutation", () => {
  it("binds a deterministic fingerprint over revision, operations, and serialized output", () => {
    const model = parseScene(PLAYER_SCENE);
    const operations = validateMutationOperations([
      {
        op: "set_property",
        nodePath: "Player/Sprite",
        property: "visible",
        value: { kind: "boolean", value: false },
      },
    ]);
    const after = serializeScene(applySceneOperations(model, operations));
    const prepared = createPreparedGodotMutation({
      targetPath: "scenes/player.tscn",
      sourceRevision: REV,
      sourceSha256: "a".repeat(64),
      kind: "scene",
      operations,
      expectedSemanticEffect: expectedSemanticEffect(operations),
      preview: {
        structuralSummary: "set Player/Sprite.visible = false",
        diff: `--- a/scenes/player.tscn\n+++ b/scenes/player.tscn\n@@ -1 +1 @@\n-visible = true\n+visible = false\n`,
      },
      serializedAfter: after,
      addedLines: 1,
      removedLines: 1,
    });
    expect(prepared.fingerprint).toMatch(/^[0-9a-f]{64}$/);
    // Any material change produces a new identity (old approval invalid).
    const changedRevision = computeMutationFingerprint({
      targetPath: prepared.targetPath,
      sourceRevision: "rev_".padEnd(36, "b"),
      sourceSha256: prepared.sourceSha256,
      kind: prepared.kind,
      operations: prepared.operations,
      serializedAfter: prepared.serializedAfter,
    });
    expect(changedRevision).not.toBe(prepared.fingerprint);
    expect(Object.isFrozen(prepared)).toBe(true);
  });

  it("serializes structured variant values deterministically", () => {
    expect(serializeVariantValue({ kind: "null" })).toBe("null");
    expect(serializeVariantValue({ kind: "boolean", value: true })).toBe("true");
    expect(serializeVariantValue({ kind: "string", value: 'a"b\\c\nd' })).toBe('"a\\"b\\\\c\\nd"');
    expect(serializeVariantValue({ kind: "vector", typeName: "Vector2", components: [1, 2] })).toBe(
      "Vector2(1, 2)",
    );
    expect(serializeVariantValue({ kind: "ext_resource", id: "1_p" })).toBe('ExtResource("1_p")');
    expect(
      serializeVariantValue({
        kind: "array",
        items: [
          { kind: "integer", value: 1 },
          { kind: "string", value: "x" },
        ],
      }),
    ).toBe('[1, "x"]');
  });
});
