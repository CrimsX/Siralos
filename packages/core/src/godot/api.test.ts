import { describe, expect, it } from "vitest";
import { godotSymbolId, type GodotApiSymbolKind } from "../index.js";

describe("godotSymbolId", () => {
  it("produces the documented deterministic identities", () => {
    expect(godotSymbolId({ kind: "class", name: "Node" })).toBe("class:Node");
    expect(godotSymbolId({ kind: "method", name: "add_child", owner: "Node" })).toBe(
      "class:Node/method:add_child",
    );
    expect(godotSymbolId({ kind: "property", name: "owner", owner: "Node" })).toBe(
      "class:Node/property:owner",
    );
    expect(godotSymbolId({ kind: "signal", name: "ready", owner: "Node" })).toBe(
      "class:Node/signal:ready",
    );
    expect(godotSymbolId({ kind: "method", name: "length", owner: "Vector2" })).toBe(
      "class:Vector2/method:length",
    );
    expect(godotSymbolId({ kind: "enum", name: "Error" })).toBe("global:enum:Error");
    expect(godotSymbolId({ kind: "constant", name: "PI" })).toBe("global:constant:PI");
    expect(godotSymbolId({ kind: "utility", name: "lerp" })).toBe("utility:lerp");
  });

  it("distinguishes overloaded members with deterministic ordinals", () => {
    expect(godotSymbolId({ kind: "method", name: "get_node", owner: "Node", ordinal: 2 })).toBe(
      "class:Node/method:get_node#2",
    );
    expect(godotSymbolId({ kind: "method", name: "get_node", owner: "Node", ordinal: 3 })).toBe(
      "class:Node/method:get_node#3",
    );
  });

  it("does not append an ordinal for the first (unique) symbol", () => {
    expect(godotSymbolId({ kind: "method", name: "add_child", owner: "Node", ordinal: 1 })).toBe(
      "class:Node/method:add_child",
    );
  });

  it("scopes class enums and constants to their owning class", () => {
    expect(godotSymbolId({ kind: "enum", name: "ProcessMode", owner: "Node" })).toBe(
      "class:Node/enum:ProcessMode",
    );
    expect(godotSymbolId({ kind: "constant", name: "NOTIFICATION_READY", owner: "Node" })).toBe(
      "class:Node/constant:NOTIFICATION_READY",
    );
  });

  it("keeps built-in class members in the class namespace with the engine-native name", () => {
    expect(godotSymbolId({ kind: "class", name: "Vector2" })).toBe("class:Vector2");
    expect(godotSymbolId({ kind: "operator", name: "+", owner: "Vector2" })).toBe(
      "class:Vector2/operator:+",
    );
    expect(godotSymbolId({ kind: "operator", name: "[]", owner: "Array" })).toBe(
      "class:Array/operator:[]",
    );
  });

  it("is stable for the same inputs and contains no paths or provider ids", () => {
    const kinds: readonly GodotApiSymbolKind[] = [
      "class",
      "method",
      "property",
      "signal",
      "constant",
      "enum",
      "utility",
      "operator",
    ];
    for (const kind of kinds) {
      const first = godotSymbolId({ kind, name: "Example", owner: "Node" });
      const second = godotSymbolId({ kind, name: "Example", owner: "Node" });
      expect(first).toBe(second);
      // Symbol ids contain the namespace separator `/` but never a
      // filesystem path shape: no backslashes and no drive-letter prefix.
      expect(first).not.toMatch(/\\|^[a-z]:[\\/]/i);
    }
  });

  it("never assumes method names alone are globally unique", () => {
    const node = godotSymbolId({ kind: "method", name: "duplicate", owner: "Node" });
    const resource = godotSymbolId({ kind: "method", name: "duplicate", owner: "Resource" });
    expect(node).not.toBe(resource);
    expect(node).toBe("class:Node/method:duplicate");
    expect(resource).toBe("class:Resource/method:duplicate");
  });
});
