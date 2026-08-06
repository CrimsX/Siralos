import type { Tool, ToolDefinition } from "./tool.js";

export interface ToolRegistry {
  definitions(): readonly ToolDefinition[];

  get(name: string): Tool | undefined;
}

export function createToolRegistry(tools: readonly Tool[]): ToolRegistry {
  const byName = new Map<string, Tool>();
  const definitions: ToolDefinition[] = [];
  for (const tool of tools) {
    if (byName.has(tool.definition.name)) {
      throw new Error(`Duplicate tool name: ${tool.definition.name}`);
    }
    byName.set(tool.definition.name, tool);
    definitions.push(tool.definition);
  }
  return {
    definitions(): readonly ToolDefinition[] {
      return [...definitions];
    },
    get(name: string): Tool | undefined {
      return byName.get(name);
    },
  };
}
