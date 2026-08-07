import {
  createDeterministicFakeProvider,
  createWorkspaceListTool,
  createWorkspaceReadTool,
  createWorkspaceSearchTool,
  getDefaultUserConfigPath,
  getSandboxDirectories,
  loadUserConfig,
  resolveWorkspaceRoot,
  createAnthropicSandboxRuntimeBackend,
} from "@solaris/adapters";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createSolarisSecurity,
  createToolRegistry,
  getBuiltInProfile,
  type SandboxBackend,
  type SolarisApplication,
  type SolarisSecurity,
  type ToolDefinition,
} from "@solaris/core";

export interface CliApplication {
  readonly providerId: string;
  readonly application: SolarisApplication;
  readonly workspaceRoot: string;
  readonly tools: readonly ToolDefinition[];
  readonly security: SolarisSecurity;
  readonly sandbox: SandboxBackend;
}

export async function createCliApplication(): Promise<CliApplication> {
  const config = await loadUserConfig(getDefaultUserConfigPath());
  const profile = getBuiltInProfile(config.sandbox.profile);
  const policy = createDefaultPolicy(config.sandbox.profile);
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const sandboxDirectories = getSandboxDirectories();
  const sandbox = createAnthropicSandboxRuntimeBackend({
    workspaceRoot,
    sandboxHome: sandboxDirectories.home,
    sandboxTemp: sandboxDirectories.temp,
  });
  const security = createSolarisSecurity({ backend: sandbox, policy, profile });
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
    security,
    sandbox,
  };
}
