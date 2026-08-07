import type { Capability } from "../security/capability.js";
import type { ToolDefinition } from "./tool.js";
import type { RegisteredTool } from "./prepared-mutation-tool.js";
import { toolCapability } from "./prepared-mutation-tool.js";

export interface RegisteredToolInfo {
  readonly definition: ToolDefinition;
  readonly capability: Capability;
}

export interface ToolRegistry {
  definitions(): readonly RegisteredToolInfo[];

  get(name: string): RegisteredTool | undefined;
}

export function createToolRegistry(tools: readonly RegisteredTool[]): ToolRegistry {
  const byName = new Map<string, RegisteredTool>();
  const definitions: RegisteredToolInfo[] = [];
  for (const tool of tools) {
    if (byName.has(tool.definition.name)) {
      throw new Error(`Duplicate tool name: ${tool.definition.name}`);
    }
    byName.set(tool.definition.name, tool);
    definitions.push({ definition: tool.definition, capability: toolCapability(tool) });
  }
  return {
    definitions(): readonly RegisteredToolInfo[] {
      return [...definitions];
    },
    get(name: string): RegisteredTool | undefined {
      return byName.get(name);
    },
  };
}
