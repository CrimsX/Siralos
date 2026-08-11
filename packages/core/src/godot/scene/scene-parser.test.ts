import { describe, expect, it } from "vitest";
import {
  GODOT_SCENE_LIMITS,
  parseGodotResource,
  parseGodotScene,
  buildSceneNodeTree,
  nodesInGroup,
} from "../../index.js";

const BASIC_SCENE = `[gd_scene load_steps=4 format=3 uid="uid://basic123"]

[ext_resource type="Script" path="res://scripts/player.gd" id="1_abc"]
[ext_resource type="PackedScene" uid="uid://weapon99" path="res://scenes/weapon.tscn" id="2_xyz"]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_1"]
size = Vector2(32, 48)

[node name="Player" type="CharacterBody2D"]
script = ExtResource("1_abc")
collision_layer = 1

[node name="Collision" type="CollisionShape2D" parent="."]
shape = SubResource("RectangleShape2D_1")

[node name="Weapon" type="Node2D" parent="." instance=ExtResource("2_xyz")]
`;

describe("parseGodotScene", () => {
  it("parses a basic .tscn into the correct scene model", () => {
    const result = parseGodotScene(BASIC_SCENE, "scenes/player.tscn", { revision: "rev_abc" });
    expect(result.status).toBe("complete");
    expect(result.kind).toBe("scene");
    expect(result.revision).toBe("rev_abc");
    const model = result.document;
    expect(model).not.toBeNull();
    expect(model!.format).toBe(3);
    expect(model!.loadSteps).toBe(4);
    expect(model!.uid).toBe("uid://basic123");
    expect(model!.externalResources).toHaveLength(2);
    expect(model!.subResources).toHaveLength(1);
    expect(model!.nodes).toHaveLength(3);
    expect(model!.baseScene).toBeUndefined();
    expect(model!.connections).toHaveLength(0);
    expect(result.diagnostics).toHaveLength(0);
  });

  it("builds the correct node hierarchy", () => {
    const result = parseGodotScene(BASIC_SCENE, "scenes/player.tscn");
    const tree = buildSceneNodeTree(result.document!);
    expect(tree.root?.node.name).toBe("Player");
    expect(tree.root?.children.map((child) => child.node.name)).toEqual(["Collision", "Weapon"]);
    // Addressing convention: "." = root, root-level children by bare name,
    // nested nodes by full chain.
    expect(tree.paths).toEqual([".", "Collision", "Weapon"]);
    expect(tree.nodesByPath.get("Weapon")?.node.instance).toBeDefined();
  });

  it("keeps node parent and owner distinct", () => {
    const source = `[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Child" type="Sprite2D" parent="." owner="."]

[node name="Nested" type="Node2D" parent="Child" owner="Root"]
`;
    const result = parseGodotScene(source, "scenes/owner.tscn");
    expect(result.status).toBe("complete");
    const model = result.document!;
    expect(model.nodes[1]!.parentPath).toBe(".");
    expect(model.nodes[1]!.ownerPath).toBe(".");
    expect(model.nodes[2]!.parentPath).toBe("Child");
    expect(model.nodes[2]!.ownerPath).toBe("Root");
    // parent != owner for the nested node
    expect(model.nodes[2]!.parentPath).not.toBe(model.nodes[2]!.ownerPath);
  });

  it("resolves script attachments to workspace paths", () => {
    const result = parseGodotScene(BASIC_SCENE, "scenes/player.tscn");
    const player = result.document!.nodes[0]!;
    expect(player.script).toBeDefined();
    expect(player.script!.resource.path).toBe("res://scripts/player.gd");
    expect(player.script!.resolvedPath).toBe("scripts/player.gd");
    expect(player.properties.find((property) => property.name === "script")?.value.kind).toBe(
      "ext_resource",
    );
  });

  it("identifies an external PackedScene instance separately from inheritance", () => {
    const result = parseGodotScene(BASIC_SCENE, "scenes/player.tscn");
    const weapon = result.document!.nodes[2]!;
    expect(weapon.instance).toBeDefined();
    expect(weapon.instance!.kind).toBe("scene");
    expect(weapon.instance!.resource.uid).toBe("uid://weapon99");
    expect(weapon.instance!.resolvedPath).toBe("scenes/weapon.tscn");
    // The root node has no instance, so there is no base scene.
    expect(result.document!.baseScene).toBeUndefined();
  });

  it("identifies scene inheritance as a base scene (root instance), distinct from child instances", () => {
    const source = `[gd_scene load_steps=3 format=3]

[ext_resource type="PackedScene" uid="uid://base001" path="res://scenes/base.tscn" id="1_base"]
[ext_resource type="PackedScene" path="res://scenes/child_weapon.tscn" id="2_wep"]

[node name="Player" instance=ExtResource("1_base")]

[node name="Weapon" parent="." instance=ExtResource("2_wep")]
`;
    const result = parseGodotScene(source, "scenes/player_variant.tscn");
    expect(result.status).toBe("complete");
    const model = result.document!;
    expect(model.baseScene).toBeDefined();
    expect(model.baseScene!.resolvedPath).toBe("scenes/base.tscn");
    expect(model.baseScene!.resource.uid).toBe("uid://base001");
    expect(model.nodes[0]!.instance).toBeDefined();
    expect(model.nodes[1]!.instance!.resolvedPath).toBe("scenes/child_weapon.tscn");
    // Inheritance and instancing are distinct relationships.
    expect(model.baseScene!.resolvedPath).not.toBe(model.nodes[1]!.instance!.resolvedPath);
  });

  it("keeps subresource identity file-local", () => {
    const source = `[gd_scene format=3]

[sub_resource type="RectangleShape2D" id="RectangleShape2D_1"]
size = Vector2(4, 4)

[node name="Root" type="Area2D"]

[node name="Shape" type="CollisionShape2D" parent="."]
shape = SubResource("RectangleShape2D_1")
`;
    const first = parseGodotScene(source, "scenes/a.tscn").document!;
    const second = parseGodotScene(source, "scenes/b.tscn").document!;
    expect(first.subResources[0]!.id).toBe("RectangleShape2D_1");
    expect(second.subResources[0]!.id).toBe("RectangleShape2D_1");
    // The same local id in different documents never resolves across files:
    // each model resolves against its own declarations only.
    expect(first.nodes[1]!.properties[0]!.value).toEqual({
      kind: "sub_resource",
      id: "RectangleShape2D_1",
    });
    expect(second.nodes[1]!.properties[0]!.value).toEqual({
      kind: "sub_resource",
      id: "RectangleShape2D_1",
    });
  });

  it("retains path and uid on external resource identity", () => {
    const result = parseGodotScene(BASIC_SCENE, "scenes/player.tscn");
    const script = result.document!.externalResources[0]!;
    expect(script.id).toBe("1_abc");
    expect(script.type).toBe("Script");
    expect(script.path).toBe("res://scripts/player.gd");
    const weapon = result.document!.externalResources[1]!;
    expect(weapon.uid).toBe("uid://weapon99");
    expect(weapon.path).toBe("res://scenes/weapon.tscn");
  });

  it("parses signal connections with source/target/method", () => {
    const source = `[gd_scene format=3]

[node name="Player" type="CharacterBody2D"]

[node name="UI" type="CanvasLayer" parent="."]

[connection signal="died" from="Player" to="UI" method="on_player_died" flags=1 binds=[1, "x"]]
`;
    const result = parseGodotScene(source, "scenes/signals.tscn");
    expect(result.status).toBe("complete");
    const connection = result.document!.connections[0]!;
    expect(connection.signal).toBe("died");
    expect(connection.from).toBe("Player");
    expect(connection.to).toBe("UI");
    expect(connection.method).toBe("on_player_died");
    expect(connection.flags).toBe(1);
    expect(connection.binds).toHaveLength(2);
    expect(connection.binds![0]).toEqual({ kind: "integer", value: 1 });
    expect(connection.binds![1]).toEqual({ kind: "string", value: "x" });
  });

  it("diagnoses a missing signal endpoint without crashing", () => {
    const source = `[gd_scene format=3]

[node name="Player" type="CharacterBody2D"]

[connection signal="died" from="Player" to="Ghost" method="on_ghost"]
`;
    const result = parseGodotScene(source, "scenes/signals.tscn");
    // Serialized-connection existence and structural endpoint validity are
    // separate claims: the connection parses (structure complete) and the
    // unresolved endpoint is reported as a warning diagnostic.
    expect(result.status).toBe("complete");
    expect(result.document).not.toBeNull();
    const codes = result.diagnostics.map((diagnostic) => diagnostic.code);
    expect(codes).toContain("scene.missing_signal_target");
  });

  it("reports malformed scenes honestly as partial or invalid", () => {
    const missingHeader = parseGodotScene(
      '[node name="X" type="Node2D"]\n',
      "scenes/no_header.tscn",
    );
    expect(missingHeader.status).toBe("invalid");
    expect(missingHeader.document).toBeNull();

    const malformedSection = parseGodotScene(
      '[gd_scene format=3]\n[node name="X" type="Node2D"]\n[broken section\n',
      "scenes/broken.tscn",
    );
    expect(malformedSection.status).toBe("partial");
    expect(
      malformedSection.diagnostics.some(
        (diagnostic) => diagnostic.code === "scene.malformed_section",
      ),
    ).toBe(true);
    expect(malformedSection.document).not.toBeNull();
  });

  it("does not create fake nodes or resources from comments or strings", () => {
    const source = `; a comment
[gd_scene format=3]
# another comment

[node name="Root" type="Node2D"]
; comment inside body
metadata/note = ";[not a section]"
custom = "[node name="Fake"]"

[node name="Real" type="Node2D" parent="."]
`;
    const result = parseGodotScene(source, "scenes/comments.tscn");
    expect(result.status).toBe("complete");
    const model = result.document!;
    expect(model.nodes.map((node) => node.name)).toEqual(["Root", "Real"]);
    expect(model.nodes[0]!.properties[0]!.value).toEqual({
      kind: "string",
      value: ";[not a section]",
    });
  });

  it("handles escaped strings and multiline arrays/dictionaries", () => {
    const source = `[gd_scene format=3]

[node name="Root" type="Node2D"]
metadata/display = "line1\\nline2\\ttab \\"quoted\\""
points = [
  Vector2(0, 0),
  Vector2(10, 20)
]
meta = {
  "name": "player",
  "stats": {"hp": 100, "mp": 50}
}
`;
    const result = parseGodotScene(source, "scenes/multiline.tscn");
    expect(result.status).toBe("complete");
    const properties = result.document!.nodes[0]!.properties;
    const display = properties.find((property) => property.name === "metadata/display")!;
    expect(display.value).toEqual({ kind: "string", value: 'line1\nline2\ttab "quoted"' });
    const points = properties.find((property) => property.name === "points")!;
    expect(points.value.kind).toBe("array");
    if (points.value.kind === "array") {
      expect(points.value.items).toHaveLength(2);
      expect(points.value.items[0]).toEqual({
        kind: "vector",
        typeName: "Vector2",
        components: [0, 0],
      });
    }
    const meta = properties.find((property) => property.name === "meta")!;
    expect(meta.value.kind).toBe("dictionary");
    if (meta.value.kind === "dictionary") {
      expect(meta.value.entries).toHaveLength(2);
      const stats = meta.value.entries[1]!.value;
      expect(stats.kind).toBe("dictionary");
    }
  });

  it("preserves property values containing misleading syntax text", () => {
    const source = `[gd_scene format=3]

[node name="Root" type="Node2D"]
message = "looks like [node name=\\"Fake\\"] but is a string"
path_text = "ExtResource(\\"9\\") is text here"
`;
    const result = parseGodotScene(source, "scenes/misleading.tscn");
    expect(result.status).toBe("complete");
    const model = result.document!;
    expect(model.nodes).toHaveLength(1);
    expect(model.nodes[0]!.properties[0]!.value).toEqual({
      kind: "string",
      value: 'looks like [node name="Fake"] but is a string',
    });
    expect(model.nodes[0]!.properties[1]!.value).toEqual({
      kind: "string",
      value: 'ExtResource("9") is text here',
    });
    expect(model.externalResources).toHaveLength(0);
  });

  it("preserves unknown property/value syntax as bounded opaque data", () => {
    const source = `[gd_scene format=3]

[node name="Root" type="Node2D"]
weird = SomeFutureType(1, "two", [3])
unquoted = bare_token_here
`;
    const result = parseGodotScene(source, "scenes/unknown.tscn");
    expect(result.status).toBe("complete");
    const properties = result.document!.nodes[0]!.properties;
    expect(properties[0]!.value.kind).toBe("opaque");
    expect(properties[0]!.rawValue.length).toBeGreaterThan(0);
    expect(properties[1]!.value.kind).toBe("opaque");
  });

  it("diagnoses duplicate and missing resource ids", () => {
    const duplicate = parseGodotScene(
      '[gd_scene format=3]\n[ext_resource type="Script" path="res://a.gd" id="1_x"]\n[ext_resource type="Script" path="res://b.gd" id="1_x"]\n[node name="Root" type="Node2D"]\n',
      "scenes/dup.tscn",
    );
    expect(duplicate.status).toBe("partial");
    expect(
      duplicate.diagnostics.some((diagnostic) => diagnostic.code === "scene.duplicate_resource_id"),
    ).toBe(true);
    expect(duplicate.document!.externalResources).toHaveLength(1);

    const missing = parseGodotScene(
      '[gd_scene format=3]\n[ext_resource type="Script" path="res://a.gd"]\n[node name="Root" type="Node2D"]\n',
      "scenes/missing.tscn",
    );
    expect(
      missing.diagnostics.some((diagnostic) => diagnostic.code === "scene.missing_resource_id"),
    ).toBe(true);
  });

  it("diagnoses unknown resource references and malformed parents", () => {
    const unknown = parseGodotScene(
      '[gd_scene format=3]\n[node name="Root" type="Node2D" instance=ExtResource("7")]\n',
      "scenes/unknown_ref.tscn",
    );
    expect(unknown.status).toBe("partial");
    expect(
      unknown.diagnostics.some(
        (diagnostic) => diagnostic.code === "scene.unknown_resource_reference",
      ),
    ).toBe(true);

    const badParent = parseGodotScene(
      '[gd_scene format=3]\n[node name="Root" type="Node2D"]\n[node name="Orphan" type="Node2D" parent="Missing"]\n',
      "scenes/bad_parent.tscn",
    );
    expect(badParent.status).toBe("complete");
    expect(
      badParent.diagnostics.some((diagnostic) => diagnostic.code === "scene.unresolved_parent"),
    ).toBe(true);
    const tree = buildSceneNodeTree(badParent.document!);
    expect(tree.orphans.map((node) => node.name)).toEqual(["Orphan"]);
  });

  it("records groups and supports group queries", () => {
    const source = `[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Goblin" type="CharacterBody2D" parent="." groups=["enemies", "hittable"]]

[node name="Chest" type="StaticBody2D" parent="." groups=["interactable"]]
`;
    const result = parseGodotScene(source, "scenes/groups.tscn");
    expect(result.status).toBe("complete");
    const tree = buildSceneNodeTree(result.document!);
    const enemies = nodesInGroup(tree, "enemies");
    expect(enemies.map((node) => node.name)).toEqual(["Goblin"]);
    expect(nodesInGroup(tree, "interactable").map((node) => node.name)).toEqual(["Chest"]);
  });

  it("reports explicit bounds for very large scenes instead of crashing", () => {
    const lines: string[] = ["[gd_scene format=3]"];
    const limit = GODOT_SCENE_LIMITS.maxNodes;
    for (let index = 0; index < limit + 10; index += 1) {
      lines.push(`[node name="N${index}" type="Node2D"${index === 0 ? "" : ` parent="."`}]`);
    }
    const result = parseGodotScene(lines.join("\n"), "scenes/huge.tscn");
    expect(result.truncated).toBe(true);
    expect(result.document!.nodes.length).toBeLessThanOrEqual(limit);
    expect(
      result.diagnostics.some((diagnostic) => diagnostic.code === "scene.document_truncated"),
    ).toBe(true);
  });

  it("enforces the resource-count bound with a single truncation diagnostic", () => {
    const lines: string[] = ["[gd_scene format=3]"];
    const limit = GODOT_SCENE_LIMITS.maxResources;
    for (let index = 0; index < limit + 10; index += 1) {
      lines.push(`[ext_resource type="Script" path="res://r${index}.gd" id="r_${index}"]`);
    }
    lines.push('[node name="Root" type="Node2D"]');
    const result = parseGodotScene(lines.join("\n"), "scenes/many_resources.tscn");
    expect(result.truncated).toBe(true);
    expect(result.document!.externalResources.length).toBeLessThanOrEqual(limit);
    // Repeated excess records produce ONE diagnostic, not one per record.
    expect(
      result.diagnostics.filter((diagnostic) => diagnostic.code === "scene.document_truncated"),
    ).toHaveLength(1);
  });

  it("enforces the property-count bound with a single truncation diagnostic", () => {
    const lines: string[] = ["[gd_scene format=3]", '[node name="Root" type="Node2D"]'];
    const limit = GODOT_SCENE_LIMITS.maxProperties;
    for (let index = 0; index < limit + 10; index += 1) {
      lines.push(`prop_${index} = ${index}`);
    }
    const result = parseGodotScene(lines.join("\n"), "scenes/many_properties.tscn");
    expect(result.truncated).toBe(true);
    const totalProperties = result.document!.nodes.reduce(
      (sum, node) => sum + node.properties.length,
      0,
    );
    expect(totalProperties).toBeLessThanOrEqual(limit);
    expect(
      result.diagnostics.filter((diagnostic) => diagnostic.code === "scene.document_truncated"),
    ).toHaveLength(1);
  });

  it("enforces the section-count bound and stops parsing", () => {
    const lines: string[] = ["[gd_scene format=3]"];
    const limit = GODOT_SCENE_LIMITS.maxSections;
    for (let index = 0; index < limit + 10; index += 1) {
      lines.push(`[editable path="Node${index}"]`);
    }
    lines.push('[node name="After" type="Node2D"]');
    const result = parseGodotScene(lines.join("\n"), "scenes/many_sections.tscn");
    expect(result.truncated).toBe(true);
    // Parsing stopped at the section bound: the trailing node was not seen.
    expect(result.document!.nodes).toHaveLength(0);
    expect(
      result.diagnostics.filter((diagnostic) => diagnostic.code === "scene.document_truncated"),
    ).toHaveLength(1);
  });

  it("returns an invalid result for a resource header inside a scene parse", () => {
    const result = parseGodotScene(
      '[gd_resource type="Resource" format=3]\n[resource]\nfoo = 1\n',
      "scenes/not_a_scene.tscn",
    );
    expect(result.status).toBe("invalid");
    expect(result.document).toBeNull();
  });
});

describe("parseGodotResource", () => {
  const STATS_TRES = `[gd_resource type="Resource" load_steps=2 format=3 uid="uid://stats01"]

[ext_resource type="Script" uid="uid://statscr" path="res://scripts/player_stats.gd" id="1_s"]

[sub_resource type="Curve" id="Curve_1"]
_bake_resolution = 10

[resource]
script = ExtResource("1_s")
max_hp = 100
damage_curve = SubResource("Curve_1")
display = "Player Stats"
`;

  it("parses a .tres type, properties, and dependencies correctly", () => {
    const result = parseGodotResource(STATS_TRES, "resources/player_stats.tres", {
      revision: "rev_tres",
    });
    expect(result.status).toBe("complete");
    expect(result.kind).toBe("resource");
    expect(result.revision).toBe("rev_tres");
    const model = result.document!;
    expect(model.type).toBe("Resource");
    expect(model.uid).toBe("uid://stats01");
    expect(model.externalResources).toHaveLength(1);
    expect(model.subResources).toHaveLength(1);
    expect(model.script!.resolvedPath).toBe("scripts/player_stats.gd");
    expect(model.properties).toHaveLength(4);
    const maxHp = model.properties.find((property) => property.name === "max_hp")!;
    expect(maxHp.value).toEqual({ kind: "integer", value: 100 });
  });

  it("parses nested subresources", () => {
    const source = `[gd_resource type="StyleBoxFlat" format=3]

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_outer"]
bg_color = Color(1, 0, 0, 1)

[sub_resource type="StyleBoxFlat" id="StyleBoxFlat_inner"]
bg_color = Color(0, 1, 0, 1)

[resource]
normal = SubResource("StyleBoxFlat_outer")
hover = SubResource("StyleBoxFlat_inner")
`;
    const result = parseGodotResource(source, "ui/theme.tres");
    expect(result.status).toBe("complete");
    expect(result.document!.subResources.map((resource) => resource.id)).toEqual([
      "StyleBoxFlat_outer",
      "StyleBoxFlat_inner",
    ]);
    expect(result.document!.properties[0]!.value).toEqual({
      kind: "sub_resource",
      id: "StyleBoxFlat_outer",
    });
  });

  it("preserves UID identity where declared", () => {
    const result = parseGodotResource(STATS_TRES, "resources/player_stats.tres");
    expect(result.document!.uid).toBe("uid://stats01");
    expect(result.document!.externalResources[0]!.uid).toBe("uid://statscr");
  });

  it("returns invalid for a scene header inside a resource parse", () => {
    const result = parseGodotResource(
      '[gd_scene format=3]\n[node name="Root" type="Node2D"]\n',
      "resources/not_a_resource.tres",
    );
    expect(result.status).toBe("invalid");
  });

  it("enforces the property-count bound for resources with a single diagnostic", () => {
    const lines: string[] = ['[gd_resource type="Resource" format=3]', "[resource]"];
    const limit = GODOT_SCENE_LIMITS.maxProperties;
    for (let index = 0; index < limit + 10; index += 1) {
      lines.push(`prop_${index} = ${index}`);
    }
    const result = parseGodotResource(lines.join("\n"), "resources/many_properties.tres");
    expect(result.truncated).toBe(true);
    expect(result.document!.properties.length).toBeLessThanOrEqual(limit);
    expect(
      result.diagnostics.filter((diagnostic) => diagnostic.code === "resource.document_truncated"),
    ).toHaveLength(1);
  });
});
