import { describe, expect, it } from "vitest";
import { GODOT_SCENE_LIMITS, parseGodotVariant } from "../../index.js";

describe("parseGodotVariant", () => {
  it("parses scalar values", () => {
    expect(parseGodotVariant("null").value).toEqual({ kind: "null" });
    expect(parseGodotVariant("true").value).toEqual({ kind: "boolean", value: true });
    expect(parseGodotVariant("42").value).toEqual({ kind: "integer", value: 42 });
    expect(parseGodotVariant("-7").value).toEqual({ kind: "integer", value: -7 });
    expect(parseGodotVariant("3.5").value).toEqual({ kind: "float", value: 3.5 });
    expect(parseGodotVariant('"hello"').value).toEqual({ kind: "string", value: "hello" });
    expect(parseGodotVariant('&"Name"').value).toEqual({ kind: "string_name", value: "Name" });
    expect(parseGodotVariant('NodePath("Player/Weapon")').value).toEqual({
      kind: "node_path",
      value: "Player/Weapon",
    });
  });

  it("parses resource reference forms", () => {
    expect(parseGodotVariant('ExtResource("2_abc")').value).toEqual({
      kind: "ext_resource",
      id: "2_abc",
    });
    expect(parseGodotVariant('SubResource("Shape_1")').value).toEqual({
      kind: "sub_resource",
      id: "Shape_1",
    });
    expect(parseGodotVariant('Resource("uid://abc123")').value).toEqual({
      kind: "resource",
      uid: "uid://abc123",
    });
    expect(parseGodotVariant('Resource("res://scenes/x.tscn", "PackedScene")').value).toEqual({
      kind: "resource",
      path: "res://scenes/x.tscn",
      type: "PackedScene",
    });
  });

  it("parses vector/color values with bounded components", () => {
    expect(parseGodotVariant("Vector2(1, 2)").value).toEqual({
      kind: "vector",
      typeName: "Vector2",
      components: [1, 2],
    });
    expect(parseGodotVariant("Color(1, 0.5, 0, 1)").value).toEqual({
      kind: "color",
      components: [1, 0.5, 0, 1],
    });
    const transform = parseGodotVariant("Transform2D(1, 0, 0, 1, 10, 20)");
    expect(transform.value.kind).toBe("vector");
  });

  it("parses packed arrays with bounded items", () => {
    const parsed = parseGodotVariant('PackedStringArray("a", "b", "c")');
    expect(parsed.value.kind).toBe("packed_array");
    if (parsed.value.kind === "packed_array") {
      expect(parsed.value.typeName).toBe("PackedStringArray");
      expect(parsed.value.items).toHaveLength(3);
    }
  });

  it("preserves unknown constructor forms as bounded opaque raw", () => {
    const parsed = parseGodotVariant("SomeFutureType(1, [2, 3])");
    expect(parsed.value.kind).toBe("opaque");
    if (parsed.value.kind === "opaque") {
      expect(parsed.value.typeName).toBe("SomeFutureType");
      expect(parsed.value.raw.text).toBe("SomeFutureType(1, [2, 3])");
      expect(parsed.value.raw.truncated).toBe(false);
    }
  });

  it("bounds nested array/dictionary depth", () => {
    let value = "0";
    for (let index = 0; index < GODOT_SCENE_LIMITS.maxVariantDepth + 2; index += 1) {
      value = `[${value}]`;
    }
    const parsed = parseGodotVariant(value);
    expect(parsed.truncated).toBe(true);
    // The value is preserved safely (opaque or partially parsed), never
    // rejected in a way that crashes the surrounding parse.
    expect(["array", "opaque"]).toContain(parsed.value.kind);
  });

  it("bounds array item counts", () => {
    const items = Array.from({ length: GODOT_SCENE_LIMITS.maxArrayItems + 10 }, (_, index) =>
      String(index),
    );
    const parsed = parseGodotVariant(`[${items.join(", ")}]`);
    expect(parsed.truncated).toBe(true);
    if (parsed.value.kind === "array") {
      expect(parsed.value.items.length).toBeLessThanOrEqual(GODOT_SCENE_LIMITS.maxArrayItems);
    }
  });

  it("bounds raw opaque text length", () => {
    const long = "X".repeat(GODOT_SCENE_LIMITS.maxRawValueLength + 100);
    const parsed = parseGodotVariant(long);
    expect(parsed.value.kind).toBe("opaque");
    if (parsed.value.kind === "opaque") {
      expect(parsed.value.raw.text.length).toBe(GODOT_SCENE_LIMITS.maxRawValueLength);
      expect(parsed.value.raw.truncated).toBe(true);
    }
  });
});
