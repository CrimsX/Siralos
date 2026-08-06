import { describe, expect, it } from "vitest";
import { createToolRegistry, type Tool, type ToolExecutionResult } from "../index.js";

function createStubTool(name: string): Tool {
  return {
    definition: { name, description: `Stub ${name}`, inputSchema: {} },
    execute(): Promise<ToolExecutionResult> {
      return Promise.resolve({ status: "success", output: { ok: true }, summary: "ok" });
    },
  };
}

describe("createToolRegistry", () => {
  it("rejects duplicate tool names", () => {
    expect(() => createToolRegistry([createStubTool("a.tool"), createStubTool("a.tool")])).toThrow(
      "Duplicate tool name: a.tool",
    );
  });

  it("exposes definitions immutably", () => {
    const registry = createToolRegistry([createStubTool("a.tool")]);
    const first = registry.definitions();
    const second = registry.definitions();
    expect(first).toEqual([{ name: "a.tool", description: "Stub a.tool", inputSchema: {} }]);
    expect(first).not.toBe(second);
  });

  it("resolves a registered tool by exact name", () => {
    const tool = createStubTool("a.tool");
    const registry = createToolRegistry([tool]);
    expect(registry.get("a.tool")).toBe(tool);
  });

  it("does not resolve unknown names to another tool", () => {
    const registry = createToolRegistry([createStubTool("a.tool")]);
    expect(registry.get("a.tool.x")).toBeUndefined();
    expect(registry.get("b.tool")).toBeUndefined();
  });
});
