import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { GODOT_LIMITS } from "@siralos/core";
import { parseGodotApiDumpWithDocs, truncateUtf8Bytes } from "./api-dump-with-docs.js";
import { buildGodotApiIndex, searchGodotApiIndex, lookupGodotApiSymbol } from "./api-index.js";

function fixtureContent(): Buffer {
  const fixturePath = path.join(
    path.dirname(fileURLToPath(import.meta.url)),
    "fixtures",
    "extension-api-with-docs.fixture.json",
  );
  return readFileSync(fixturePath);
}

function buildFixtureIndex() {
  const parsed = parseGodotApiDumpWithDocs(fixtureContent());
  if (!parsed.ok) {
    throw new Error(`fixture failed to parse: ${parsed.message}`);
  }
  const built = buildGodotApiIndex(parsed.document);
  if (!built.ok) {
    throw new Error(`fixture failed to build: ${built.message}`);
  }
  return { document: parsed.document, index: built.index };
}

describe("parseGodotApiDumpWithDocs", () => {
  it("parses the with-docs dump header and document fields", () => {
    const parsed = parseGodotApiDumpWithDocs(fixtureContent());
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) {
      return;
    }
    expect(parsed.document.versionFullName).toBe("4.7.1.stable.official");
    expect(parsed.document.hash).toBe("0123456789abcdef");
    expect(parsed.document.classes.map((entry) => entry.name)).toEqual(["Node", "CharacterBody2D"]);
    expect(parsed.document.builtinClasses.map((entry) => entry.name)).toEqual(["Vector2"]);
    expect(parsed.document.globalEnums.map((entry) => entry.name)).toEqual(["Error"]);
    expect(parsed.document.globalConstants.map((entry) => entry.name)).toEqual(["PI"]);
    expect(parsed.document.utilityFunctions.map((entry) => entry.name)).toEqual(["lerp"]);
    expect(parsed.document.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(parsed.document.rawBytes).toBe(fixtureContent().length);
  });

  it("preserves documentation, parameters, defaults, qualifiers, and hashes", () => {
    const parsed = parseGodotApiDumpWithDocs(fixtureContent());
    if (!parsed.ok) {
      return;
    }
    const node = parsed.document.classes.find((entry) => entry.name === "Node");
    expect(node?.briefDescription).toContain("Base class for all scene objects");
    expect(node?.description).toContain("most important building blocks");
    const addChild = node?.methods.find((entry) => entry.name === "add_child");
    expect(addChild?.returnType).toBe("void");
    expect(addChild?.hash).toBe("724948260");
    expect(addChild?.parameters).toEqual([
      { name: "node", type: "Node", defaultValue: null },
      { name: "force_readable_name", type: "bool", defaultValue: "false" },
      { name: "internal", type: "int", defaultValue: "0" },
    ]);
    expect(addChild?.qualifiers).toEqual([]);
    expect(addChild?.description).toContain("Adds a child node");
    const owner = node?.properties.find((entry) => entry.name === "owner");
    expect(owner?.setter).toBe("set_owner");
    expect(owner?.getter).toBe("get_owner");
    expect(node?.signals.find((entry) => entry.name === "ready")?.description).toContain(
      "enters the tree",
    );
    expect(node?.constants.find((entry) => entry.name === "NOTIFICATION_READY")?.value).toBe("13");
    expect(node?.enums.find((entry) => entry.name === "ProcessMode")?.values).toEqual([
      { name: "PROCESS_MODE_INHERIT", value: "0" },
      { name: "PROCESS_MODE_PAUSABLE", value: "1" },
    ]);
  });

  it("indexes built-in operators and constants", () => {
    const parsed = parseGodotApiDumpWithDocs(fixtureContent());
    if (!parsed.ok) {
      return;
    }
    const vector2 = parsed.document.builtinClasses[0];
    expect(vector2?.operators.map((entry) => entry.name)).toEqual(["+"]);
    expect(vector2?.constants.map((entry) => entry.name)).toEqual(["ZERO"]);
  });

  it("rejects invalid JSON and non-object roots", () => {
    expect(parseGodotApiDumpWithDocs(Buffer.from("{not json"))).toEqual({
      ok: false,
      message: "The API documentation dump is not valid JSON.",
    });
    expect(parseGodotApiDumpWithDocs(Buffer.from("[1,2,3]"))).toEqual({
      ok: false,
      message: "The API documentation dump is not a JSON object.",
    });
  });

  it("tolerates unknown fields and missing documentation conservatively", () => {
    const minimal = Buffer.from(
      JSON.stringify({
        header: { version_full_name: "4.7.1.stable.official", hash: "h" },
        classes: [
          {
            name: "Empty",
            base_class: "Object",
            api_type: "core",
            some_unknown_field: { nested: [1, 2, 3] },
            methods: [{ name: "no_docs" }],
            properties: [],
            signals: [],
            constants: [],
            enums: [],
          },
        ],
      }),
    );
    const parsed = parseGodotApiDumpWithDocs(minimal);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) {
      return;
    }
    const empty = parsed.document.classes[0];
    expect(empty?.briefDescription).toBeNull();
    expect(empty?.description).toBeNull();
    expect(empty?.methods[0]?.description).toBeNull();
    expect(empty?.methods[0]?.returnType).toBeNull();
    expect(empty?.methods[0]?.hash).toBeNull();
  });

  it("bounds descriptions to the immutable limit", () => {
    const huge = "x".repeat(GODOT_LIMITS.maxApiDescriptionBytes + 1024);
    const parsed = parseGodotApiDumpWithDocs(
      Buffer.from(
        JSON.stringify({
          classes: [
            {
              name: "Huge",
              description: huge,
              methods: [],
              properties: [],
              signals: [],
              constants: [],
              enums: [],
            },
          ],
        }),
      ),
    );
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) {
      return;
    }
    expect(Buffer.byteLength(parsed.document.classes[0]?.description ?? "", "utf8")).toBe(
      GODOT_LIMITS.maxApiDescriptionBytes,
    );
  });
});

describe("buildGodotApiIndex", () => {
  it("indexes classes, methods, properties, signals, constants, enums, utilities, and built-ins with deterministic ids", () => {
    const { index } = buildFixtureIndex();
    const ids = index.symbols.map((symbol) => symbol.id);
    expect(ids).toContain("class:Node");
    expect(ids).toContain("class:Node/method:add_child");
    expect(ids).toContain("class:Node/method:get_node");
    expect(ids).toContain("class:Node/property:owner");
    expect(ids).toContain("class:Node/signal:ready");
    expect(ids).toContain("class:Node/constant:NOTIFICATION_READY");
    expect(ids).toContain("class:Node/enum:ProcessMode");
    expect(ids).toContain("class:CharacterBody2D/method:move_and_slide");
    expect(ids).toContain("class:Vector2");
    expect(ids).toContain("class:Vector2/method:length");
    expect(ids).toContain("class:Vector2/operator:+");
    expect(ids).toContain("class:Vector2/constant:ZERO");
    expect(ids).toContain("global:enum:Error");
    expect(ids).toContain("global:constant:PI");
    expect(ids).toContain("utility:lerp");
    expect(ids.length).toBe(new Set(ids).size);
  });

  it("preserves inheritance, engine version, and exact engine-native names", () => {
    const { index } = buildFixtureIndex();
    const node = index.symbols.find((symbol) => symbol.id === "class:Node");
    expect(node?.inheritedFrom).toBe("Object");
    expect(node?.apiType).toBe("native");
    expect(index.engineVersion).toBe("4.7.1.stable.official");
    expect(node?.name).toBe("Node");
  });

  it("keeps documentation bounded and builds signatures with defaults", () => {
    const { index } = buildFixtureIndex();
    const addChild = index.symbols.find((symbol) => symbol.id === "class:Node/method:add_child");
    expect(addChild?.signature).toBe(
      "add_child(node: Node, force_readable_name: bool := false, internal: int := 0) -> void",
    );
    expect(addChild?.details.returnType).toBe("void");
    expect(addChild?.details.hash).toBe("724948260");
    expect(addChild?.details.parameters?.[0]).toEqual({
      name: "node",
      type: "Node",
      defaultValue: null,
    });
    expect(addChild?.description).toContain("Adds a child node");
    const utility = index.symbols.find((symbol) => symbol.id === "utility:lerp");
    expect(utility?.signature).toBe("lerp(from: float, to: float, weight: float) -> float");
  });

  it("distinguishes overloaded members with deterministic ordinals", () => {
    const parsed = parseGodotApiDumpWithDocs(
      Buffer.from(
        JSON.stringify({
          classes: [
            {
              name: "Overloaded",
              methods: [
                { name: "same", return_type: "int", arguments: [], hash: "1" },
                { name: "same", return_type: "int", arguments: [], hash: "2" },
              ],
              properties: [],
              signals: [],
              constants: [],
              enums: [],
            },
          ],
        }),
      ),
    );
    if (!parsed.ok) {
      return;
    }
    const built = buildGodotApiIndex(parsed.document);
    expect(built.ok).toBe(true);
    if (!built.ok) {
      return;
    }
    const ids = built.index.symbols.map((symbol) => symbol.id);
    expect(ids).toContain("class:Overloaded/method:same");
    expect(ids).toContain("class:Overloaded/method:same#2");
  });

  it("fails safely when the class or symbol limits are exceeded", () => {
    const tooManyClasses = Buffer.from(
      JSON.stringify({
        classes: Array.from({ length: GODOT_LIMITS.maxApiClasses + 1 }, (_, index) => ({
          name: `C${index}`,
          methods: [],
          properties: [],
          signals: [],
          constants: [],
          enums: [],
        })),
      }),
    );
    const parsed = parseGodotApiDumpWithDocs(tooManyClasses);
    if (!parsed.ok) {
      return;
    }
    expect(buildGodotApiIndex(parsed.document)).toMatchObject({ ok: false });
  });

  it("fails safely on an oversized dump", () => {
    const parsed = parseGodotApiDumpWithDocs(fixtureContent());
    if (!parsed.ok) {
      return;
    }
    const oversized = {
      ...parsed.document,
      rawBytes: GODOT_LIMITS.maxApiDumpWithDocsBytes + 1,
    };
    expect(buildGodotApiIndex(oversized)).toMatchObject({ ok: false });
  });
});

describe("searchGodotApiIndex", () => {
  it("ranks an exact class name first", () => {
    const { index } = buildFixtureIndex();
    const outcome = searchGodotApiIndex(index, "Node");
    expect(outcome.results[0]).toMatchObject({
      symbol: "class:Node",
      kind: "class",
      name: "Node",
      rank: "exact",
    });
  });

  it("ranks an exact method name first within its class", () => {
    const { index } = buildFixtureIndex();
    const outcome = searchGodotApiIndex(index, "move_and_slide", { kinds: ["method"] });
    expect(outcome.results[0]).toMatchObject({
      symbol: "class:CharacterBody2D/method:move_and_slide",
      rank: "exact",
    });
  });

  it("finds prefix matches", () => {
    const { index } = buildFixtureIndex();
    const outcome = searchGodotApiIndex(index, "add_ch");
    expect(outcome.results.some((entry) => entry.symbol === "class:Node/method:add_child")).toBe(
      true,
    );
    expect(outcome.results[0]?.rank).toBe("prefix");
  });

  it("finds token and document matches", () => {
    const { index } = buildFixtureIndex();
    const token = searchGodotApiIndex(index, "child");
    expect(token.results.some((entry) => entry.symbol === "class:Node/method:add_child")).toBe(
      true,
    );
    const document = searchGodotApiIndex(index, "physics body characters");
    expect(document.results.some((entry) => entry.symbol === "class:CharacterBody2D")).toBe(true);
  });

  it("matches case-insensitively", () => {
    const { index } = buildFixtureIndex();
    const outcome = searchGodotApiIndex(index, "nOdE");
    expect(outcome.results[0]?.symbol).toBe("class:Node");
  });

  it("applies the kind filter", () => {
    const { index } = buildFixtureIndex();
    const outcome = searchGodotApiIndex(index, "Node", { kinds: ["property"] });
    expect(outcome.results.length).toBeGreaterThan(0);
    expect(outcome.results.every((entry) => entry.kind === "property")).toBe(true);
  });

  it("bounds and truncates results deterministically", () => {
    const { index } = buildFixtureIndex();
    const first = searchGodotApiIndex(index, "e", { limit: 3 });
    const second = searchGodotApiIndex(index, "e", { limit: 3 });
    expect(first.results).toEqual(second.results);
    expect(first.results.length).toBeLessThanOrEqual(3);
    expect(first.truncated).toBe(true);
    const capped = searchGodotApiIndex(index, "e", { limit: 10_000 });
    expect(capped.results.length).toBeLessThanOrEqual(GODOT_LIMITS.maxApiSearchResults);
  });

  it("exposes no cache paths in results", () => {
    const { index } = buildFixtureIndex();
    const outcome = searchGodotApiIndex(index, "Node");
    const serialized = JSON.stringify(outcome.results);
    expect(serialized).not.toMatch(/\.siralos|C:\\|knowledge[\\/]/);
  });
});

describe("lookupGodotApiSymbol", () => {
  it("looks up a class with inheritance and the engine version", () => {
    const { index } = buildFixtureIndex();
    const result = lookupGodotApiSymbol(index, "class:Node");
    expect(result).toMatchObject({
      symbol: "class:Node",
      kind: "class",
      name: "Node",
      owner: null,
      inheritedFrom: "Object",
    });
  });

  it("looks up a method with its signature and description", () => {
    const { index } = buildFixtureIndex();
    const result = lookupGodotApiSymbol(index, "class:Node/method:add_child");
    expect(result?.signature).toContain("add_child(node: Node");
    expect(result?.description).toContain("Adds a child node");
    expect(result?.owner).toBe("Node");
  });

  it("looks up properties, signals, enums, and utilities", () => {
    const { index } = buildFixtureIndex();
    expect(lookupGodotApiSymbol(index, "class:Node/property:owner")?.name).toBe("owner");
    expect(lookupGodotApiSymbol(index, "class:Node/signal:ready")?.kind).toBe("signal");
    expect(lookupGodotApiSymbol(index, "class:Node/enum:ProcessMode")?.details.values).toEqual([
      { name: "PROCESS_MODE_INHERIT", value: "0" },
      { name: "PROCESS_MODE_PAUSABLE", value: "1" },
    ]);
    expect(lookupGodotApiSymbol(index, "global:enum:Error")?.owner).toBeNull();
    expect(lookupGodotApiSymbol(index, "utility:lerp")?.signature).toContain("-> float");
  });

  it("returns null for unknown symbols", () => {
    const { index } = buildFixtureIndex();
    expect(lookupGodotApiSymbol(index, "class:Node/method:does_not_exist")).toBeNull();
    expect(lookupGodotApiSymbol(index, "class:Node/property:owner#2")).toBeNull();
  });

  it("keeps descriptions within the immutable description bound", () => {
    const { index } = buildFixtureIndex();
    const result = lookupGodotApiSymbol(index, "class:Node");
    expect(Buffer.byteLength(result?.description ?? "", "utf8")).toBeLessThanOrEqual(
      GODOT_LIMITS.maxApiDescriptionBytes,
    );
  });
});

describe("truncateUtf8Bytes", () => {
  it("never splits a code point and returns exact byte bounds", () => {
    expect(truncateUtf8Bytes("héllo", 4)).toBe("h\u00e9l");
    expect(Buffer.byteLength(truncateUtf8Bytes("😀😀😀", 5), "utf8")).toBe(4);
    expect(truncateUtf8Bytes("short", 1024)).toBe("short");
  });
});
