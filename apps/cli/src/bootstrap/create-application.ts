import { homedir } from "node:os";
import { join } from "node:path";
import {
  createAnthropicSandboxRuntimeBackend,
  createDeterministicFakeProvider,
  createEngineProfileCache,
  createFailClosedChangeSetFilePrimitives,
  createFilesystemCheckpointStore,
  createGDScriptDevelopmentService,
  createGitCliAdapter,
  createGitDiffTool,
  createGitStatusTool,
  createGodotApiLookupTool,
  createGodotApiSearchTool,
  createGodotCheckProjectScriptsTool,
  createGodotCheckScriptTool,
  createGodotCompleteTool,
  createGodotDefinitionTool,
  createGodotDevelopmentStatusTool,
  createGodotDiagnosticsService,
  createGDScriptLanguageService,
  createGodotHoverTool,
  createGodotLSPDiagnosticsTool,
  createGodotLSPSessionTool,
  createGodotInspectEngineTool,
  createGodotInspectProjectTool,
  createGodotInspector,
  createGodotKnowledgeCache,
  createGodotKnowledgeService,
  createGodotProbeProjectTool,
  createGodotProbeRunner,
  createGodotProjectProbeService,
  createMutationLock,
  createNpmScriptRunner,
  createNodeScriptRunner,
  createProcessRunTool,
  createRunDirectoryProvider,
  createSha256CommandDigestService,
  createUndoService,
  createWorkspaceApplyTextChangesetTool,
  createWorkspaceCreateFileTool,
  createWorkspaceDeleteFileTool,
  createWorkspaceEditFileTool,
  createWorkspaceListTool,
  createWorkspaceReadTool,
  createWorkspaceSearchTool,
  DEFAULT_CHECKPOINT_ROOT,
  getDefaultUserConfigPath,
  loadUserConfig,
  readGodotEnvironmentOverrides,
  readParentEnvironment,
  reconcileWorkspaceCheckpoints,
  resolveGodotSelection,
  resolveWorkspaceRoot,
} from "@solaris/adapters";
import {
  createCommandRunnerRegistry,
  createDefaultPolicy,
  createSolarisApplication,
  createSolarisSecurity,
  createToolRegistry,
  getBuiltInProfile,
  VALIDATION_OFFLINE_PROFILE,
  type ApprovalReviewer,
  type CheckpointStore,
  type CommandRunnerRegistry,
  type GDScriptDevelopmentService,
  type GitInspector,
  type GodotInspector,
  type GodotProjectProbe,
  type GDScriptLanguageService,
  type GodotKnowledge,
  type GodotDiagnostics,
  type RegisteredToolInfo,
  type SandboxBackend,
  type SolarisApplication,
  type SolarisSecurity,
  type UndoService,
} from "@solaris/core";
import { GodotSelectionError } from "@solaris/adapters";

export interface CreateCliApplicationOptions {
  readonly reviewer?: ApprovalReviewer;
  /** `--godot-path` override. */
  readonly godotPath?: string;
  /** `--godot-installation` override. */
  readonly godotInstallation?: string;
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
  readonly godot: GodotInspector;
  readonly godotProbe: GodotProjectProbe;
  readonly knowledge: GodotKnowledge;
  readonly diagnostics: GodotDiagnostics;
  readonly language: GDScriptLanguageService;
  readonly development: GDScriptDevelopmentService;
  readonly undo: UndoService;
  readonly runners: CommandRunnerRegistry;
}

export async function createCliApplication(
  options: CreateCliApplicationOptions = {},
): Promise<CliApplication> {
  const config = await loadUserConfig(getDefaultUserConfigPath());
  const profile = getBuiltInProfile(config.sandbox.profile);
  const policy = createDefaultPolicy(config.sandbox.profile);
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const runsRoot = join(homedir(), ".solaris", "runs");
  const sandbox = createAnthropicSandboxRuntimeBackend({ workspaceRoot });
  const security = createSolarisSecurity({ backend: sandbox, policy, profile });
  const mutationLock = createMutationLock();
  const checkpoints = await createFilesystemCheckpointStore({ workspaceRoot });
  await reconcileWorkspaceCheckpoints({ workspaceRoot, store: checkpoints });
  const runDirectories = createRunDirectoryProvider({ workspaceRoot, runsRoot });
  const git = createGitCliAdapter({
    workspaceRoot,
    backend: sandbox,
    runDirectories,
  });
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
  const digest = createSha256CommandDigestService();
  const runners = createCommandRunnerRegistry([
    createNpmScriptRunner({ digest }),
    createNodeScriptRunner({ digest }),
  ]);
  const processTool = createProcessRunTool({
    workspaceRoot,
    runners,
    backend: sandbox,
    runDirectories,
    lock: mutationLock,
    git,
    executionProfile: VALIDATION_OFFLINE_PROFILE,
    executionPolicy: createDefaultPolicy("validation-offline"),
  });
  const environmentOverrides = readGodotEnvironmentOverrides();
  const resolvedSelection = resolveGodotSelection({
    cliPath: options.godotPath ?? null,
    cliInstallationId: options.godotInstallation ?? null,
    environmentPath: environmentOverrides.path,
    environmentInstallationId: environmentOverrides.installationId,
    config: config.godot,
  });
  if (!resolvedSelection.ok) {
    throw new GodotSelectionError(resolvedSelection.message);
  }
  const overrideSource =
    options.godotPath !== undefined || options.godotInstallation !== undefined
      ? ("cli" as const)
      : environmentOverrides.path !== null || environmentOverrides.installationId !== null
        ? ("environment" as const)
        : null;
  const parentEnvironment = readParentEnvironment();
  const godotProbe = createGodotProjectProbeService({
    workspaceRoot,
    config: config.godot,
    preference: resolvedSelection.preference,
    overrideSource,
    backend: sandbox,
    probeRunner: createGodotProbeRunner({
      backend: sandbox,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      parentEnvironment,
    }),
    cache: createEngineProfileCache({}),
    hostPath: parentEnvironment["PATH"] ?? null,
    hostPathExt: parentEnvironment["PATHEXT"] ?? null,
    platform: process.platform,
    runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
    checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
    git,
    parentEnvironment,
  });
  const godot = createGodotInspector({
    config: config.godot,
    preference: resolvedSelection.preference,
    overrideSource,
    workspaceRoot,
    backend: sandbox,
    probeRunner: createGodotProbeRunner({
      backend: sandbox,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      parentEnvironment,
    }),
    cache: createEngineProfileCache({}),
    hostPath: parentEnvironment["PATH"] ?? null,
    hostPathExt: parentEnvironment["PATHEXT"] ?? null,
    platform: process.platform,
    recoveryProbe: godotProbe,
  });
  const knowledge = createGodotKnowledgeService({
    workspaceRoot,
    config: config.godot,
    preference: resolvedSelection.preference,
    overrideSource,
    backend: sandbox,
    probeRunner: createGodotProbeRunner({
      backend: sandbox,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      parentEnvironment,
    }),
    cache: createGodotKnowledgeCache({}),
    engineProfileCache: createEngineProfileCache({}),
    hostPath: parentEnvironment["PATH"] ?? null,
    hostPathExt: parentEnvironment["PATHEXT"] ?? null,
    platform: process.platform,
    parentEnvironment,
  });
  const diagnostics = createGodotDiagnosticsService({
    workspaceRoot,
    config: config.godot,
    preference: resolvedSelection.preference,
    overrideSource,
    backend: sandbox,
    probeRunner: createGodotProbeRunner({
      backend: sandbox,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      parentEnvironment,
    }),
    cache: createEngineProfileCache({}),
    hostPath: parentEnvironment["PATH"] ?? null,
    hostPathExt: parentEnvironment["PATHEXT"] ?? null,
    platform: process.platform,
    runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
    checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
    parentEnvironment,
  });
  const language = createGDScriptLanguageService({
    workspaceRoot,
    config: config.godot,
    preference: resolvedSelection.preference,
    overrideSource,
    backend: sandbox,
    probeRunner: createGodotProbeRunner({
      backend: sandbox,
      runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
      parentEnvironment,
    }),
    cache: createEngineProfileCache({}),
    hostPath: parentEnvironment["PATH"] ?? null,
    hostPathExt: parentEnvironment["PATHEXT"] ?? null,
    platform: process.platform,
    runDirectories: createRunDirectoryProvider({ workspaceRoot, runsRoot }),
    checkpointRoot: DEFAULT_CHECKPOINT_ROOT,
    parentEnvironment,
  });
  const development = createGDScriptDevelopmentService({
    workspaceRoot,
    platform: process.platform,
    store: checkpoints,
    lock: mutationLock,
    language,
    diagnostics,
    git,
    canApplyIdentityBound: false,
    primitives: createFailClosedChangeSetFilePrimitives(),
  });
  const workspaceTools = [
    createWorkspaceListTool(workspaceRoot),
    createWorkspaceReadTool(workspaceRoot),
    createWorkspaceSearchTool(workspaceRoot),
    createWorkspaceCreateFileTool(workspaceRoot, mutationLock, checkpoints),
    createWorkspaceEditFileTool(workspaceRoot, mutationLock, checkpoints),
    createWorkspaceDeleteFileTool(workspaceRoot, mutationLock, checkpoints),
    createWorkspaceApplyTextChangesetTool(development),
    createGodotInspectEngineTool(godot),
    createGodotInspectProjectTool(godot),
    createGodotProbeProjectTool(godotProbe),
    createGodotApiSearchTool(knowledge),
    createGodotApiLookupTool(knowledge),
    createGodotCheckScriptTool(diagnostics),
    createGodotCheckProjectScriptsTool(diagnostics),
    createGodotLSPSessionTool(language),
    createGodotLSPDiagnosticsTool(language, () => development.languageQueryGate()),
    createGodotHoverTool(language, () => development.languageQueryGate()),
    createGodotCompleteTool(language, () => development.languageQueryGate()),
    createGodotDefinitionTool(language, () => development.languageQueryGate()),
    createGodotDevelopmentStatusTool(development),
    createGitStatusTool(git),
    createGitDiffTool(git),
    processTool,
  ];
  const registry = createToolRegistry(workspaceTools);
  const provider = createDeterministicFakeProvider();
  const application = createSolarisApplication({
    provider,
    tools: registry,
    policy,
    profile,
    ...(options.reviewer === undefined ? {} : { reviewer: options.reviewer }),
    onProviderTurnCompleted: () => {
      development.completeFromProviderTurn();
    },
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
    godot,
    godotProbe,
    knowledge,
    diagnostics,
    language,
    development,
    undo,
    runners,
  };
}
