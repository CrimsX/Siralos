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
  createProviderChangeReviewer,
  createQualityValidationExecutor,
  createReviewerToolRegistry,
  createRunDirectoryProvider,
  createSha256CommandDigestService,
  createUndoService,
  createValidationPlanDiscovery,
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
  TASK_RUNTIME_VERSION,
  capabilityPolicyFingerprint,
  createCommandRunnerRegistry,
  createDefaultPolicy,
  createProjectionService,
  createRouteContextCapacity,
  createSolarisApplication,
  createSolarisSecurity,
  createTaskRuntime,
  createToolProjector,
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
  type ModelProvider,
  type RegisteredToolInfo,
  type SandboxBackend,
  type ProjectionService,
  type SolarisApplication,
  type SolarisSecurity,
  type TaskRuntime,
  type TaskRuntimeSnapshotSources,
  type UndoService,
} from "@solaris/core";
import { GodotSelectionError } from "@solaris/adapters";
import { resolveReviewProviderId } from "./review-provider.js";

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
  readonly tasks: TaskRuntime;
  readonly taskSources: TaskRuntimeSnapshotSources;
  readonly projection: ProjectionService;
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
  const developmentHolder: { current: GDScriptDevelopmentService | null } = { current: null };
  const qualityStage = buildQualityStage({
    workspaceRoot,
    git,
    godot,
    knowledge,
    language,
    reviewer,
    processTool,
    reviewProviderId: config.quality.reviewProvider,
    toolProjector: createToolProjector({ policy, profile }),
    // Late-bound: the reviewer's read-only LSP query tools consult the
    // workflow's language-query gate, which exists only after the
    // development service is created (no composition cycle).
    languageQueryGate: () =>
      developmentHolder.current?.languageQueryGate() ?? { blocked: false, message: null },
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
    qualityStage,
  });
  developmentHolder.current = development;
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
    createGodotLSPSessionTool(language, () => {
      const workflow = development.status().session;
      return workflow !== null && workflow.state.kind === "active"
        ? {
            blocked: true,
            message:
              "The development workflow manages the language session lifecycle; its one-time approval covers LSP recreation after approved edits.",
          }
        : { blocked: false, message: null };
    }),
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
  const tasks = createTaskRuntime();
  const taskSources: TaskRuntimeSnapshotSources = {
    runtimeVersion: TASK_RUNTIME_VERSION,
    provider: { profileId: provider.id, route: null },
    sandboxProfileId: profile.id,
    capabilityPolicyRevision: capabilityPolicyFingerprint(policy),
    workspaceIdentity: workspaceRoot,
    godotEngineFingerprint: null,
    workflow: null,
  };
  const projection = createProjectionService({
    policy,
    profile,
    capacity: createRouteContextCapacity("develop-offline"),
    getTaskSnapshot: () => tasks.latestTask()?.snapshot() ?? null,
    getTaskRequest: () => tasks.latestTask()?.contract().request ?? null,
  });
  const application = createSolarisApplication({
    provider,
    tools: registry,
    policy,
    profile,
    projection,
    ...(options.reviewer === undefined ? {} : { reviewer: options.reviewer }),
    onProviderTurnCompleted: () => {
      development.completeFromProviderTurn();
    },
  });
  return {
    providerId: provider.id,
    application,
    workspaceRoot,
    tasks,
    taskSources,
    projection,
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

/** Registered provider profiles available for the independent reviewer. */
const REVIEW_PROVIDER_FACTORIES: Readonly<Record<string, () => ModelProvider>> = {
  "deterministic-fake": () => createDeterministicFakeProvider(),
};

const DEFAULT_REVIEW_PROVIDER_ID = "deterministic-fake";

/**
 * Composition of the quality stage (ADR 0013 §26–§27): the validation
 * plan discovery and executor (project commands still require their own
 * one-time process approval through the interactive reviewer) and the
 * independent reviewer over a FRESH provider context with a strictly
 * read-only tool registry. The review provider defaults to the active
 * development provider profile; an explicitly configured profile that
 * does not exist fails clearly and never silently falls back to an
 * unrelated provider. No new credential system is introduced.
 */
function buildQualityStage(options: {
  readonly workspaceRoot: string;
  readonly git: GitInspector;
  readonly godot: GodotInspector;
  readonly knowledge: GodotKnowledge;
  readonly language: GDScriptLanguageService;
  readonly reviewer: ApprovalReviewer;
  readonly processTool: import("@solaris/core").PreparedCommandTool;
  readonly reviewProviderId: string | null;
  readonly toolProjector: import("@solaris/core").ToolProjector;
  readonly languageQueryGate: () => { readonly blocked: boolean; readonly message: string | null };
}): NonNullable<Parameters<typeof createGDScriptDevelopmentService>[0]["qualityStage"]> {
  const resolved = resolveReviewProviderId({
    configured: options.reviewProviderId,
    registered: new Set(Object.keys(REVIEW_PROVIDER_FACTORIES)),
    defaultId: DEFAULT_REVIEW_PROVIDER_ID,
  });
  if (!resolved.ok) {
    throw new Error(resolved.message);
  }
  const providerFactory = REVIEW_PROVIDER_FACTORIES[resolved.providerId] as () => ModelProvider;
  const reviewer = createProviderChangeReviewer({
    providerFactory,
    tools: createReviewerToolRegistry({
      workspaceRoot: options.workspaceRoot,
      git: options.git,
      godot: options.godot,
      knowledge: options.knowledge,
      language: options.language,
      languageQueryGate: options.languageQueryGate,
    }),
    toolProjector: options.toolProjector,
  });
  return {
    reviewer,
    validation: {
      discovery: createValidationPlanDiscovery({ workspaceRoot: options.workspaceRoot }),
      executor: createQualityValidationExecutor({
        processTool: options.processTool,
        reviewer: options.reviewer,
      }),
    },
  };
}
