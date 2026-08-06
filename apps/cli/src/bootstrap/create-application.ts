import {
  createDeterministicFakeProvider,
  createWorkspaceListTool,
  createWorkspaceReadTool,
  createWorkspaceSearchTool,
  resolveWorkspaceRoot,
} from "@solaris/adapters";
import {
  createSolarisApplication,
  createToolRegistry,
  type SolarisApplication,
  type ToolDefinition,
} from "@solaris/core";

export interface CliApplication {
  readonly providerId: string;
  readonly application: SolarisApplication;
  readonly workspaceRoot: string;
  readonly tools: readonly ToolDefinition[];
}

export async function createCliApplication(): Promise<CliApplication> {
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const workspaceTools = [
    createWorkspaceListTool(workspaceRoot),
    createWorkspaceReadTool(workspaceRoot),
    createWorkspaceSearchTool(workspaceRoot),
  ];
  const provider = createDeterministicFakeProvider();
  const application = createSolarisApplication({
    provider,
    tools: createToolRegistry(workspaceTools),
  });
  return {
    providerId: provider.id,
    application,
    workspaceRoot,
    tools: workspaceTools.map((tool) => tool.definition),
  };
}
