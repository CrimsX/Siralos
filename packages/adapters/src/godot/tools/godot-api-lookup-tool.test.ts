import { describe, expect, it } from "vitest";
import type { GodotKnowledge } from "@solaris/core";
import { createGodotApiLookupTool } from "./godot-api-lookup-tool.js";

function readyKnowledge(): GodotKnowledge {
  return {
    support: () =>
      Promise.resolve({ state: "unavailable", reason: "unavailable", platform: "linux" }),
    refresh: () => Promise.resolve({ status: "unavailable", message: "unavailable" }),
    search: () => Promise.resolve({ status: "unavailable", message: "unavailable" }),
    lookup: (symbol: string) =>
      symbol === "class:Node/method:add_child"
        ? Promise.resolve({
            status: "ready",
            engineVersion: "4.7.1.stable.official",
            result: {
              symbol: "class:Node/method:add_child",
              kind: "method",
              name: "add_child",
              owner: "Node",
              inheritedFrom: null,
              signature:
                "add_child(node: Node, force_readable_name := false, internal := 0) -> void",
              description: "Adds a child node.",
              apiType: "native",
              details: { returnType: "void" },
            },
          })
        : Promise.resolve({
            status: "not_found",
            message: `Unknown API symbol ${symbol}.`,
          }),
    status: () => ({
      state: "unavailable",
      reason: "unavailable",
      platform: "linux",
      profile: null,
      cacheEnabled: false,
      schemaVersion: 1,
      manualChannel: null,
    }),
  };
}

describe("createGodotApiLookupTool", () => {
  it("returns the bounded structured documentation with the engine version", async () => {
    const tool = createGodotApiLookupTool(readyKnowledge());
    const result = await tool.execute({ symbol: "class:Node/method:add_child" }, {});
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect(result.output).toMatchObject({
      symbol: "class:Node/method:add_child",
      engineVersion: "4.7.1.stable.official",
      signature: "add_child(node: Node, force_readable_name := false, internal := 0) -> void",
      description: "Adds a child node.",
      owner: "Node",
      inheritedFrom: null,
      details: { returnType: "void" },
    });
  });

  it("returns a structured not-found result for an unknown symbol", async () => {
    const tool = createGodotApiLookupTool(readyKnowledge());
    const result = await tool.execute({ symbol: "class:Node/method:nope" }, {});
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect(result.output).toMatchObject({
      found: false,
      symbol: "class:Node/method:nope",
      engineVersion: null,
    });
  });

  it("rejects a missing or empty symbol", async () => {
    const tool = createGodotApiLookupTool(readyKnowledge());
    expect((await tool.execute({}, {})).status).toBe("invalid_input");
    expect((await tool.execute({ symbol: " " }, {})).status).toBe("invalid_input");
    expect((await tool.execute({ symbol: 42 }, {})).status).toBe("invalid_input");
  });
});
