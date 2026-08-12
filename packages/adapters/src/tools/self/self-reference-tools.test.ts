import { describe, expect, it } from "vitest";
import { createDefaultPolicy } from "@siralos/core";
import { createSelfReference } from "@siralos/core";
import type { SelfReferencePort } from "@siralos/core";
import {
  createSelfReferenceReadTool,
  createSelfReferenceSearchTool,
} from "./self-reference-tools.js";

function makePort(): SelfReferencePort {
  return createSelfReference({
    runtime: { version: "0.0.0", nodeMajor: 24, platform: "win32" },
    registeredTools: [],
    sandboxProfileId: "inspect",
    policy: createDefaultPolicy("inspect"),
  });
}

describe("self.read tool", () => {
  it("returns a bounded section on demand", async () => {
    const port = makePort();
    const tool = createSelfReferenceReadTool(port);
    expect(tool.capability).toBe("self.inspect");
    const result = await tool.execute({ section: "commands" }, {});
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const output = result.output as { sectionId: string; lines: { key: string; value: string }[] };
    expect(output.sectionId).toBe("commands");
    expect(output.lines.some((entry) => entry.key === "/doctor")).toBe(true);
    expect(output.lines.some((entry) => entry.key === "/siralos")).toBe(true);
    expect(result.summary).toContain("commands");
  });

  it("rejects unknown sections with a precise message", async () => {
    const tool = createSelfReferenceReadTool(makePort());
    const result = await tool.execute({ section: "nope" }, {});
    expect(result.status).toBe("invalid_input");
    if (result.status !== "invalid_input") {
      return;
    }
    expect(result.message).toContain("Unknown self-reference section");
  });

  it("validates input shape", async () => {
    const tool = createSelfReferenceReadTool(makePort());
    const result = await tool.execute({}, {});
    expect(result.status).toBe("invalid_input");
  });
});

describe("self.search tool", () => {
  it("returns bounded matches for a topic", async () => {
    const port = makePort();
    const tool = createSelfReferenceSearchTool(port);
    const result = await tool.execute({ query: "GDScript LSP" }, {});
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    const output = result.output as { matches: { sectionId: string; lines: unknown[] }[] };
    expect(output.matches.length).toBeGreaterThan(0);
    expect(output.matches[0]!.sectionId).toBeTruthy();
  });

  it("returns no matches for unknown tokens", async () => {
    const tool = createSelfReferenceSearchTool(makePort());
    const result = await tool.execute({ query: "zzzz-nope" }, {});
    expect(result.status).toBe("success");
    if (result.status !== "success") {
      return;
    }
    expect((result.output as { matches: unknown[] }).matches).toEqual([]);
  });
});
