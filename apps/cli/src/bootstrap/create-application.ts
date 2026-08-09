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
  createGitHubResearchSource,
  createGodotDocsResearchSource,
  createMutationLock,
  createNodeHttpsTransport,
  createNpmScriptRunner,
  createNodeScriptRunner,
  createProcessRunTool,
  createProviderChangeReviewer,
  createQualityValidationExecutor,
  createReferenceServices,
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
  createProjectInstructionService,
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
  buildGodotProjectKnowledgeCandidates,
  canonicalizeJson,
  capabilityPolicyFingerprint,
  createCommandRunnerRegistry,
  createKnowledgeCoordinator,
  createWorkspaceRevisionRegistry,
  resolveInstructionSet,
  sha256Hex,
  createDefaultPolicy,
  createProjectionService,
  createRouteContextCapacity,
  createSolarisApplication,
  createSolarisSecurity,
  createTaskRuntime,
  createToolProjector,
  createToolRegistry,
  createResearchService,
  getBuiltInProfile,
  parseReferenceDeclarationsSection,
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
  type KnowledgeCoordinator,
  type ModelProvider,
  type ProjectInstructionService,
  type ReferenceMaterializerPort,
  type ReferenceRegistry,
  type RegisteredToolInfo,
  type ResearchService,
  type ResearchSourcePort,
  type SandboxBackend,
  type ProjectionService,
  type SolarisApplication,
  type SolarisSecurity,
  type TaskRuntime,
  type TaskRuntimeSnapshotSources,
  type Tool,
  type UndoService,
  type WorkspaceRevisionRegistry,
} from "@solaris/core";
import { GodotSelectionError } from "@solaris/adapters";
import { resolveReviewProviderId } from "./review-provider.js";
import {
  createReferenceEvidenceRing,
  createResearchTools,
  observeReferenceTools,
} from "./reference-research.js";

export interface CreateCliApplicationOptions {
  readonly reviewer?: ApprovalReviewer;
  /** `--godot-path` override. */
  readonly godotPath?: string;
  /** `--godot-installation` override. */
  readonly godotInstallation?: string;
  /**
   * User config path override (tests/smoke). Defaults to
   * `getDefaultUserConfigPath()`.
   */
  readonly configPath?: string;
}

export interface CliApplication {
  readonly providerId: string;
  readonly application: SolarisApplication;
  readonly workspaceRoot: string;
  readonly tasks: TaskRuntime;
  readonly taskSources: TaskRuntimeSnapshotSources;
  readonly projection: ProjectionService;
  readonly revisions: WorkspaceRevisionRegistry;
  readonly workspaceRead: Tool;
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
  readonly instructions: ProjectInstructionService;
  readonly projectKnowledge: KnowledgeCoordinator;
  readonly references: ReferenceRegistry;
  readonly referenceMaterializer: ReferenceMaterializerPort;
  /** Precise reason the references config failed semantic parse; null when clean. */
  readonly referenceConfigError: string | null;
  readonly research: ResearchService;
  /** The configured research source ports (kind/id/label, in registration order). */
  readonly researchSources: readonly ResearchSourcePort[];
  /** Releases the reference services (currently a no-op; kept for interface stability). */
  close(): void;
}

export async function createCliApplication(
  options: CreateCliApplicationOptions = {},
): Promise<CliApplication> {
  const config = await loadUserConfig(options.configPath ?? getDefaultUserConfigPath());
  const profile = getBuiltInProfile(config.sandbox.profile);
  const policy = createDefaultPolicy(config.sandbox.profile);
  const workspaceRoot = await resolveWorkspaceRoot(process.cwd());
  const runsRoot = join(homedir(), ".solaris", "runs");
  const sandbox = createAnthropicSandboxRuntimeBackend({ workspaceRoot });
  const security = createSolarisSecurity({ backend: sandbox, policy, profile });
  // References (Stage 3 milestone 5): a config parse failure NEVER crashes
  // startup — the precise reason is surfaced by /references while the
  // registry stays empty (fail closed, nothing half-configured).
  const parsedReferences = parseReferenceDeclarationsSection(
    transformReferencesSection(config.references),
  );
  const referenceConfigError = parsedReferences.ok ? null : parsedReferences.reason;
  const referenceServices = await createReferenceServices({
    declarations: parsedReferences.ok ? parsedReferences.declarations : [],
    workspaceRoot,
    trustFor: () => "explicit-user",
  });
  // Consult the materializer once per ready reference so its status() is
  // truthful from startup (local-directory → "not-required"; repository →
  // "unavailable" at this stage). Zero filesystem operations occur.
  for (const reference of referenceServices.registry.list()) {
    if (reference.status !== "ready") {
      continue;
    }
    const revision = referenceServices.registry.revision(reference.id);
    if (revision !== null) {
      await referenceServices.materializer.materialize(reference.id, revision.identity);
    }
  }
  // Research (Stage 3 milestone 5): the two real sources over the real
  // node:https transport. The research service gates every fetch on the
  // `research.fetch` capability (denied by every built-in profile), so the
  // source ports are never invoked under the default policy.
  const researchTransport = createNodeHttpsTransport();
  const researchSources: readonly ResearchSourcePort[] = [
    createGitHubResearchSource({ transport: researchTransport }),
    createGodotDocsResearchSource({ transport: researchTransport }),
  ];
  const research = createResearchService({ policy, profile, sources: researchSources });
  const referenceEvidenceRing = createReferenceEvidenceRing();
  const referenceTools = observeReferenceTools(
    referenceServices.tools,
    referenceServices.registry,
    referenceEvidenceRing,
  );
  const researchTools = createResearchTools(research);
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
  const revisions = createWorkspaceRevisionRegistry({
    workspaceFingerprint: sha256Hex(canonicalizeJson({ workspaceRoot })),
  });
  const instructions = createProjectInstructionService({ workspaceRoot, revisions });
  await instructions.load();
  const projectKnowledge = createKnowledgeCoordinator();
  // Conservative deterministic seeding (ADR 0017 §42): only facts the
  // static project profile proves. Architectural ownership is never
  // inferred from weak evidence. Every candidate flows through the
  // single-writer coordinator.
  try {
    const profile = await godot.projectProfile();
    for (const candidate of buildGodotProjectKnowledgeCandidates({
      projectFileSha256: profile.projectFileSha256,
      declaredEngineVersionRaw:
        profile.declaredEngineVersion === null ? null : profile.declaredEngineVersion.raw,
      languageProfile: profile.languageProfile === "unknown" ? null : profile.languageProfile,
      hasDotnet: profile.executableContent.dotnetProjectFiles.length > 0,
      projectName: profile.name,
    })) {
      projectKnowledge.propose(candidate);
    }
  } catch {
    // Seeding is best-effort and read-only; a failed static scan never
    // blocks startup and never fabricates facts.
  }
  const development = createGDScriptDevelopmentService({
    workspaceRoot,
    platform: process.platform,
    store: checkpoints,
    lock: mutationLock,
    language,
    diagnostics,
    git,
    revisions,
    canApplyIdentityBound: false,
    primitives: createFailClosedChangeSetFilePrimitives(),
    qualityStage,
  });
  developmentHolder.current = development;
  const workspaceReadTool = createWorkspaceReadTool(workspaceRoot, { revisions });
  const workspaceTools = [
    createWorkspaceListTool(workspaceRoot),
    workspaceReadTool,
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
  // Reference tools register ONLY when at least one reference is READY
  // (no empty/declined tool surface); research tools register always — the
  // ToolProjector hides them under the default deny policy for
  // `research.fetch`.
  const readyReferenceCount = referenceServices.registry
    .list()
    .filter((reference) => reference.status === "ready").length;
  const registeredTools = [
    ...workspaceTools,
    ...(readyReferenceCount > 0 ? referenceTools : []),
    ...researchTools,
  ];
  const registry = createToolRegistry(registeredTools);
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
    instructions: {
      resolve: (focusPaths) => {
        const safe = focusPaths.filter(isSafeRelativeFocusPath);
        const set = resolveInstructionSet({
          instructions: instructions.instructions(),
          paths: safe.length === 0 ? ["."] : safe,
        });
        return set.instructions.length === 0 ? null : set;
      },
    },
    knowledge: {
      pinned: () => projectKnowledge.pinnedFacts(),
      retrieve: (query) => projectKnowledge.retrieve(query),
    },
    references: {
      list: () => referenceServices.registry.list(),
      latestEvidence: () => referenceEvidenceRing.list(),
    },
    research: {
      latestEvidence: () => research.latestEvidence(),
    },
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
    revisions,
    workspaceRead: workspaceReadTool,
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
    instructions,
    projectKnowledge,
    references: referenceServices.registry,
    referenceMaterializer: referenceServices.materializer,
    referenceConfigError,
    research,
    researchSources,
    close(): void {
      referenceServices.close();
    },
  };
}

/**
 * Derive the canonical declaration-form section (alias-keyed, with the
 * alias repeated inside each declaration and the source object nested, as
 * core's `parseReferenceDeclarationsSection` expects) from the raw
 * user-config `references` section (kind + path/repository/ref flattened).
 * The config layer already validated the structural shape defensively; this
 * mapping only re-arranges fields, so it cannot fail. Semantic validation
 * (absolute paths, repository normalization, ref shapes, bounds) is core's
 * job — see packages/adapters/src/config/user-config.ts.
 */
function transformReferencesSection(
  references: Readonly<Record<string, unknown>>,
): Record<string, unknown> {
  const section: Record<string, unknown> = {};
  for (const [alias, declaration] of Object.entries(references)) {
    if (typeof declaration !== "object" || declaration === null || Array.isArray(declaration)) {
      continue;
    }
    const record = declaration as Record<string, unknown>;
    const description =
      typeof record["description"] === "string" ? { description: record["description"] } : {};
    if (record["kind"] === "local-directory") {
      section[alias] = {
        alias,
        kind: "local-directory",
        source: { kind: "local-directory", path: record["path"] },
        ...description,
      };
    } else {
      section[alias] = {
        alias,
        kind: "repository",
        source: {
          kind: "repository",
          repository: record["repository"],
          ...(record["ref"] === undefined ? {} : { ref: record["ref"] }),
        },
        ...description,
      };
    }
  }
  return section;
}

/**
 * Focus paths come from session revision evidence (already canonical
 * workspace-relative paths); the projection wrapper still rejects anything
 * that could not be a safe relative path. The authoritative containment
 * check lives in discovery and the adapter service.
 */
function isSafeRelativeFocusPath(path: string): boolean {
  return (
    path.length > 0 &&
    !path.includes("\0") &&
    !path.includes("\\") &&
    !path.startsWith("/") &&
    !/^[A-Za-z]:/.test(path) &&
    !path.split("/").includes("..") &&
    !path.split("/").includes(".")
  );
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
