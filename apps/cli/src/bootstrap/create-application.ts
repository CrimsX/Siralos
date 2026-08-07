import {
  createDeterministicFakeProvider,
  createWorkspaceListTool,
  createWorkspaceReadTool,
  createWorkspaceSearchTool,
  createWorkspaceCreateFileTool,
  createWorkspaceEditFileTool,
  createWorkspaceDeleteFileTool,
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
  type ApprovalReviewer,
  type RegisteredToolInfo,
  type SandboxBackend,
  type SolarisApplication,
  type SolarisSecurity,
} from "@solaris/core";
import { createMutationLock } from "@solaris/adapters";

export interface CreateCliApplicationOptions {
  readonly reviewer?: ApprovalReviewer;
}

export interface CliApplication {
  readonly providerId: string;
  readonly application: SolarisApplication;
  readonly workspaceRoot: string;
  readonly tools: readonly RegisteredToolInfo[];
  readonly security: SolarisSecurity;
  readonly sandbox: SandboxBackend;
}

export async function createCliApplication(
  options: CreateCliApplicationOptions = {},
): Promise<CliApplication> {
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
  const mutationLock = createMutationLock();
  const workspaceTools = [
    createWorkspaceListTool(workspaceRoot),
    createWorkspaceReadTool(workspaceRoot),
    createWorkspaceSearchTool(workspaceRoot),
    createWorkspaceCreateFileTool(workspaceRoot, mutationLock),
    createWorkspaceEditFileTool(workspaceRoot, mutationLock),
    createWorkspaceDeleteFileTool(workspaceRoot, mutationLock),
  ];
  const registry = createToolRegistry(workspaceTools);
  const provider = createDeterministicFakeProvider();
  const application = createSolarisApplication({
    provider,
    tools: registry,
    policy,
    profile,
    ...(options.reviewer === undefined ? {} : { reviewer: options.reviewer }),
  });
  return {
    providerId: provider.id,
    application,
    workspaceRoot,
    tools: registry.definitions(),
    security,
    sandbox,
  };
}
