import { describe, expect, it } from "vitest";
import type { GodotKnowledge } from "@siralos/core";
import { createGodotApiSearchTool } from "./godot-api-search-tool.js";

function readyKnowledge(): GodotKnowledge {
  return {
    support: () =>
      Promise.resolve({ state: "unavailable", reason: "unavailable", platform: "linux" }),
    refresh: () => Promise.resolve({ status: "unavailable", message: "unavailable" }),
    search: () =>
      Promise.resolve({
        status: "ready",
        engineVersion: "4.7.1.stable.official",
        results: [
          {
            symbol: "class:Node/property:owner",
            kind: "property",
            name: "owner",
            owner: "Node",
            summary: "The owner of this node.",
            rank: "exact",
            apiType: "native",
          },
        ],
        truncated: false,
      }),
    lookup: () => Promise.resolve({ status: "not_found", message: "not found" }),
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

function unavailableKnowledge(): GodotKnowledge {
  const ready = readyKnowledge();
  return {
    ...ready,
    search: () =>
      Promise.resolve({
        status: "unavailable",
        message:
          "No Godot API knowledge is loaded: exact-engine API generation is unavailable on this platform.",
      }),
  };
}

describe("createGodotApiSearchTool", () => {
  it("returns bounded exact-version search results", async () => {
    const tool = createGodotApiSearchTool(readyKnowledge());
    const result = await tool.execute({ query: "Node owner", kinds: ["property"], limit: 10 }, {});
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect(result.output).toMatchObject({
      engineVersion: "4.7.1.stable.official",
      results: [
        {
          symbol: "class:Node/property:owner",
          kind: "property",
          name: "owner",
          owner: "Node",
          summary: "The owner of this node.",
          rank: "exact",
          apiType: "native",
        },
      ],
      truncated: false,
    });
    expect(JSON.stringify(result.output)).not.toContain(".siralos");
    expect(JSON.stringify(result.output)).not.toContain("extension_api.json");
  });

  it("requires a query", async () => {
    const tool = createGodotApiSearchTool(readyKnowledge());
    const missing = await tool.execute({}, {});
    expect(missing.status).toBe("invalid_input");
    const blank = await tool.execute({ query: "" }, {});
    expect(blank.status).toBe("invalid_input");
  });

  it("rejects malformed input", async () => {
    const tool = createGodotApiSearchTool(readyKnowledge());
    expect((await tool.execute("query", {})).status).toBe("invalid_input");
    expect((await tool.execute({ query: 42 }, {})).status).toBe("invalid_input");
    expect((await tool.execute({ query: "x", kinds: "class" }, {})).status).toBe("invalid_input");
    expect((await tool.execute({ query: "x", limit: 0 }, {})).status).toBe("invalid_input");
  });

  it("maps an unavailable knowledge service to a typed unavailable result", async () => {
    const tool = createGodotApiSearchTool(unavailableKnowledge());
    const result = await tool.execute({ query: "Node" }, {});
    expect(result.status).toBe("unavailable");
    if (result.status === "unavailable") {
      expect(result.message).toContain("unavailable");
    }
  });
});
