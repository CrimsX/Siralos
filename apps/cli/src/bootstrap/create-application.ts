import {
  createAnthropicSandboxRuntimeBackend,
  createDeterministicFakeProvider,
  createFilesystemCheckpointStore,
  createGitCliAdapter,
  createGitDiffTool,
  createGitStatusTool,
  createMutationLock,
  createUndoService,
  createWorkspaceCreateFileTool,
  createWorkspaceDeleteFileTool,
  createWorkspaceEditFileTool,
  createWorkspaceListTool,
  createWorkspaceReadTool,
  createWorkspaceSearchTool,
  getDefaultUserConfigPath,
  getSandboxDirectories,
  loadUserConfig,
  reconcileWorkspaceCheckpoints,
  resolveWorkspaceRoot,
} from "@solaris/adapters";
import {
  createDefaultPolicy,
  createSolarisApplication,
  createSolarisSecurity,
  createToolRegistry,
  getBuiltInProfile,
  type ApprovalReviewer,
  type CheckpointStore,
  type GitInspector,
  type RegisteredToolInfo,
  type SandboxBackend,
  type SolarisApplication,
  type SolarisSecurity,
  type UndoService,
} from "@solaris/core";

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
  readonly checkpoints: CheckpointStore;
  readonly git: GitInspector;
  readonly undo: UndoService;
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
  const checkpoints = await createFilesystemCheckpointStore({ workspaceRoot });
  await reconcileWorkspaceCheckpoints({ workspaceRoot, store: checkpoints });
  const git = createGitCliAdapter({ workspaceRoot });
  const reviewer: ApprovalReviewer = options.reviewer ?? {
    review(): Promise<{ type: "deny" }> {
      return Promise.resolve({
        type: "deny",
        reason: "No interactive approval reviewer is configured.",
      });
    },
  };
  const undo = createUndoService({
    workspaceRoot,
    store: checkpoints,
    lock: mutationLock,
    reviewer,
  });
  const sandboxAvailable = (await sandbox.inspect()).state === "available";
  const workspaceTools = [
    createWorkspaceListTool(workspaceRoot),
    createWorkspaceReadTool(workspaceRoot),
    createWorkspaceSearchTool(workspaceRoot),
    createWorkspaceCreateFileTool(workspaceRoot, mutationLock, checkpoints),
    createWorkspaceEditFileTool(workspaceRoot, mutationLock, checkpoints),
    createWorkspaceDeleteFileTool(workspaceRoot, mutationLock, checkpoints),
  ];
  if (sandboxAvailable) {
    workspaceTools.push(createGitStatusTool(git));
    workspaceTools.push(createGitDiffTool(git));
  }
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
    checkpoints,
    git,
    undo,
  };
}
